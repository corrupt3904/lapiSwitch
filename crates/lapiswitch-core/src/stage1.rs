//! Verified original backups and legacy original boot/GPT restoration.
//! New conversions stage TWRP through recovery_flow; custom boot is retired.

use std::path::{Path, PathBuf};

use ltbox_core::live;

use crate::avb_info::AvbSummary;
use crate::backup::{self, BackupBuilder, RecoverySources, TransactionStage, VerifiedBackup};
use crate::firmware::{FirmwareReport, MODEL};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
    NotNeeded,
    Restored,
    Failed,
}

#[derive(Debug, thiserror::Error)]
#[error("{cause}\n백업: {backup_dir}\n복구 결과: {recovery:?}\n{}", .details.join("\n"))]
pub struct StageError {
    pub cause: String,
    pub backup_dir: PathBuf,
    pub recovery: RecoveryOutcome,
    pub details: Vec<String>,
}

impl StageError {
    fn before_write(cause: String, dir: &Path) -> Self {
        Self {
            cause,
            backup_dir: dir.to_path_buf(),
            recovery: RecoveryOutcome::NotNeeded,
            details: vec![],
        }
    }
}

pub struct StageReport {
    pub backup_dir: PathBuf,
    pub warnings: Vec<String>,
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
        .filter(|fp| ltbox_core::model::fingerprint_model_match(fp, MODEL))
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
    firmware: &FirmwareReport,
    builder: BackupBuilder,
    log: &mut Vec<String>,
) -> Result<VerifiedBackup, String> {
    take_backup_with_confirmation(device, prepared, firmware, builder, &mut || Ok(false), log)
}

/// Only unrecognized ROM metadata is overridable, with an explicit answer
/// before partition backup/staging. Known stock versions above the ceiling
/// and model/transport failures never reach this confirmation.
pub fn take_backup_with_confirmation<D: StageIo>(
    device: &mut D,
    prepared: &PrepareReport,
    firmware: &FirmwareReport,
    mut builder: BackupBuilder,
    confirm_unrecognized: &mut dyn FnMut() -> Result<bool, String>,
    log: &mut Vec<String>,
) -> Result<VerifiedBackup, String> {
    device.ensure_single_target()?;
    live!(log, "S3: 기기를 다시 확인하고 원본 파티션을 백업합니다.");
    let (fingerprint, key) = identify(device, prepared.slot, log)?;
    live!(log, "S3: 기기 서명 키 {key} (표시·기록용)");
    if builder.is_recovery() {
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
    }
    let snapshot = device.scan(log)?;
    let boot_name = format!("boot{}", prepared.slot.suffix());
    let boot = if builder.is_recovery() {
        None
    } else {
        Some(storage::find_partition(&snapshot, &boot_name, 4)?.clone())
    };
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
    let mut originals = vec![boot, Some(devinfo), Some(persist), proinfo.clone()];
    if builder.is_recovery() {
        originals.push(Some(
            storage::find_partition(&snapshot, &format!("recovery{}", prepared.slot.suffix()), 4)?
                .clone(),
        ));
        originals.push(Some(storage::find_partition(&snapshot, "misc", 0)?.clone()));
        originals.push(Some(
            storage::find_partition(
                &snapshot,
                &format!("vendor_boot{}", prepared.slot.suffix()),
                4,
            )?
            .clone(),
        ));
    }
    for part in originals.into_iter().flatten() {
        let filename = format!("{}.img", part.name);
        dump(device, &part, &builder.partition_dir().join(&filename), log)?;
        builder.record(&filename, Some(part))?;
    }
    if let Some(raw) = &prepared.identity {
        builder.save_identity(raw)?;
        builder.record(backup::IDENTITY_NAME, None)?;
    }
    let backup = builder.finish(
        prepared,
        firmware,
        fingerprint,
        key,
        snapshot,
        proinfo.is_none(),
    )?;
    live!(
        log,
        "S3: 백업 파일을 다시 읽어 크기와 SHA-256을 확인했습니다: {}",
        backup.path().display()
    );
    Ok(backup)
}

fn verify_written<D: StageIo>(
    device: &mut D,
    part: &Partition,
    image: &Path,
    readback: &Path,
    log: &mut Vec<String>,
) -> Result<(), String> {
    let (len, expected) = backup::hash_file(image, None)?;
    let prefix = part.prefix(len)?;
    dump(device, &prefix, readback, log)?;
    if part.name == "gpt_main4" {
        if std::fs::read(image).map_err(|error| error.to_string())?
            != std::fs::read(readback).map_err(|error| error.to_string())?
        {
            return Err("쓰기 후 GPT 바이트 비교 실패".into());
        }
        return Ok(());
    }
    if backup::hash_file(readback, Some(len))?.1 != expected {
        return Err(format!("쓰기 후 검증 실패: {}", part.name));
    }
    Ok(())
}

fn recover_sources<D: StageIo>(
    device: &mut D,
    backup: &VerifiedBackup,
    sources: &RecoverySources,
    geometry: &Partition,
    boot: Option<&Partition>,
    success_reset: ResetMode,
    log: &mut Vec<String>,
) -> (Vec<String>, Vec<String>) {
    let mut errors = vec![];
    let mut warnings = vec![];
    if let Err(error) = backup.mark(TransactionStage::RecoveryRequired) {
        errors.push(format!("복구 시작 기록 실패: {error}"));
    }
    let mut originals = vec![];
    if let Some(boot) = boot {
        originals.push((boot, sources.boot()));
    }
    originals.push((geometry, sources.gpt()));
    // Attempt GPT recovery even if restoring boot fails.
    for (part, image) in originals {
        let result = device.write_at(part, &image, log).and_then(|_| {
            verify_written(
                device,
                part,
                &image,
                &sources
                    .work
                    .path()
                    .join(format!("restore_{}.bin", part.name)),
                log,
            )
        });
        if let Err(error) = result {
            errors.push(format!("{} 복구 실패: {error}", part.name));
        }
    }
    // Record verified originals before the system handoff. An endpoint error
    // during reset must not imply that boot/GPT need to be flashed again.
    if errors.is_empty()
        && let Err(error) = backup.mark(TransactionStage::Restored)
    {
        errors.push(format!("복구 완료 기록 실패: {error}"));
    }
    let mode = if errors.is_empty() {
        success_reset
    } else {
        ResetMode::Edl
    };
    if mode == ResetMode::System {
        live!(
            log,
            "원본 boot/GPT 복구·검증 완료. 시스템 부팅을 요청합니다."
        );
    }
    if let Err(error) = device.reset(mode, log) {
        if mode == ResetMode::System {
            warnings.push(format!("원본 boot/GPT 복구·검증은 완료했지만 시스템 부팅 요청 결과를 확인하지 못했습니다: {error}. 기기의 실제 부팅 상태를 확인하세요."));
        } else {
            errors.push(format!("EDL 복귀 실패: {error}"));
            // Preserve the existing recovery-required behavior for EDL cleanup.
            if let Err(error) = backup.mark(TransactionStage::RecoveryRequired) {
                errors.push(format!("EDL 복귀 실패 기록 오류: {error}"));
            }
        }
    } else if mode == ResetMode::System {
        live!(
            log,
            "시스템 부팅 요청을 전송했습니다. 기기의 실제 부팅 상태를 확인하세요."
        );
    }
    (errors, warnings)
}

/// Manual recovery opens a new session: verify all original files, the live
/// TB520FU model, matching disk GUID and boot coordinates BEFORE writing.
/// Request system boot only after both originals and the completion journal
/// verify; incomplete recovery remains in EDL.
pub fn restore<D: StageIo>(
    device: &mut D,
    backup: &VerifiedBackup,
    log: &mut Vec<String>,
) -> Result<StageReport, StageError> {
    if backup.manifest().version >= 3 {
        return Err(StageError::before_write(
            "DeviceInfo 백업은 새 Recovery 전용 복구 흐름으로 처리하세요.".into(),
            backup.path(),
        ));
    }
    let before = |cause| StageError::before_write(cause, backup.path());
    let sources = backup.freeze().map_err(before)?;
    device.ensure_single_target().map_err(before)?;
    identify(device, backup.manifest().slot, log).map_err(before)?;
    let original = std::fs::read(sources.gpt()).map_err(|error| before(error.to_string()))?;
    let geometry = storage::primary_geometry(&original).map_err(before)?;
    let boot = storage::find_partition(
        &backup.manifest().snapshot,
        &format!("boot{}", backup.manifest().slot.suffix()),
        4,
    )
    .map_err(before)?
    .clone();
    let header = Partition {
        num_sectors: 2,
        name: "restore_header4".into(),
        ..geometry.clone()
    };
    let path = sources.work.path().join("live_header.bin");
    dump(device, &header, &path, log).map_err(before)?;
    let live = std::fs::read(&path).map_err(|error| before(error.to_string()))?;
    if storage::disk_guid(&live).map_err(before)?
        != storage::disk_guid(&original).map_err(before)?
    {
        return Err(before(
            "백업과 기기의 GPT disk GUID가 다릅니다. 이 기기의 원본 백업을 선택하세요.".into(),
        ));
    }
    let live_gpt_path = sources.work.path().join("live_gpt.bin");
    dump(device, &geometry, &live_gpt_path, log).map_err(before)?;
    let live_gpt = std::fs::read(&live_gpt_path).map_err(|error| before(error.to_string()))?;
    // A failed GPT write can destroy its CRC or entries. Requiring a working
    // GPT lookup here would prevent recovery. Model + disk GUID establish
    // identity; a valid current table must also agree with boot geometry.
    if storage::disk_guid(&live_gpt).map_err(before)?
        != storage::disk_guid(&original).map_err(before)?
    {
        return Err(before(
            "복구 확인 도중 GPT disk GUID가 변경되었습니다.".into(),
        ));
    }
    match storage::validate_gpt(&live_gpt) {
        Ok(current) => {
            storage::match_gpt_partition(&current, &boot).map_err(before)?;
            device
                .validate_snapshot(std::slice::from_ref(&boot))
                .map_err(before)?;
        }
        Err(error) => live!(
            log,
            "현재 GPT를 읽을 수 없습니다: {error}. 기기 GUID를 확인했으므로 검증한 원본 좌표로 복구합니다."
        ),
    }
    let (details, warnings) = recover_sources(
        device,
        backup,
        &sources,
        &geometry,
        Some(&boot),
        ResetMode::System,
        log,
    );
    if !details.is_empty() {
        return Err(StageError { cause: "원본 복구를 완료하지 못했습니다. 백업과 로그를 보존하고 기기를 EDL로 연결해 다시 복구하세요.".into(), backup_dir: backup.path().to_path_buf(), recovery: RecoveryOutcome::Failed, details });
    }
    Ok(StageReport {
        backup_dir: backup.path().to_path_buf(),
        warnings,
    })
}
