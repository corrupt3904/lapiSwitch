//! Original restoration is a separate, durable reverse transaction. Classify
//! conversion evidence on the PC before connecting to or writing any device.
use super::*;
use crate::{region::Region, vendor_boot};

pub fn unknown_message() -> String {
    crate::msg!("core_restore_unknown")
}
const JOURNAL: &str = "original_restore.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assessment {
    BeforeWrite,
    /// The verified final read; `changed` is any DeviceInfo byte difference.
    Verified {
        original: Option<Region>,
        current: Option<Region>,
        changed: bool,
    },
    Unknown(String),
}
impl Assessment {
    pub fn description(&self) -> String {
        let token = |region: &Option<Region>| region.map_or("?", Region::token);
        match self {
            Self::BeforeWrite => crate::msg!("core_assessment_before_write"),
            Self::Verified { changed: false, .. } => {
                crate::msg!("core_assessment_unchanged")
            }
            Self::Verified {
                original, current, ..
            } => crate::msg!(
                "core_assessment_changed",
                original = token(original),
                current = token(current)
            ),
            Self::Unknown(_) => crate::msg!("core_assessment_unknown"),
        }
    }
    /// The original DeviceInfo must be written back through TWRP.
    pub fn needs_twrp(&self) -> bool {
        matches!(self, Self::Verified { changed: true, .. })
    }
}
fn evidence_json<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, String> {
    if backup::hash_file(p, None)?.0 > 65536 {
        return Err(crate::msg!("core_record_size_error"));
    }
    backup::read_json(p)
}
fn progress(backup: &VerifiedBackup) -> Result<Option<Progress>, String> {
    let path = backup.path().join("recovery_progress.json");
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let p: Progress = evidence_json(&path)?;
    Ok(Some(p))
}
fn proof(backup: &VerifiedBackup, p: &Progress) -> Result<(Vec<u8>, Vec<u8>), String> {
    let a = p
        .attempt
        .as_ref()
        .ok_or(crate::msg!("core_write_intent_missing"))?;
    let dir = attempt_dir(backup, &a.directory)?;
    let before_path = dir.join("before_shared_1.bin");
    if backup::hash_file(&before_path, None)?
        != (deviceinfo::SHARED_SIZE as u64, a.before_sha256.clone())
    {
        return Err(crate::msg!("core_original_hash_mismatch"));
    }
    let before = std::fs::read(before_path).map_err(|e| e.to_string())?;
    if backup.device().field_map()?.region(&before) != a.original_region {
        return Err(crate::msg!("core_original_region_mismatch"));
    }
    let planned = map::apply(backup.device().field_map()?, &before, &a.edits)?;
    if backup::digest_bytes(&planned) != a.planned_sha256 {
        return Err(crate::msg!("core_planned_hash_mismatch"));
    }
    Ok((before, planned))
}
fn classify(backup: &VerifiedBackup, p: Option<&Progress>) -> Result<Assessment, String> {
    let Some(p) = p else {
        let files = std::fs::read_dir(backup.work_dir()?).map_err(|e| e.to_string())?;
        for file in files {
            let name = file.map_err(|e| e.to_string())?.file_name();
            let name = name.to_string_lossy();
            if name == ATTEMPT_DIR {
                return Err(crate::msg!("core_attempt_without_record"));
            }
        }
        if backup.transaction_stage()? == TransactionStage::BackupVerified {
            return Ok(Assessment::BeforeWrite);
        }
        return Err(crate::msg!("core_deviceinfo_record_missing"));
    };
    if matches!(
        p.phase,
        Phase::Preflight
            | Phase::BackedUp
            | Phase::Staging
            | Phase::Staged
            | Phase::RecoveryRequested
            | Phase::RecoveryWaiting
            | Phase::Planned
    ) {
        if p.phase == Phase::Planned && p.attempt.is_none() {
            return Err(crate::msg!("core_planned_original_missing"));
        }
        if let Some(a) = &p.attempt {
            proof(backup, p)?;
            if attempt_dir(backup, &a.directory)?
                .join("after_independent.bin")
                .try_exists()
                .map_err(|e| e.to_string())?
            {
                return Err(crate::msg!("core_prewrite_final_conflict"));
            }
        }
        for file in std::fs::read_dir(backup.work_dir()?).map_err(|e| e.to_string())? {
            let file = file.map_err(|e| e.to_string())?;
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name == ATTEMPT_DIR
                && file
                    .path()
                    .join("after_independent.bin")
                    .try_exists()
                    .map_err(|e| e.to_string())?
            {
                return Err(crate::msg!("core_prewrite_final_left"));
            }
        }
        return Ok(Assessment::BeforeWrite);
    }
    let (before, planned) = proof(backup, p)?;
    let a = p.attempt.as_ref().unwrap();
    let final_path = attempt_dir(backup, &a.directory)?.join("after_independent.bin");
    if backup::hash_file(&final_path, None)?
        != (deviceinfo::SHARED_SIZE as u64, a.planned_sha256.clone())
    {
        return Err(crate::msg!("core_final_evidence_bad"));
    }
    let after = std::fs::read(final_path).map_err(|e| e.to_string())?;
    deviceinfo::verify(&planned, &after, &after)?;
    Ok(Assessment::Verified {
        original: backup.device().field_map()?.region(&before),
        current: backup.device().field_map()?.region(&after),
        changed: before != after,
    })
}
/// File existence alone never means a write failed or succeeded.
pub fn assess(backup: &VerifiedBackup) -> Result<Assessment, String> {
    backup.verify()?;
    match progress(backup) {
        Err(e) => Ok(Assessment::Unknown(e)),
        Ok(p) => {
            if p.as_ref().is_some_and(|p| {
                matches!(
                    p.phase,
                    Phase::FirmwareWriting | Phase::FinalReboot | Phase::Done
                )
            }) {
                return Err(crate::msg!("core_backup_firmware_started"));
            }
            Ok(classify(backup, p.as_ref()).unwrap_or_else(Assessment::Unknown))
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum RestorePhase {
    Prepared,
    BootRequested,
    RegionWritePending,
    RegionVerified,
    Complete,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u8,
    manifest_sha256: String,
    progress_sha256: Option<String>,
    phase: RestorePhase,
    before_sha256: Option<String>,
    planned_sha256: Option<String>,
}
fn regular_directory(path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(crate::msg!("core_restore_folder_link"));
        }
    }
    if !meta.file_type().is_dir() {
        return Err(crate::msg!("core_restore_folder_error"));
    }
    Ok(())
}
fn directory(backup: &VerifiedBackup, create: bool) -> Result<PathBuf, String> {
    let p = backup.work_dir()?.join("OriginalRestore");
    if create {
        match std::fs::create_dir(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if p.try_exists().map_err(|e| e.to_string())? {
        regular_directory(&p)?;
    }
    Ok(p)
}
fn progress_hash(backup: &VerifiedBackup) -> Result<Option<String>, String> {
    let p = backup.path().join("recovery_progress.json");
    if p.try_exists().map_err(|e| e.to_string())? {
        Ok(Some(backup::hash_file(&p, None)?.1))
    } else {
        Ok(None)
    }
}
fn load_journal(backup: &VerifiedBackup) -> Result<Option<Journal>, String> {
    let path = directory(backup, false)?.join(JOURNAL);
    if !path.try_exists().map_err(|e| e.to_string())? {
        let dir = directory(backup, false)?;
        if dir.try_exists().map_err(|e| e.to_string())?
            && std::fs::read_dir(dir)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
        {
            return Err(crate::msg!(
                "core_restore_files_without_record",
                unknown_message = unknown_message()
            ));
        }
        return Ok(None);
    }
    let j: Journal = evidence_json(&path)?;
    if j.version != 1
        || j.manifest_sha256
            != backup::hash_file(&backup.path().join(backup::MANIFEST_NAME), None)?.1
        || j.progress_sha256 != progress_hash(backup)?
    {
        return Err(crate::msg!("core_restore_record_mismatch"));
    }
    Ok(Some(j))
}
/// The reverse plan is the complete original DeviceInfo read before the
/// conversion; the live state must still be exactly the verified result.
fn restore_proof(backup: &VerifiedBackup, j: &Journal) -> Result<bool, String> {
    let dir = directory(backup, false)?;
    if j.before_sha256.is_none() && j.planned_sha256.is_none() {
        return Ok(false);
    }
    let before_path = dir.join("deviceinfo_before.bin");
    let before_hash = j
        .before_sha256
        .as_ref()
        .ok_or(crate::msg!("core_restore_before_hash_missing"))?;
    if backup::hash_file(&before_path, None)? != (4096, before_hash.clone()) {
        return Err(crate::msg!("core_restore_before_altered"));
    }
    let before = std::fs::read(before_path).map_err(|e| e.to_string())?;
    let p = progress(backup)?.ok_or(crate::msg!("core_conversion_record_missing"))?;
    let (original, converted) = proof(backup, &p)?;
    if before != converted {
        return Err(crate::msg!("core_restore_before_differs"));
    }
    let planned = original;
    let expected = j
        .planned_sha256
        .as_ref()
        .ok_or(crate::msg!("core_restore_planned_hash_missing"))?;
    if backup::digest_bytes(&planned) != *expected {
        return Err(crate::msg!("core_restore_plan_mismatch"));
    }
    let after_path = dir.join("deviceinfo_after.bin");
    if !after_path.try_exists().map_err(|e| e.to_string())? {
        return Ok(false);
    }
    if backup::hash_file(&after_path, None)? != (4096, expected.clone()) {
        return Err(crate::msg!("core_restore_final_altered"));
    }
    let after = std::fs::read(after_path).map_err(|e| e.to_string())?;
    deviceinfo::verify(&planned, &after, &after)?;
    Ok(true)
}
/// Preflight for both backup selection and execution. No USB or DeviceInfo read.
pub fn preflight(backup: &VerifiedBackup) -> Result<Assessment, String> {
    let state = assess(backup)?;
    if let Assessment::Unknown(reason) = &state {
        return Err(format!("{}\n{reason}", unknown_message()));
    }
    let j = load_journal(backup)?;
    if let Assessment::Verified {
        original,
        changed: true,
        ..
    } = state
    {
        let vb = original_bytes(
            backup,
            &format!("vendor_boot{}.img", backup.manifest().slot.suffix()),
        )?;
        if original.is_some_and(|original| {
            vendor_boot::region(backup.device().region_marker(), &vb).ok() != Some(original)
        }) {
            return Err(crate::msg!("core_restore_vendor_boot_region"));
        }
        if let Some(j) = &j {
            let proven = restore_proof(backup, j)?;
            if (matches!(
                j.phase,
                RestorePhase::RegionWritePending
                    | RestorePhase::RegionVerified
                    | RestorePhase::Complete
            )) && !proven
            {
                return Err(crate::msg!(
                    "core_restore_write_unproven",
                    unknown_message = unknown_message()
                ));
            }
        }
    }
    Ok(state)
}
pub fn ensure_conversion_allowed(backup: &VerifiedBackup) -> Result<(), String> {
    if directory(backup, false)?
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Err(crate::msg!("core_restore_started"));
    }
    Ok(())
}
pub trait Session: DeviceInfoIo {
    fn verify_identity(&mut self, backup: &VerifiedBackup) -> Result<(), String>;
    fn reboot_edl(&mut self) -> Result<(), String>;
}
pub trait Connector {
    type Recovery: Session;
    fn connect(
        &mut self,
        backup: &VerifiedBackup,
        ui: &mut impl Ui,
        log: &mut Vec<String>,
    ) -> Result<Self::Recovery, String>;
}
pub struct NativeConnector;
impl Connector for NativeConnector {
    type Recovery = super::native::Recovery;
    fn connect(
        &mut self,
        b: &VerifiedBackup,
        u: &mut impl Ui,
        l: &mut Vec<String>,
    ) -> Result<Self::Recovery, String> {
        super::native::Recovery::wait(b, u, l)
    }
}
/// For a restore whose DeviceInfo was never written: no TWRP session is needed.
pub struct NoConnector;
impl Connector for NoConnector {
    type Recovery = super::native::Recovery;
    fn connect(
        &mut self,
        _: &VerifiedBackup,
        _: &mut impl Ui,
        _: &mut Vec<String>,
    ) -> Result<Self::Recovery, String> {
        Err(crate::msg!("core_restore_no_twrp_needed"))
    }
}
impl Session for super::native::Recovery {
    fn verify_identity(&mut self, b: &VerifiedBackup) -> Result<(), String> {
        self.verify_identity(b)
    }
    fn reboot_edl(&mut self) -> Result<(), String> {
        self.reboot_edl()
    }
}
fn save(dir: &Path, j: &mut Journal, phase: RestorePhase) -> Result<(), String> {
    j.phase = phase;
    backup::write_json_atomic(&dir.join(JOURNAL), j)
}
fn preserve(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.try_exists().map_err(|e| e.to_string())? {
        if backup::hash_file(path, None)? != (bytes.len() as u64, backup::digest_bytes(bytes)) {
            return Err(crate::msg!("core_restore_file_changed"));
        }
        Ok(())
    } else {
        backup::write_new(path, bytes)
    }
}
fn bootstrap(
    device: &mut impl FinishIo,
    backup: &VerifiedBackup,
    twrp: &Path,
    current: Region,
    log: &mut Vec<String>,
) -> Result<(), String> {
    crate::recovery_asset::verify(backup.device(), twrp)?;
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let (geometry, rec) = validate_restore_device(device, backup, work.path(), log)?;
    let vb = part(
        backup,
        &format!("vendor_boot{}", backup.manifest().slot.suffix()),
        4,
    )?;
    // Dump the live image before any bootstrap write. Only immutable originals
    // and their exact region-patched copy are accepted on reentry.
    let original = original_bytes(backup, &format!("{}.img", vb.name))?;
    let modified = vendor_boot::patch(backup.device().region_marker(), &original, current)?;
    let live = data(device, &vb, &work.path().join("live_vendor_boot.img"), log)?;
    if live != original && live != modified {
        return Err(crate::msg!("core_live_vendor_boot_differs"));
    }
    let gpt = storage::validate_gpt(&original_bytes(backup, "gpt_main4.bin")?)?
        .remove_region_vbmeta(backup.manifest().slot)
        .map_err(|e| e.to_string())?;
    let misc = part(backup, "misc", 0)?;
    let live_misc = data(device, &misc, &work.path().join("live_misc.img"), log)?;
    let patched_misc = deviceinfo::recovery_misc(&live_misc)?;
    let twrp_bytes = std::fs::read(twrp).map_err(|e| e.to_string())?;
    for (p, bytes, name) in [
        (&geometry, gpt, "gpt.bin"),
        (&vb, modified, "vendor_boot.img"),
        (&rec, twrp_bytes, "twrp.img"),
        (&misc, patched_misc, "misc.img"),
    ] {
        let path = work.path().join(name);
        backup::write_new(&path, &bytes)?;
        write_verified(
            device,
            p,
            &path,
            &work.path().join(format!("verify_{name}")),
            log,
        )?;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)] // Explicit transport, recovery and clock seams for fault tests.
pub fn run(
    device: &mut impl FinishIo,
    backup: &VerifiedBackup,
    loader: &Path,
    twrp: Option<&Path>,
    connector: &mut impl Connector,
    clock: &mut impl workflow::Clock,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    let assessment = preflight(backup)?;
    let (dir, mut j) = open_journal(backup)?;
    if let Assessment::Verified {
        current,
        changed: true,
        ..
    } = assessment
        && !restore_proof(backup, &j)?
    {
        let current = current.ok_or(crate::msg!("core_changed_region_invalid"))?;
        ui.status(&crate::msg!("core_restore_bootstrap"), false);
        bootstrap(
            device,
            backup,
            twrp.ok_or(crate::msg!("core_twrp_required"))?,
            current,
            log,
        )?;
        save(&dir, &mut j, RestorePhase::BootRequested)?;
        device.reboot_recovery(log)?;
        ui.phase(Phase::RecoveryWaiting);
        ui.status(&crate::msg!("core_restore_waiting_twrp"), true);
        let mut recovery = connector.connect(backup, ui, log)?;
        recovery.verify_identity(backup)?;
        let scratch = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
        let before = recovery.read(&scratch.path().join("read1"), log)?;
        let second = recovery.read(&scratch.path().join("read2"), log)?;
        let p = progress(backup)?.ok_or(crate::msg!("core_conversion_record_missing"))?;
        let (planned, expected) = proof(backup, &p)?;
        deviceinfo::verify(&expected, &before, &second)?;
        preserve(&dir.join("deviceinfo_before.bin"), &before)?;
        j.before_sha256 = Some(backup::digest_bytes(&before));
        j.planned_sha256 = Some(backup::digest_bytes(&planned));
        // Transfers and the dry-run precede the durable write intent.
        recovery.prepare(&before, &planned, log)?;
        save(&dir, &mut j, RestorePhase::RegionWritePending)?;
        ui.phase(Phase::Planned);
        ui.status(&crate::msg!("core_restore_writing"), false);
        let (same, fresh) =
            recovery.apply(&before, &planned, &scratch.path().join("apply"), log)?;
        deviceinfo::verify(&planned, &same, &fresh)?;
        let independent = recovery.read(&scratch.path().join("independent"), log)?;
        deviceinfo::verify(&planned, &independent, &independent)?;
        preserve(&dir.join("deviceinfo_after.bin"), &independent)?;
        save(&dir, &mut j, RestorePhase::RegionVerified)?;
        recovery.verify_identity(backup)?;
        if let Err(e) = recovery.reboot_edl() {
            live!(
                log,
                "{}",
                crate::msg!("core_restore_edl_reboot_response", e = e)
            );
        }
        drop(recovery);
        super::wait_for_edl(device, loader, clock, ui, log)?;
        if !ui.finish_wait() {
            return Err(crate::msg!("core_restore_wait_stopped"));
        }
    }
    // Recheck the PC proof and binding after every wait, before final writes.
    preflight(backup)?;
    restore_partitions(device, backup, &dir, &mut j, ui, log)
}

/// The restore journal, created on the first attempt and kept on reentry.
fn open_journal(backup: &VerifiedBackup) -> Result<(PathBuf, Journal), String> {
    let dir = directory(backup, true)?;
    let mut j = match load_journal(backup)? {
        Some(j) => j,
        None => Journal {
            version: 1,
            manifest_sha256: backup::hash_file(&backup.path().join(backup::MANIFEST_NAME), None)?.1,
            progress_sha256: progress_hash(backup)?,
            phase: RestorePhase::Prepared,
            before_sha256: None,
            planned_sha256: None,
        },
    };
    let phase = j.phase;
    save(&dir, &mut j, phase)?;
    // This journal also prevents conversion resume when the conversion itself
    // stopped before its first durable recovery_progress checkpoint.
    if j.progress_sha256.is_some() {
        backup.mark(TransactionStage::RecoveryRequired)?;
    }
    Ok((dir, j))
}

pub fn twrp_other_message() -> String {
    crate::msg!("core_restore_twrp_other")
}

/// Original restore started with the tablet already in TWRP: DeviceInfo is
/// read there first. Equal to the original, nothing is written; equal to the
/// value this run planned, the original is written back and verified; any
/// other value stops without a write. EDL then restores the partitions.
#[allow(clippy::too_many_arguments)] // Explicit transport, recovery and clock seams for fault tests.
pub fn run_from_twrp(
    device: &mut impl FinishIo,
    recovery: &mut impl Session,
    backup: &VerifiedBackup,
    loader: &Path,
    clock: &mut impl workflow::Clock,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    backup.verify()?;
    let p = progress(backup)?;
    if p.as_ref().is_some_and(|p| {
        matches!(
            p.phase,
            Phase::FirmwareWriting | Phase::FinalReboot | Phase::Done
        )
    }) {
        return Err(crate::msg!("core_backup_firmware_started"));
    }
    let (dir, mut j) = open_journal(backup)?;
    recovery.verify_identity(backup)?;
    match p.as_ref().filter(|p| p.attempt.is_some()) {
        Some(p) => {
            let (original, planned) = proof(backup, p)?;
            ui.phase(Phase::Planned);
            ui.status(&crate::msg!("core_restore_twrp_reading"), false);
            let scratch = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
            let first = recovery.read(&scratch.path().join("twrp1"), log)?;
            let second = recovery.read(&scratch.path().join("twrp2"), log)?;
            if first != second {
                return Err(crate::msg!("core_reads_differ_not_written"));
            }
            if first == original {
                live!(log, "{}", crate::msg!("core_restore_twrp_original"));
            } else if first == planned {
                live!(log, "{}", crate::msg!("core_restore_twrp_revert"));
                preserve(&dir.join("deviceinfo_before.bin"), &first)?;
                j.before_sha256 = Some(backup::digest_bytes(&first));
                j.planned_sha256 = Some(backup::digest_bytes(&original));
                // Transfers and the dry-run precede the durable write intent.
                recovery.prepare(&first, &original, log)?;
                save(&dir, &mut j, RestorePhase::RegionWritePending)?;
                ui.status(&crate::msg!("core_restore_writing"), false);
                let (same, fresh) =
                    recovery.apply(&first, &original, &scratch.path().join("apply"), log)?;
                deviceinfo::verify(&original, &same, &fresh)?;
                let independent = recovery.read(&scratch.path().join("independent"), log)?;
                deviceinfo::verify(&original, &independent, &independent)?;
                preserve(&dir.join("deviceinfo_after.bin"), &independent)?;
                save(&dir, &mut j, RestorePhase::RegionVerified)?;
            } else {
                live!(log, "{}", twrp_other_message());
                return Err(twrp_other_message());
            }
        }
        None => {
            // DeviceInfo was never read, so it was never written either.
            if p.as_ref().is_some_and(|p| {
                !matches!(
                    p.phase,
                    Phase::Preflight
                        | Phase::BackedUp
                        | Phase::Staging
                        | Phase::Staged
                        | Phase::RecoveryRequested
                        | Phase::RecoveryWaiting
                )
            }) {
                return Err(crate::msg!("core_restore_no_original_record"));
            }
            live!(log, "{}", crate::msg!("core_restore_before_read"));
        }
    }
    recovery.verify_identity(backup)?;
    ui.status(&crate::msg!("core_restore_to_edl"), false);
    if let Err(e) = recovery.reboot_edl() {
        live!(
            log,
            "{}",
            crate::msg!("core_restore_edl_reboot_response", e = e)
        );
    }
    super::wait_for_edl(device, loader, clock, ui, log)?;
    if !ui.finish_wait() {
        return Err(crate::msg!("core_restore_wait_stopped"));
    }
    restore_partitions(device, backup, &dir, &mut j, ui, log)
}

/// The common end of every restore: originals written back in EDL.
fn restore_partitions(
    device: &mut impl FinishIo,
    backup: &VerifiedBackup,
    dir: &Path,
    j: &mut Journal,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    ui.phase(Phase::Restoring);
    ui.status(&crate::msg!("core_restore_partitions"), false);
    super::restore_originals(device, backup, log)?;
    save(dir, j, RestorePhase::Complete)?;
    backup.mark(TransactionStage::Restored)?;
    live!(log, "{}", crate::msg!("core_restore_done"));
    let mut warnings = vec![];
    if let Err(e) = device.reset(crate::stage1::ResetMode::System, log) {
        warnings.push(crate::msg!("core_system_boot_failed", e = e));
    }
    Ok(warnings)
}
