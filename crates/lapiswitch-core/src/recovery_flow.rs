//! TWRP/DeviceInfo conversion and verified original restoration.
use crate::{
    backup::{self, TransactionStage, VerifiedBackup},
    deviceinfo::{self, map},
    firmware::FirmwareReport,
    region::Region,
    rollback::{self, Downgrade, RollbackIndices},
    stage1::StageIo,
    state::{FirmwareBinding, FlashOptions, Request, ResumeAction, RunState, Stage, StateStore},
    storage::{self, Partition},
    workflow::{self, FinishIo},
};
use ltbox_core::live;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
pub mod native;
pub mod restore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    Preflight,
    BackedUp,
    Staging,
    Staged,
    RecoveryRequested,
    RecoveryWaiting,
    Planned,
    WritePending,
    RegionVerified,
    EdlRequested,
    EdlWaiting,
    Restoring,
    OriginalsRestored,
    FirmwareWriting,
    FinalReboot,
    Done,
    Restored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub directory: String,
    pub before_sha256: String,
    pub planned_sha256: String,
    /// PRC/ROW of the original, when its region field holds one of them.
    pub original_region: Option<Region>,
    /// The chosen field values; with the original they rebuild the plan.
    pub edits: map::Edits,
    /// The user unlocked read-only fields for these edits in the editor.
    #[serde(default)]
    pub unlocked: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub phase: Phase,
    pub attempt: Option<Attempt>,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            phase: Phase::Preflight,
            attempt: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Twrp,
    Edl,
}

pub const CONNECTION_WAIT_SECONDS: u64 = 180;

/// Shared by TWRP and EDL waits, including the check immediately before
/// handing a newly connected session to any partition/DeviceInfo operation.
pub fn check_connection_timeout(transition: Transition, elapsed: u64) -> Result<(), String> {
    if elapsed < CONNECTION_WAIT_SECONDS {
        return Ok(());
    }
    let connection = match transition {
        Transition::Twrp => "ADB(TWRP)",
        Transition::Edl => "EDL",
    };
    Err(crate::msg!(
        "core_connection_timeout",
        connection = connection
    ))
}
pub trait Ui {
    /// Presentation only: does not change the durable write/resume checkpoints.
    fn phase(&mut self, _phase: Phase) {}
    fn confirm(&mut self, transition: Transition) -> Result<(), String>;
    /// Keep mode found firmware rollback indices below those stored in
    /// DeviceInfo. `Ok(true)` resets all 32 indices and continues; `Ok(false)`
    /// cancels the conversion and restores the originals. An error (the prompt
    /// was closed) stops without any write and keeps the run for resume/restore.
    fn confirm_rollback_reset(&mut self, downgrades: &[Downgrade]) -> Result<bool, String>;
    /// DeviceInfo edit: the live buffer is shown for editing. `Ok(None)`
    /// cancels and restores the originals; an error stops without a write.
    fn edit_deviceinfo(
        &mut self,
        layout: map::Layout,
        before: &[u8],
    ) -> Result<Option<EditRequest>, String>;
    fn status(&mut self, text: &str, stoppable: bool);
    /// A DeviceInfo dump was saved in this folder.
    fn dumped(&mut self, _folder: &Path) {}
    fn cancelled(&self) -> bool;
    fn finish_wait(&mut self) -> bool;
}
/// Field values chosen in the editor. A changed region needs the folder of
/// that region's official firmware, which is then checked and installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRequest {
    pub edits: map::Edits,
    pub firmware_dir: Option<PathBuf>,
    /// Applied after installing that firmware; `None` keeps the country code.
    /// Only a region change installs firmware, so only it may set one.
    pub country: Option<String>,
    /// Read-only fields were unlocked, after the risk was agreed to, and
    /// changed.
    pub unlocked: bool,
}
pub trait DeviceInfoIo {
    fn read(&mut self, out: &Path, log: &mut Vec<String>) -> Result<Vec<u8>, String>;
    /// Transfer and dry-run this exact plan without any write request. A
    /// failure here happens before the durable write intent is recorded.
    /// `unlocked` plans may change read-only fields.
    fn prepare(
        &mut self,
        before: &[u8],
        planned: &[u8],
        unlocked: bool,
        log: &mut Vec<String>,
    ) -> Result<(), String>;
    /// The only write request, for the plan just prepared in this session.
    /// Never retry this operation; a transport error leaves a pending intent.
    fn apply(
        &mut self,
        before: &[u8],
        planned: &[u8],
        out: &Path,
        log: &mut Vec<String>,
    ) -> Result<(Vec<u8>, Vec<u8>), String>;
}
pub fn resume_action(state: &RunState, transaction: Option<TransactionStage>) -> ResumeAction {
    let phase = state.recovery.phase;
    if transaction == Some(TransactionStage::Restored)
        || matches!(phase, Phase::Done | Phase::Restored)
    {
        return ResumeAction::Restart;
    }
    if matches!(
        transaction,
        Some(
            TransactionStage::RecoveryRequired
                | TransactionStage::GptWriting
                | TransactionStage::BootWriting
        )
    ) || phase == Phase::Staging
    {
        return ResumeAction::Recovery;
    }
    if !matches!(phase, Phase::Preflight | Phase::BackedUp)
        && transaction != Some(TransactionStage::S4Done)
    {
        return ResumeAction::Recovery;
    }
    match phase {
        Phase::Preflight | Phase::BackedUp => ResumeAction::Restart,
        Phase::FinalReboot => ResumeAction::Finalize,
        Phase::OriginalsRestored | Phase::FirmwareWriting | Phase::Restoring => ResumeAction::Flash,
        _ => ResumeAction::Wait,
    }
}
pub fn checkpoint(state: &mut RunState, store: &StateStore, phase: Phase) -> Result<(), String> {
    state.recovery.phase = phase;
    state.stage = match phase {
        Phase::Preflight => Stage::S1,
        Phase::BackedUp => Stage::S3,
        Phase::Staging => Stage::S4,
        Phase::Staged => Stage::S4Done,
        Phase::OriginalsRestored | Phase::FirmwareWriting => Stage::S6,
        Phase::FinalReboot => Stage::FinalReboot,
        Phase::Done | Phase::Restored => Stage::Done,
        _ => Stage::S5,
    };
    store.save(state)?;
    // Retain the irreversible firmware boundary even without the active state.
    backup::write_json_atomic(
        &state.backup_dir.join("recovery_progress.json"),
        &state.recovery,
    )
}
/// Result of a DeviceInfo transaction that did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The target region/rollback plan is written and independently re-read.
    Verified,
    /// The user cancelled at the rollback prompt; DeviceInfo was not written.
    Cancelled,
}
pub fn rollback_lower_message() -> String {
    crate::msg!("core_rollback_lower")
}
pub fn rollback_cancelled_message() -> String {
    crate::msg!("core_rollback_cancelled")
}
pub fn not_applied_message() -> String {
    crate::msg!("core_write_not_applied")
}

/// Only two fresh reads equal to the PC original prove that a failed write
/// request left DeviceInfo unchanged. Read errors or any other bytes do not.
fn unapplied(
    device: &mut impl DeviceInfoIo,
    original: &[u8],
    dir: &Path,
    log: &mut Vec<String>,
) -> bool {
    device
        .read(&dir.join("unapplied_1"), log)
        .is_ok_and(|first| first == original)
        && device
            .read(&dir.join("unapplied_2"), log)
            .is_ok_and(|second| second == original)
}

fn stopped(ui: &impl Ui) -> Result<(), String> {
    if ui.cancelled() {
        Err(crate::msg!("core_wait_stopped_resume"))
    } else {
        Ok(())
    }
}

fn data(
    device: &mut impl StageIo,
    part: &Partition,
    path: &Path,
    log: &mut Vec<String>,
) -> Result<Vec<u8>, String> {
    device.dump_at(part, path, log)?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != part.size_bytes()? {
        return Err(crate::msg!("core_dump_length_mismatch", part = part.name));
    }
    Ok(bytes)
}
fn write_verified(
    device: &mut impl StageIo,
    part: &Partition,
    image: &Path,
    out: &Path,
    log: &mut Vec<String>,
) -> Result<(), String> {
    device.ensure_single_target()?;
    let (size, hash) = backup::hash_file(image, None)?;
    let prefix = part.prefix(size)?;
    device.write_at(part, image, log)?;
    device.dump_at(&prefix, out, log)?;
    if backup::hash_file(out, None)? != (prefix.size_bytes()?, hash.clone()) {
        // Only a sector-padded image prefix may differ in the last sector.
        if std::fs::metadata(out).map_err(|e| e.to_string())?.len() != prefix.size_bytes()?
            || backup::hash_file(out, Some(size))?.1 != hash
        {
            return Err(crate::msg!("core_write_sha_failed", part = part.name));
        }
    }
    Ok(())
}
fn part(backup: &VerifiedBackup, name: &str, lun: u8) -> Result<Partition, String> {
    Ok(storage::find_partition(&backup.manifest().snapshot, name, lun)?.clone())
}
/// The fixed DeviceInfo evidence folder inside the backup's Partition folder.
pub const ATTEMPT_DIR: &str = "DeviceInfo";
pub fn recovery_name(slot: crate::slot::Slot) -> String {
    format!("recovery{}", slot.suffix())
}

/// Reconstruct the exact durable plan from the original and the recorded
/// target/mode. Its hash must match the intent saved before the only write.
fn saved_plan(
    layout: map::Layout,
    dir: &Path,
    record: &Attempt,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let original = dir.join("before_shared_1.bin");
    if backup::hash_file(&original, None)?
        != (deviceinfo::SHARED_SIZE as u64, record.before_sha256.clone())
    {
        return Err(crate::msg!("core_deviceinfo_backup_changed"));
    }
    let before = std::fs::read(original).map_err(|e| e.to_string())?;
    let planned = map::apply(layout, &before, &record.edits, record.unlocked)?;
    if backup::digest_bytes(&planned) != record.planned_sha256 {
        return Err(crate::msg!("core_deviceinfo_plan_differs"));
    }
    Ok((before, planned))
}

/// Both retained files and the durable plan hash are required before firmware.
fn verify_evidence(backup: &VerifiedBackup, state: &RunState) -> Result<(), String> {
    let record = state
        .recovery
        .attempt
        .as_ref()
        .ok_or(crate::msg!("core_deviceinfo_no_evidence"))?;
    let dir = attempt_dir(backup, &record.directory)?;
    let (before, planned) = saved_plan(backup.device().field_map()?, &dir, record)?;
    let final_read = dir.join("after_independent.bin");
    if backup::hash_file(&final_read, None)?
        != (
            deviceinfo::SHARED_SIZE as u64,
            record.planned_sha256.clone(),
        )
    {
        return Err(crate::msg!("core_deviceinfo_final_evidence_changed"));
    }
    let after = std::fs::read(final_read).map_err(|e| e.to_string())?;
    map::check_plan(
        backup.device().field_map()?,
        &before,
        &after,
        record.unlocked,
    )?;
    deviceinfo::verify(&planned, &after, &after)
}

fn original_bytes(backup: &VerifiedBackup, name: &str) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(backup.file_path(name)?).map_err(|e| e.to_string())?;
    let record = backup.entry(name)?;
    if bytes.len() as u64 != record.size_bytes || backup::digest_bytes(&bytes) != record.sha256 {
        return Err(crate::msg!("core_original_backup_changed", name = name));
    }
    Ok(bytes)
}

fn validate_staged(
    device: &mut impl StageIo,
    backup: &VerifiedBackup,
    log: &mut Vec<String>,
) -> Result<(), String> {
    backup.verify()?;
    device.ensure_single_target()?;
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let original = original_bytes(backup, "gpt_main4.bin")?;
    let geometry = storage::primary_geometry(&original)?;
    let expected = storage::validate_gpt(&original)?
        .remove_region_vbmeta(backup.manifest().slot)
        .map_err(|e| e.to_string())?;
    if data(device, &geometry, &work.path().join("gpt.bin"), log)? != expected {
        return Err(crate::msg!("core_staged_gpt_differs"));
    }
    let rec = part(backup, &recovery_name(backup.manifest().slot), 4)?
        .prefix(crate::recovery_asset::size(backup.device()))?;
    data(device, &rec, &work.path().join("recovery.img"), log)?;
    crate::recovery_asset::verify(backup.device(), &work.path().join("recovery.img"))?;
    let misc = part(backup, "misc", 0)?;
    let expected = deviceinfo::recovery_misc(&original_bytes(backup, "misc.img")?)?;
    if data(device, &misc, &work.path().join("misc.img"), log)? != expected {
        return Err(crate::msg!("core_staged_misc_differs"));
    }
    let persist = part(backup, "persist", 0)?;
    data(device, &persist, &work.path().join("persist.img"), log)?;
    if backup::hash_file(&work.path().join("persist.img"), None)?.1
        != backup.entry("persist.img")?.sha256
    {
        return Err(crate::msg!("core_staged_persist_differs"));
    }
    Ok(())
}

/// Caller already bound the backup/state and the active slot confirmed by
/// ADB and the EDL GPT. An EDL-only AVB estimate is never used for a Recovery write.
pub fn stage(
    device: &mut impl StageIo,
    backup: &VerifiedBackup,
    twrp: &Path,
    state: &mut RunState,
    store: &StateStore,
    log: &mut Vec<String>,
) -> Result<(), String> {
    backup.verify()?;
    crate::recovery_asset::verify(backup.device(), twrp)?;
    if state.recovery.phase != Phase::BackedUp
        || backup.transaction_stage()? != TransactionStage::BackupVerified
    {
        return Err(crate::msg!("core_stage_not_restartable"));
    }
    device.ensure_single_target()?;
    device.validate_snapshot(&backup.manifest().snapshot)?;
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let original = original_bytes(backup, "gpt_main4.bin")?;
    let gpt = storage::validate_gpt(&original)?;
    let geometry = storage::primary_geometry(&original)?;
    let rec = part(backup, &recovery_name(backup.manifest().slot), 4)?;
    rec.prefix(crate::recovery_asset::size(backup.device()))?;
    let misc = part(backup, "misc", 0)?;
    let original_misc = original_bytes(backup, "misc.img")?;
    let modified_misc = deviceinfo::recovery_misc(&original_misc)?;
    let gpt_path = work.path().join("gpt_modified.bin");
    backup::write_new(
        &gpt_path,
        &gpt.remove_region_vbmeta(backup.manifest().slot)
            .map_err(|e| e.to_string())?,
    )?;
    let misc_path = work.path().join("misc_recovery.img");
    backup::write_new(&misc_path, &modified_misc)?;
    // Preserve generated plans separately from immutable originals.
    for (source, name) in [
        (&gpt_path, "gpt_main4_modify.bin"),
        (&misc_path, "misc_boot_recovery.img"),
    ] {
        let artifact = backup.work_dir()?.join(name);
        let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
        if artifact.exists() {
            if std::fs::read(&artifact).map_err(|e| e.to_string())? != bytes {
                return Err(crate::msg!("core_stage_previous_image_differs"));
            }
        } else {
            backup::write_new(&artifact, &bytes)?;
        }
    }
    let twrp_copy = work.path().join("twrp.img");
    std::fs::copy(twrp, &twrp_copy).map_err(|e| e.to_string())?;
    crate::recovery_asset::verify(backup.device(), &twrp_copy)?;
    // Re-read all originals immediately before the first write.
    for (p, name) in [
        (&geometry, "gpt_main4.bin"),
        (&rec, &format!("{}.img", rec.name)),
        (&misc, "misc.img"),
    ] {
        let current = work.path().join(format!("before_{}", name));
        data(device, p, &current, log)?;
        if backup::hash_file(&current, None)?.1 != backup.entry(name)?.sha256 {
            return Err(crate::msg!("core_changed_since_backup", p = p.name));
        }
    }
    checkpoint(state, store, Phase::Staging)?;
    backup.mark(TransactionStage::GptWriting)?;
    let result = (|| {
        live!(
            log,
            "{}",
            crate::msg!("core_stage_writing", slot = backup.manifest().slot.suffix())
        );
        write_verified(
            device,
            &geometry,
            &gpt_path,
            &work.path().join("gpt_check.bin"),
            log,
        )?;
        backup.mark(TransactionStage::BootWriting)?;
        write_verified(
            device,
            &rec,
            &twrp_copy,
            &work.path().join("recovery_check.img"),
            log,
        )?;
        write_verified(
            device,
            &misc,
            &misc_path,
            &work.path().join("misc_check.img"),
            log,
        )?;
        backup.mark(TransactionStage::S4Done)?;
        checkpoint(state, store, Phase::Staged)
    })();
    if let Err(error) = result {
        live!(
            log,
            "{}",
            crate::msg!("core_stage_error_rollback", error = error)
        );
        match restore_originals(device, backup, log) {
            Ok(()) => {
                backup.mark(TransactionStage::Restored)?;
                checkpoint(state, store, Phase::Restored)?;
            }
            Err(restore) => {
                let _ = backup.mark(TransactionStage::RecoveryRequired);
                return Err(crate::msg!(
                    "core_stage_restore_failed",
                    error = error,
                    restore = restore
                ));
            }
        }
        return Err(error);
    }
    Ok(())
}

/// Verify immutable backup and identity before using frozen coordinates. No misc
/// restore and no old DeviceInfo replay, including on error paths.
pub fn restore_originals(
    device: &mut impl StageIo,
    backup: &VerifiedBackup,
    log: &mut Vec<String>,
) -> Result<(), String> {
    backup.verify()?;
    let journal = backup.path().join("recovery_progress.json");
    if journal.exists() {
        let progress: Progress = backup::read_json(&journal)?;
        if matches!(
            progress.phase,
            Phase::FirmwareWriting | Phase::FinalReboot | Phase::Done
        ) {
            return Err(crate::msg!("core_restore_blocked_firmware"));
        }
    } else if backup.transaction_stage()? != TransactionStage::BackupVerified {
        return Err(crate::msg!("core_restore_blocked_no_progress"));
    }
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let (geometry, rec) = validate_restore_device(device, backup, work.path(), log)?;
    let mut errors = vec![];
    let vb = part(
        backup,
        &format!("vendor_boot{}", backup.manifest().slot.suffix()),
        4,
    )?;
    let name = format!("{}.img", vb.name);
    let original = original_bytes(backup, &name)?;
    if data(
        device,
        &vb,
        &work.path().join("current_vendor_boot.img"),
        log,
    )? != original
    {
        let source = work.path().join(&name);
        backup::write_new(&source, &original)?;
        write_verified(
            device,
            &vb,
            &source,
            &work.path().join("vendor_boot_verified.img"),
            log,
        )?;
    }
    for (p, name) in [
        (&rec, format!("{}.img", rec.name)),
        (&geometry, "gpt_main4.bin".to_owned()),
    ] {
        let source = work.path().join(&name);
        std::fs::copy(backup.file_path(&name)?, &source).map_err(|e| e.to_string())?;
        if backup::hash_file(&source, None)?.1 != backup.entry(&name)?.sha256 {
            return Err(crate::msg!("core_original_copy_failed"));
        }
        if let Err(e) = write_verified(
            device,
            p,
            &source,
            &work.path().join(format!("check_{name}")),
            log,
        ) {
            errors.push(e);
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    live!(
        log,
        "{}",
        crate::msg!("core_originals_restored", rec = rec.name)
    );
    Ok(())
}

fn validate_restore_device(
    device: &mut impl StageIo,
    backup: &VerifiedBackup,
    work: &Path,
    log: &mut Vec<String>,
) -> Result<(Partition, Partition), String> {
    device.ensure_single_target()?;
    let original = original_bytes(backup, "gpt_main4.bin")?;
    let geometry = storage::primary_geometry(&original)?;
    let header = Partition {
        name: "identity_header4".into(),
        num_sectors: 2,
        ..geometry.clone()
    };
    let current = data(device, &header, &work.join("header.bin"), log)?;
    if storage::disk_guid(&current)? != storage::disk_guid(&original)? {
        return Err(crate::msg!("core_gpt_guid_differs"));
    }
    // TWRP mounts persist RW for qseecomd/secure storage; its pre-boot hash
    // is not an invariant. Bind restoration by the unique original GPT GUID
    // and the still-intact model AVB.
    let mut matched = false;
    for name in ["vbmeta_system_a", "vbmeta_system_b"] {
        if let Some(avb) = device.read_avb(name, log)? {
            matched |= avb
                .build_fingerprint()
                .is_some_and(|fp| crate::device::Device::identify(&fp) == Some(backup.device()));
        }
    }
    if !matched {
        return Err(crate::msg!(
            "core_restore_device_unconfirmed",
            device = backup.device()
        ));
    }
    let rec = part(backup, &recovery_name(backup.manifest().slot), 4)?;
    let live_gpt = data(device, &geometry, &work.join("current_gpt.bin"), log)?;
    match storage::validate_gpt(&live_gpt) {
        Ok(table) => {
            storage::match_gpt_partition(&table, &rec)?;
            let targets = vec![
                rec.clone(),
                part(backup, "misc", 0)?,
                part(
                    backup,
                    &format!("vendor_boot{}", backup.manifest().slot.suffix()),
                    4,
                )?,
            ];
            for p in targets.iter().filter(|p| p.lun == 4) {
                storage::match_gpt_partition(&table, p)?;
            }
            device.validate_snapshot(&targets)?;
        }
        Err(error) => live!(
            log,
            "{}",
            crate::msg!("core_live_gpt_invalid", error = error)
        ),
    }
    Ok((geometry, rec))
}

fn attempt_dir(backup: &VerifiedBackup, name: &str) -> Result<PathBuf, String> {
    if name != ATTEMPT_DIR {
        return Err(crate::msg!("core_attempt_path_invalid"));
    }
    let path = backup.work_dir()?.join(name);
    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(crate::msg!("core_attempt_path_link"));
        }
    }
    if !meta.file_type().is_dir() {
        return Err(crate::msg!("core_attempt_folder_invalid"));
    }
    Ok(path)
}
/// Edits of this run: the conversion preset, or the user's choice in the
/// editor, with whether read-only fields were unlocked for them. An edit that
/// changes the region binds that region's firmware.
fn choose_edits(
    before: &[u8],
    state: &mut RunState,
    firmware: &mut Option<FirmwareReport>,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Option<(map::Edits, bool)>, String> {
    let layout = state.device()?.write_layout()?;
    match state.request {
        Request::Convert {
            target,
            rollback_mode,
        } => {
            if firmware.as_ref().map(|firmware| firmware.region) != Some(target) {
                return Err(crate::msg!("core_no_target_firmware"));
            }
            Ok(Some((
                deviceinfo::conversion_edits(layout, before, target, rollback_mode)?,
                false,
            )))
        }
        Request::Dump => Err(crate::msg!("core_dump_changes_nothing")),
        Request::Bootloader { unlock } => {
            let value = if unlock { "1" } else { "0" };
            let edits = deviceinfo::BOOTLOADER_FLAGS
                .iter()
                .map(|key| ((*key).to_owned(), value.to_owned()))
                .collect();
            let planned = map::apply(layout, before, &edits, false)?;
            // A device already in the chosen state keeps its DeviceInfo.
            let note = match (planned == before, unlock) {
                (true, true) => crate::msg!("core_bootloader_already_unlocked"),
                (true, false) => crate::msg!("core_bootloader_already_locked"),
                (false, _) => crate::msg!("core_bootloader_reset_user_data"),
            };
            live!(log, "{note}");
            state.warnings.push(note);
            Ok(Some((map::edits_between(layout, before, &planned), false)))
        }
        Request::Edit => {
            ui.status(&crate::msg!("core_edit_read_done"), false);
            let Some(request) = ui.edit_deviceinfo(layout, before)? else {
                live!(log, "{}", crate::msg!("core_edit_cancelled"));
                return Ok(None);
            };
            let planned = map::apply(layout, before, &request.edits, request.unlocked)?;
            let target = layout
                .region(&planned)
                .ok_or(crate::msg!("core_region_field_invalid"))?;
            if layout.region(before) == Some(target) {
                state.firmware = None;
                state.options = FlashOptions::new(None)?;
                *firmware = None;
            } else {
                let dir = request.firmware_dir.ok_or_else(|| {
                    crate::msg!("core_region_change_needs_firmware", target = target.token())
                })?;
                let checked = crate::firmware::check_firmware(&dir, Some(target))
                    .map_err(|error| error.to_string())?;
                if checked.device != state.device()? {
                    return Err(crate::msg!(
                        "core_firmware_for_other_device",
                        device = checked.device,
                        device2 = state.device()?
                    ));
                }
                live!(
                    log,
                    "{}",
                    crate::msg!(
                        "core_region_change_log",
                        region = layout.region(before).map_or("?", Region::token),
                        target = target.token()
                    )
                );
                state.options = FlashOptions::new(request.country.as_deref())?;
                if let Some(code) = &state.options.country {
                    live!(
                        log,
                        "{}",
                        crate::msg!("core_country_after_flash", code = code)
                    );
                }
                state.firmware = Some(FirmwareBinding::new(&checked)?);
                *firmware = Some(checked);
            }
            Ok(Some((
                map::edits_between(layout, before, &planned),
                request.unlocked,
            )))
        }
    }
}

/// A firmware below the planned rollback indices would not boot. The stored
/// values decide; the user may reset all 32 or cancel and restore.
fn check_planned_rollback(
    layout: map::Layout,
    before: &[u8],
    (edits, unlocked): (map::Edits, bool),
    firmware: &FirmwareReport,
    state: &mut RunState,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Option<map::Edits>, String> {
    let planned = map::apply(layout, before, &edits, unlocked)?;
    let stored = deviceinfo::indices(layout, &planned);
    let device = RollbackIndices {
        boot: stored[3],
        vbmeta_system: stored[2],
    };
    for (partition, image, floor) in [
        ("boot", firmware.indices.boot, device.boot),
        (
            "vbmeta_system",
            firmware.indices.vbmeta_system,
            device.vbmeta_system,
        ),
    ] {
        live!(
            log,
            "{}",
            crate::msg!(
                "core_rollback_compare",
                partition = partition,
                image = image,
                floor = floor
            )
        );
    }
    let Err(downgrades) = rollback::check(firmware.indices, device) else {
        return Ok(Some(edits));
    };
    let details = rollback::describe(&downgrades);
    live!(log, "{} {details}", rollback_lower_message());
    if !ui.confirm_rollback_reset(&downgrades)? {
        live!(log, "{}", rollback_cancelled_message());
        return Ok(None);
    }
    state.warnings.push(crate::msg!(
        "core_rollback_reset_confirmed",
        rollback_lower_message = rollback_lower_message(),
        details = details
    ));
    live!(log, "{}", crate::msg!("core_rollback_reset_chosen"));
    Ok(Some(deviceinfo::with_rollback_reset(
        layout, before, &edits, unlocked,
    )?))
}

/// Field changes for the log. Long opaque values (keys, device caches) are
/// named but never printed.
fn log_changes(layout: map::Layout, before: &[u8], planned: &[u8], log: &mut Vec<String>) {
    let changes = map::diff(layout, before, planned);
    live!(
        log,
        "{}",
        crate::msg!("core_changed_fields", changes = changes.len())
    );
    for change in changes {
        let field = &change.field;
        if field.kind == map::Kind::Bytes && field.size > 8 {
            live!(
                log,
                "{}",
                crate::msg!(
                    "core_changed_field_opaque",
                    key = field.key,
                    offset = format!("{:04X}", field.offset),
                    size = field.size
                )
            );
        } else {
            live!(
                log,
                "  {} (0x{:04X}): {} → {}",
                field.key,
                field.offset,
                change.before,
                change.after
            );
        }
    }
}

pub fn transact(
    device: &mut impl DeviceInfoIo,
    backup: &VerifiedBackup,
    firmware: &mut Option<FirmwareReport>,
    state: &mut RunState,
    store: &StateStore,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Outcome, String> {
    backup.verify()?;
    restore::ensure_conversion_allowed(backup)?;
    state.check_binding(backup, firmware.as_ref())?;
    stopped(ui)?;
    let phase = state.recovery.phase;
    if matches!(phase, Phase::WritePending | Phase::RegionVerified) {
        let record = state
            .recovery
            .attempt
            .clone()
            .ok_or(crate::msg!("core_no_intent_record"))?;
        let dir = attempt_dir(backup, &record.directory)?;
        let (before, planned) = saved_plan(backup.device().field_map()?, &dir, &record)?;
        let verify = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
        let a = device.read(&verify.path().join("resume_1"), log)?;
        let b = device.read(&verify.path().join("resume_2"), log)?;
        // An unresolved request whose result is still the exact original was
        // not applied. Return to Planned and stop; never write in this call.
        if phase == Phase::WritePending && a == before && b == before {
            checkpoint(state, store, Phase::Planned)?;
            live!(log, "{}", not_applied_message());
            return Err(not_applied_message());
        }
        deviceinfo::verify(&planned, &a, &b)
            .map_err(|_| crate::msg!("core_previous_write_unknown").to_owned())?;
        let evidence = dir.join("after_independent.bin");
        if evidence.exists() {
            if std::fs::read(&evidence).map_err(|e| e.to_string())? != planned {
                return Err(crate::msg!("core_independent_evidence_changed"));
            }
        } else {
            backup::write_new(&evidence, &b)?;
        }
        checkpoint(state, store, Phase::RegionVerified)?;
        return Ok(Outcome::Verified);
    }
    ui.status(&crate::msg!("core_reading_original"), false);
    let attempt = backup.work_dir()?.join(ATTEMPT_DIR);
    match std::fs::create_dir(&attempt) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            attempt_dir(backup, ATTEMPT_DIR)?;
        }
        Err(error) => return Err(error.to_string()),
    }
    // Transport copies are private scratch files, removed on scope exit.
    // Preserve the very first read immediately, even if a later check fails.
    let scratch = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let before = device.read(&scratch.path().join("read1"), log)?;
    let layout = backup.device().write_layout()?;
    layout.check_buffer(&before)?;
    let original = attempt.join("before_shared_1.bin");
    if original.try_exists().map_err(|e| e.to_string())? {
        // A retry after a failed read must keep the very first original.
        if backup::hash_file(&original, None)?
            != (
                deviceinfo::SHARED_SIZE as u64,
                backup::digest_bytes(&before),
            )
        {
            return Err(crate::msg!("core_differs_from_first"));
        }
    } else {
        backup::write_new(&original, &before)?;
    }
    let second = device.read(&scratch.path().join("read2"), log)?;
    if before != second {
        return Err(crate::msg!("core_reads_differ_not_written"));
    }
    // A plan recorded before an unapplied write is reused as chosen.
    let (edits, unlocked) = if let Some(record) = state.recovery.attempt.clone() {
        saved_plan(layout, &attempt, &record)?;
        (record.edits, record.unlocked)
    } else {
        let Some((edits, unlocked)) = choose_edits(&before, state, firmware, ui, log)? else {
            return Ok(Outcome::Cancelled);
        };
        let edits = match firmware.as_ref() {
            Some(chosen) => match check_planned_rollback(
                layout,
                &before,
                (edits, unlocked),
                chosen,
                state,
                ui,
                log,
            )? {
                Some(edits) => edits,
                None => return Ok(Outcome::Cancelled),
            },
            None => edits,
        };
        stopped(ui)?;
        (edits, unlocked)
    };
    let planned = map::apply(layout, &before, &edits, unlocked)?;
    log_changes(layout, &before, &planned, log);
    state.recovery.attempt = Some(Attempt {
        directory: attempt
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or(crate::msg!("core_no_attempt_name"))?
            .into(),
        before_sha256: backup::digest_bytes(&before),
        planned_sha256: backup::digest_bytes(&planned),
        original_region: layout.region(&before),
        edits,
        unlocked,
    });
    if backup::hash_file(&attempt.join("before_shared_1.bin"), None)?
        != (before.len() as u64, backup::digest_bytes(&before))
    {
        return Err(crate::msg!("core_pc_backup_verify_failed"));
    }
    // The plan, and any firmware it requires, become durable here.
    checkpoint(state, store, Phase::Planned)?;
    stopped(ui)?;
    if before != planned {
        ui.status(&crate::msg!("core_writing_deviceinfo"), false);
        // Transfers and the dry-run cannot change DeviceInfo. Their failures
        // stay at Planned instead of becoming an unresolved write.
        device.prepare(&before, &planned, unlocked, log)?;
        checkpoint(state, store, Phase::WritePending)?;
        // A durable PC intent exists before the only write call.
        let (same, fresh) =
            match device.apply(&before, &planned, &scratch.path().join("apply"), log) {
                Ok(readback) => readback,
                Err(error) if unapplied(device, &before, scratch.path(), log) => {
                    checkpoint(state, store, Phase::Planned)
                        .map_err(|checkpoint| format!("{error}\n{checkpoint}"))?;
                    live!(log, "{}", not_applied_message());
                    return Err(format!("{error}\n{}", not_applied_message()));
                }
                Err(error) => return Err(error),
            };
        deviceinfo::verify(&planned, &same, &fresh)?;
    } else {
        live!(log, "{}", crate::msg!("core_write_skipped"));
    }
    // Independent helper process, freshly opened TA session, compared on PC too.
    let independent = device.read(&scratch.path().join("independent"), log)?;
    deviceinfo::verify(&planned, &independent, &independent)?;
    backup::write_new(&attempt.join("after_independent.bin"), &independent)?;
    checkpoint(state, store, Phase::RegionVerified)?;
    live!(log, "{}", crate::msg!("core_write_verified"));
    Ok(Outcome::Verified)
}

/// DeviceInfo dump: two independent reads must match. The 4096 bytes are
/// saved as `Deviceinfo_dump_<MODEL>_<unix time>/deviceinfo.bin` with an
/// `info.json` under the chosen folder. Nothing is written to the device and
/// no field map is needed.
pub fn dump(
    device: &mut impl DeviceInfoIo,
    backup: &VerifiedBackup,
    state: &RunState,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<PathBuf, String> {
    backup.verify()?;
    stopped(ui)?;
    let root = state
        .dump_root
        .as_deref()
        .ok_or(crate::msg!("core_dump_no_location"))?;
    ui.status(&crate::msg!("core_dump_reading"), false);
    let scratch = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let first = device.read(&scratch.path().join("dump1"), log)?;
    let second = device.read(&scratch.path().join("dump2"), log)?;
    if first.len() != deviceinfo::SHARED_SIZE {
        return Err(crate::msg!(
            "core_dump_wrong_size",
            first = first.len(),
            shared_size = deviceinfo::SHARED_SIZE
        ));
    }
    if first != second {
        return Err(crate::msg!("core_dump_reads_differ"));
    }
    let model = backup.device();
    let now = chrono::Utc::now();
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let folder = root.join(format!("Deviceinfo_dump_{model}_{}", now.timestamp()));
    std::fs::create_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let bin = folder.join("deviceinfo.bin");
    backup::write_new(&bin, &first)?;
    let info = serde_json::json!({
        "model": model.model(),
        "codename": model.codename(),
        "dumped_at": now.to_rfc3339(),
        "unix_time": now.timestamp(),
        "size": first.len(),
        "sha256": backup::digest_bytes(&first),
        "logical_size": model.layout().map(|layout| layout.logical_size()),
        "slot": state.slot,
    });
    backup::write_new(
        &folder.join("info.json"),
        &serde_json::to_vec_pretty(&info).map_err(|e| e.to_string())?,
    )?;
    if std::fs::read(&bin).map_err(|e| e.to_string())? != first {
        return Err(crate::msg!("core_dump_verify_failed"));
    }
    live!(
        log,
        "{}",
        crate::msg!("core_dump_saved", folder = folder.display())
    );
    ui.dumped(&folder);
    Ok(folder)
}

/// The user cancelled at the rollback prompt while TWRP is running and
/// DeviceInfo is unwritten. Reboot to EDL and run the verified original
/// restore (GPT, Recovery, vendor_boot), then archive the restored run.
#[allow(clippy::too_many_arguments)] // Explicit transport, recovery and clock seams for fault tests.
pub fn cancel_to_original(
    device: &mut impl FinishIo,
    recovery: &mut impl restore::Session,
    backup: &VerifiedBackup,
    loader: &Path,
    store: &StateStore,
    clock: &mut impl workflow::Clock,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    ui.status(&crate::msg!("core_cancel_to_original"), false);
    back_to_original(device, recovery, backup, loader, store, clock, ui, log)
}

/// TWRP is running and DeviceInfo is unwritten: reboot to EDL, restore the
/// verified originals and archive the restored run.
#[allow(clippy::too_many_arguments)]
fn back_to_original(
    device: &mut impl FinishIo,
    recovery: &mut impl restore::Session,
    backup: &VerifiedBackup,
    loader: &Path,
    store: &StateStore,
    clock: &mut impl workflow::Clock,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    recovery.verify_identity(backup)?;
    if let Err(e) = recovery.reboot_edl() {
        live!(
            log,
            "{}",
            crate::msg!("core_restore_edl_reboot_response", e = e)
        );
    }
    wait_for_edl(device, loader, clock, ui, log)?;
    if !ui.finish_wait() {
        return Err(crate::msg!("core_wait_stopped_restore"));
    }
    let mut warnings = restore::run(
        device,
        backup,
        loader,
        None,
        &mut restore::NoConnector,
        clock,
        ui,
        log,
    )?;
    match store.archive_restored(backup) {
        Ok(Some(path)) => live!(
            log,
            "{}",
            crate::msg!("core_restored_run_archived", path = path.display())
        ),
        Ok(None) => {}
        Err(error) => warnings.push(crate::msg!("core_restored_archive_failed", error = error)),
    }
    Ok(warnings)
}

/// The run ends as an error either way: the conversion did not happen.
pub fn cancelled(restore: Result<Vec<String>, String>) -> String {
    match restore {
        Ok(warnings) => {
            let mut text = crate::msg!(
                "core_cancel_restored",
                rollback_cancelled_message = rollback_cancelled_message()
            );
            for warning in warnings {
                text.push('\n');
                text.push_str(&warning);
            }
            text
        }
        Err(error) => crate::msg!(
            "core_cancel_restore_failed",
            rollback_cancelled_message = rollback_cancelled_message(),
            error = error
        ),
    }
}

/// Recovery STEP 7: keep watching through missing or unstable EDL ports.
/// Each transport attempt retains its own bounded wait; no partition writes
/// or DeviceInfo replay occur here. Fatal open errors still stop the workflow.
pub fn wait_for_edl(
    device: &mut impl FinishIo,
    loader: &Path,
    clock: &mut impl workflow::Clock,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<(), String> {
    ui.phase(Phase::EdlWaiting);
    ui.status(&crate::msg!("core_waiting_edl"), true);
    let started = clock.seconds();
    loop {
        stopped(ui)?;
        check_connection_timeout(Transition::Edl, clock.seconds().saturating_sub(started))?;
        device.ensure_single_target()?;
        if device.probe_edl()?.is_some() && device.try_open_edl(loader, log)? {
            stopped(ui)?;
            check_connection_timeout(Transition::Edl, clock.seconds().saturating_sub(started))?;
            return Ok(());
        }
        check_connection_timeout(Transition::Edl, clock.seconds().saturating_sub(started))?;
        clock.sleep_one_second();
    }
}

/// Firmware is unreachable until both DeviceInfo verification AND original
/// Recovery/GPT restoration have completed. Without bound firmware (an edit
/// that keeps the region) the run ends there with a normal boot and no wipe.
pub fn finish(
    device: &mut impl FinishIo,
    backup: &VerifiedBackup,
    mut firmware: Option<FirmwareReport>,
    state: &mut RunState,
    store: &StateStore,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<(), String> {
    backup.verify()?;
    if backup.transaction_stage()? != TransactionStage::S4Done {
        return Err(crate::msg!("core_no_stage_record"));
    }
    state.check_binding(backup, firmware.as_ref())?;
    let loader = state.loader_path().to_path_buf();
    let mut phase = state.recovery.phase;
    ui.phase(phase);
    if phase == Phase::FinalReboot {
        state
            .warnings
            .push(crate::msg!("core_stopped_after_reboot"));
        checkpoint(state, store, Phase::Done)?;
        return Ok(());
    }
    if matches!(
        phase,
        Phase::Staged
            | Phase::RecoveryRequested
            | Phase::RecoveryWaiting
            | Phase::Planned
            | Phase::WritePending
            | Phase::RegionVerified
    ) {
        // Each reboot prompt comes after its checks and the reboot follows
        // it at once; the GUI confirms it by itself after a few seconds.
        if phase == Phase::Staged {
            ui.phase(Phase::RecoveryRequested);
            ui.status(&crate::msg!("core_checking_staged_recovery"), false);
            validate_staged(device, backup, log)?;
            ui.confirm(Transition::Twrp)?;
            stopped(ui)?;
            checkpoint(state, store, Phase::RecoveryRequested)?;
            device.reboot_recovery(log)?;
            checkpoint(state, store, Phase::RecoveryWaiting)?;
        }
        ui.phase(Phase::RecoveryWaiting);
        let mut recovery = native::Recovery::wait(backup, ui, log)?;
        if state.request == Request::Dump {
            ui.phase(Phase::Planned);
            let folder = dump(&mut recovery, backup, state, ui, log)?;
            ui.status(&crate::msg!("core_dump_saved_restoring"), false);
            let warnings = back_to_original(
                device,
                &mut recovery,
                backup,
                &loader,
                store,
                &mut workflow::native::SystemClock::default(),
                ui,
                log,
            )
            .map_err(|error| {
                crate::msg!(
                    "core_dump_restore_failed",
                    folder = folder.display(),
                    error = error
                )
            })?;
            state.warnings.extend(warnings);
            live!(log, "{}", crate::msg!("core_dump_done"));
            return Ok(());
        }
        ui.phase(Phase::Planned);
        if transact(&mut recovery, backup, &mut firmware, state, store, ui, log)?
            == Outcome::Cancelled
        {
            return Err(cancelled(cancel_to_original(
                device,
                &mut recovery,
                backup,
                &loader,
                store,
                &mut workflow::native::SystemClock::default(),
                ui,
                log,
            )));
        }
        // The write was just verified by an independent read; the prompt
        // closes within seconds, so it is not read again after it.
        ui.phase(Phase::EdlRequested);
        recovery.verify_identity(backup)?;
        ui.confirm(Transition::Edl)?;
        stopped(ui)?;
        checkpoint(state, store, Phase::EdlRequested)?;
        if let Err(e) = recovery.reboot_edl() {
            live!(log, "{}", crate::msg!("core_edl_reboot_response", e = e));
        }
        drop(recovery);
        checkpoint(state, store, Phase::EdlWaiting)?;
        phase = Phase::EdlWaiting;
    }
    if matches!(
        phase,
        Phase::EdlRequested
            | Phase::EdlWaiting
            | Phase::Restoring
            | Phase::OriginalsRestored
            | Phase::FirmwareWriting
    ) {
        wait_for_edl(
            device,
            &loader,
            &mut workflow::native::SystemClock::default(),
            ui,
            log,
        )?;
        stopped(ui)?;
    } else {
        return Err(crate::msg!("core_phase_not_resumable"));
    }
    if !ui.finish_wait() {
        return Err(crate::msg!("core_stop_handled"));
    }
    verify_evidence(backup, state)?;
    ui.phase(Phase::Restoring);
    ui.status(&crate::msg!("core_restoring_originals"), false);
    if phase != Phase::FirmwareWriting {
        checkpoint(state, store, Phase::Restoring)?;
        restore_originals(device, backup, log)?;
        let identity = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
        let persist = part(backup, "persist", 0)?;
        let bytes = data(
            device,
            &persist,
            &identity.path().join("post_twrp_persist.img"),
            log,
        )?;
        state.persist_fingerprint = Some(crate::country::persist_fingerprint(&bytes));
        checkpoint(state, store, Phase::OriginalsRestored)?;
    }
    let Some(firmware) = firmware else {
        // DeviceInfo edit without a region change: nothing is installed.
        ui.phase(Phase::FinalReboot);
        ui.status(&crate::msg!("core_originals_no_firmware"), false);
        checkpoint(state, store, Phase::FinalReboot)?;
        if let Some(warning) = device.final_reboot(log) {
            state.warnings.push(warning);
        }
        checkpoint(state, store, Phase::Done)?;
        live!(log, "{}", crate::msg!("core_edit_done_no_firmware"));
        return Ok(());
    };
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    workflow::confirm_identity(device, backup, state, work.path(), log)?;
    backup.verify()?;
    restore::ensure_conversion_allowed(backup)?;
    state.verify_binding(backup, Some(&firmware))?;
    checkpoint(state, store, Phase::FirmwareWriting)?;
    ui.phase(Phase::FirmwareWriting);
    ui.status(&crate::msg!("core_installing_firmware"), false);
    device.flash_firmware(&firmware, true, log)?;
    ui.phase(Phase::FinalReboot);
    ui.status(&crate::msg!("core_finalizing"), false);
    workflow::restore_gpt_if_needed(device, backup, work.path(), log)?;
    if let Some(code) = &state.options.country {
        match backup.device().country() {
            lapiswitch_devices::Country::DevinfoPersist => state
                .warnings
                .extend(workflow::country_change(device, code, work.path(), log)),
            lapiswitch_devices::Country::Oemowninfo(field) => {
                workflow::change_partition_country(device, backup, &field, code, work.path(), log)?
            }
        }
    }
    device.set_active_a(log)?;
    checkpoint(state, store, Phase::FinalReboot)?;
    if let Some(warning) = device.final_reboot(log) {
        state.warnings.push(warning);
    }
    checkpoint(state, store, Phase::Done)?;
    live!(log, "{}", crate::msg!("core_flash_done"));
    Ok(())
}
