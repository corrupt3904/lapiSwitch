//! S3: verified original backups. Staging, DeviceInfo and original
//! restoration continue in `recovery_flow`.

use std::path::Path;

use ltbox_core::live;

use crate::avb_info::AvbSummary;
use crate::backup::{self, BackupBuilder, VerifiedBackup};
use crate::prepare::PrepareReport;
use crate::slot::Slot;
use crate::storage::{self, Partition};

pub mod native;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    System,
    Edl,
}

/// All transfers use explicit frozen coordinates, including restoration when
/// the on-device GPT is missing entries or its write failed part-way through.
pub trait StageIo {
    fn ensure_single_target(&mut self) -> Result<(), String>;
    fn read_avb(
        &mut self,
        partition: &str,
        log: &mut Vec<String>,
    ) -> Result<Option<AvbSummary>, String>;
    fn scan(&mut self, log: &mut Vec<String>) -> Result<Vec<Partition>, String>;
    fn validate_snapshot(&mut self, snapshot: &[Partition]) -> Result<(), String>;
    /// A scan omission alone cannot establish absence; only a definitive
    /// PartitionNotFound from a direct read may confirm missing proinfo.
    fn confirm_proinfo_absent(&mut self, log: &mut Vec<String>) -> Result<(), String>;
    fn dump_at(
        &mut self,
        part: &Partition,
        output: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String>;
    fn write_at(
        &mut self,
        part: &Partition,
        image: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String>;
    fn reset(&mut self, mode: ResetMode, log: &mut Vec<String>) -> Result<(), String>;
}

fn identify<D: StageIo>(
    device: &mut D,
    slot: Slot,
    log: &mut Vec<String>,
) -> Result<(String, String), String> {
    let a = device.read_avb("vbmeta_system_a", log)?;
    let b = device.read_avb("vbmeta_system_b", log)?;
    let active = if slot == Slot::A { a } else { b };
    let active = active.ok_or("활성 슬롯의 vbmeta_system 정보를 읽을 수 없습니다.")?;
    active
        .build_fingerprint()
        .filter(|fp| ltbox_core::model::fingerprint_model_match(fp, crate::device::model()))
        .ok_or("EDL 활성 슬롯의 기기 모델이 TB520FU로 확인되지 않았습니다.")?;
    let fingerprint = active.version_fingerprint().unwrap_or_default();
    Ok((fingerprint, format!("{:?}", active.key_class())))
}

fn dump<D: StageIo>(
    device: &mut D,
    part: &Partition,
    path: &Path,
    log: &mut Vec<String>,
) -> Result<(), String> {
    device.dump_at(part, path, log)?;
    let len = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    if len != part.size_bytes()? {
        return Err(format!("덤프 길이가 잘못되었습니다: {}", part.name));
    }
    Ok(())
}

/// Caller saves the ADB identity in the builder directory before S2; this
/// function adds the mandatory original partitions to it.
pub fn take_backup<D: StageIo>(
    device: &mut D,
    prepared: &PrepareReport,
    builder: BackupBuilder,
    log: &mut Vec<String>,
) -> Result<VerifiedBackup, String> {
    take_backup_with_confirmation(device, prepared, builder, &mut || Ok(false), log)
}

/// Only unrecognized ROM metadata is overridable, with an explicit answer
/// before partition backup/staging. Known stock versions above the ceiling
/// and model/transport failures never reach this confirmation.
pub fn take_backup_with_confirmation<D: StageIo>(
    device: &mut D,
    prepared: &PrepareReport,
    mut builder: BackupBuilder,
    confirm_unrecognized: &mut dyn FnMut() -> Result<bool, String>,
    log: &mut Vec<String>,
) -> Result<VerifiedBackup, String> {
    device.ensure_single_target()?;
    live!(log, "S3: 기기를 다시 확인하고 원본 파티션을 백업합니다.");
    let (fingerprint, key) = identify(device, prepared.slot, log)?;
    live!(log, "S3: 기기 서명 키 {key} (표시·기록용)");
    match crate::device_version::check(&fingerprint) {
        Ok(version) => live!(
            log,
            "S3: 활성 슬롯 {} 기기 펌웨어 {version}, 버전 제한 검사 통과",
            prepared.slot.suffix()
        ),
        Err(crate::device_version::VersionError::Unrecognized) => {
            live!(log, "S3: 순정롬 식별 불가 (system_ext 우선): {fingerprint}");
            if !confirm_unrecognized()? {
                return Err(
                    "순정롬 확인 불가 경고에서 작업을 취소했습니다. 기기에는 쓰지 않았습니다."
                        .into(),
                );
            }
            live!(
                log,
                "S3: 사용자가 순정롬 확인 불가 경고를 확인하고 계속 진행을 선택했습니다."
            );
            device.ensure_single_target()?;
        }
        Err(error) => return Err(error.to_string()),
    }
    let snapshot = device.scan(log)?;
    storage::find_partition(&snapshot, "vbmeta_a", 4)?;
    let devinfo = storage::find_partition(&snapshot, "devinfo", 4)?.clone();
    let persist = storage::find_partition(&snapshot, "persist", 0)?.clone();
    let proinfo = if snapshot.iter().any(|part| part.name == "proinfo") {
        Some(storage::find_partition(&snapshot, "proinfo", 0)?.clone())
    } else {
        device.confirm_proinfo_absent(log)?;
        None
    };
    device.validate_snapshot(&snapshot)?;
    let header = Partition {
        name: "gpt_header4".into(),
        lun: 4,
        start_sector: 0,
        num_sectors: 2,
    };
    let header_path = builder.partition_dir().join("gpt_header4.bin");
    dump(device, &header, &header_path, log)?;
    let header_bytes = std::fs::read(&header_path).map_err(|error| error.to_string())?;
    let geometry = storage::primary_geometry(&header_bytes)?;
    let gpt_path = builder.partition_dir().join("gpt_main4.bin");
    dump(device, &geometry, &gpt_path, log)?;
    let bytes = std::fs::read(&gpt_path).map_err(|error| error.to_string())?;
    let gpt = storage::validate_gpt(&bytes)?;
    // Re-read header must agree with the one used to choose the dump length.
    if bytes.get(..header_bytes.len()) != Some(header_bytes.as_slice()) {
        return Err("GPT 헤더가 덤프 도중 변경되었습니다.".into());
    }
    for part in snapshot.iter().filter(|part| part.lun == 4) {
        storage::match_gpt_partition(&gpt, part)?;
    }
    // Same rule as S1: set flags must agree; blank flags keep the ADB slot.
    if gpt.active_slot(Some(prepared.slot))? != prepared.slot {
        return Err("백업 시 GPT 활성 슬롯이 준비 단계와 달라졌습니다.".into());
    }
    gpt.remove_region_vbmeta(prepared.slot)
        .map_err(|error| error.to_string())?;
    builder.record("gpt_main4.bin", Some(geometry))?;
    let originals = [
        Some(devinfo),
        Some(persist),
        proinfo.clone(),
        Some(
            storage::find_partition(&snapshot, &format!("recovery{}", prepared.slot.suffix()), 4)?
                .clone(),
        ),
        Some(storage::find_partition(&snapshot, "misc", 0)?.clone()),
        Some(
            storage::find_partition(
                &snapshot,
                &format!("vendor_boot{}", prepared.slot.suffix()),
                4,
            )?
            .clone(),
        ),
    ];
    for part in originals.into_iter().flatten() {
        let filename = format!("{}.img", part.name);
        dump(device, &part, &builder.partition_dir().join(&filename), log)?;
        builder.record(&filename, Some(part))?;
    }
    if let Some(raw) = &prepared.identity {
        builder.save_identity(raw)?;
        builder.record(backup::IDENTITY_NAME, None)?;
    }
    let backup = builder.finish(prepared, fingerprint, key, snapshot, proinfo.is_none())?;
    live!(
        log,
        "S3: 백업 파일을 다시 읽어 크기와 SHA-256을 확인했습니다: {}",
        backup.path().display()
    );
    Ok(backup)
}
