//! S3: verified original backups. Staging, DeviceInfo and original
//! restoration continue in `recovery_flow`.

use std::path::Path;

use ltbox_core::live;

use crate::avb_info::AvbSummary;
use crate::backup::{self, BackupBuilder, VerifiedBackup};
use crate::device::Device;
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
    expected: Device,
    slot: Slot,
    log: &mut Vec<String>,
) -> Result<(String, String), String> {
    let a = device.read_avb("vbmeta_system_a", log)?;
    let b = device.read_avb("vbmeta_system_b", log)?;
    let active = if slot == Slot::A { a } else { b };
    let active = active.ok_or(crate::msg!("core_stage1_vbmeta_unread"))?;
    active
        .build_fingerprint()
        .filter(|fp| Device::identify(fp) == Some(expected))
        .ok_or_else(|| crate::msg!("core_stage1_model_unconfirmed", expected = expected))?;
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
        return Err(crate::msg!("core_stage1_dump_length", part = part.name));
    }
    Ok(())
}

/// A device whose country code lives in a dedicated partition keeps that
/// partition's original too; its location and size come from device.toml.
fn country_partition(device: Device, snapshot: &[Partition]) -> Result<Option<Partition>, String> {
    let lapiswitch_devices::Country::Oemowninfo(field) = device.country() else {
        return Ok(None);
    };
    let part = storage::find_partition(snapshot, field.partition, field.lun)?;
    if part.size_bytes()? != field.size as u64 {
        return Err(crate::msg!(
            "core_stage1_partition_size",
            partition = field.partition,
            size = field.size
        ));
    }
    Ok(Some(part.clone()))
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
    live!(log, "{}", crate::msg!("core_s3_backup_start"));
    let (fingerprint, key) = identify(device, prepared.device, prepared.slot, log)?;
    live!(log, "{}", crate::msg!("core_s3_key", key = key));
    match crate::device_version::check(prepared.device, &fingerprint) {
        Ok(version) => live!(
            log,
            "{}",
            crate::msg!(
                "core_s3_version_ok",
                slot = prepared.slot.suffix(),
                version = version
            )
        ),
        Err(crate::device_version::VersionError::Unrecognized) => {
            live!(
                log,
                "{}",
                crate::msg!("core_s3_unrecognized", fingerprint = fingerprint)
            );
            if !confirm_unrecognized()? {
                return Err(crate::msg!("core_s3_unrecognized_cancelled"));
            }
            live!(log, "{}", crate::msg!("core_s3_unrecognized_continued"));
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
        return Err(crate::msg!("core_gpt_header_changed_dump"));
    }
    for part in snapshot.iter().filter(|part| part.lun == 4) {
        storage::match_gpt_partition(&gpt, part)?;
    }
    // Same rule as S1: set flags must agree; blank flags keep the ADB slot.
    if gpt.active_slot(Some(prepared.slot))? != prepared.slot {
        return Err(crate::msg!("core_gpt_slot_changed_backup"));
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
        country_partition(prepared.device, &snapshot)?,
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
        "{}",
        crate::msg!("core_s3_backup_verified", backup = backup.path().display())
    );
    Ok(backup)
}
