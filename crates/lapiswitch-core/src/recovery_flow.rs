//! TWRP/DeviceInfo conversion and verified original restoration.
use crate::{
    backup::{self, TransactionStage, VerifiedBackup},
    deviceinfo::{self, RollbackMode},
    firmware::FirmwareReport,
    stage1::StageIo,
    state::{ResumeAction, RunState, Stage, StateStore},
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
    pub original_region: crate::region::Region,
    pub mode: RollbackMode,
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
    Err(format!(
        "{connection} 연결 대기 시간 초과: 3분(180초) 안에 연결이 확인되지 않았습니다. 백업과 작업 기록을 보존했습니다. 연결 후 이어하기 또는 EDL에서 원본 복구를 진행하세요."
    ))
}
pub trait Ui {
    /// Presentation only: does not change the durable write/resume checkpoints.
    fn phase(&mut self, _phase: Phase) {}
    fn confirm(&mut self, transition: Transition) -> Result<(), String>;
    fn status(&mut self, text: &str, stoppable: bool);
    fn cancelled(&self) -> bool;
    fn finish_wait(&mut self) -> bool;
}
pub trait DeviceInfoIo {
    fn read(&mut self, out: &Path, log: &mut Vec<String>) -> Result<Vec<u8>, String>;
    /// Transfer and dry-run this exact plan without any write request. A
    /// failure here happens before the durable write intent is recorded.
    fn prepare(
        &mut self,
        before: &[u8],
        planned: &[u8],
        mode: RollbackMode,
        log: &mut Vec<String>,
    ) -> Result<(), String>;
    /// The only write request, for the plan just prepared in this session.
    /// Never retry this operation; a transport error leaves a pending intent.
    fn apply(
        &mut self,
        before: &[u8],
        planned: &[u8],
        mode: RollbackMode,
        out: &Path,
        log: &mut Vec<String>,
    ) -> Result<(Vec<u8>, Vec<u8>), String>;
}
pub fn resume_action(state: &RunState, transaction: Option<TransactionStage>) -> ResumeAction {
    let phase = state.recovery.as_ref().expect("recovery branch").phase;
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
    state
        .recovery
        .as_mut()
        .ok_or("새 흐름 기록이 없습니다.")?
        .phase = phase;
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
        state.recovery.as_ref().expect("checked"),
    )
}
pub const NOT_APPLIED_MESSAGE: &str = "DeviceInfo를 두 번 다시 읽어 최초 원본과 같음을 확인했습니다. 쓰기는 적용되지 않았으며 작업 기록을 쓰기 전 단계로 되돌렸습니다. 이어하기를 누르면 쓰기를 한 번 다시 요청하며, 원본 복구도 선택할 수 있습니다.";

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
        Err("연결 대기를 중지했습니다. 백업과 작업 기록을 보존했습니다. 이어하기 또는 원본 복구를 선택하세요.".into())
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
        return Err(format!("{} 덤프 길이가 다릅니다.", part.name));
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
            return Err(format!("{} 쓰기 후 SHA-256 검증 실패", part.name));
        }
    }
    Ok(())
}
fn part(backup: &VerifiedBackup, name: &str, lun: u8) -> Result<Partition, String> {
    Ok(storage::find_partition(&backup.manifest().snapshot, name, lun)?.clone())
}
pub fn recovery_name(slot: crate::slot::Slot) -> String {
    format!("recovery{}", slot.suffix())
}

/// Reconstruct the exact durable plan from the original and the recorded
/// target/mode. Its hash must match the intent saved before the only write.
fn saved_plan(
    dir: &Path,
    state: &RunState,
    record: &Attempt,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if record.mode != state.options.rollback_mode {
        return Err("롤백 옵션이 검증 기록과 다릅니다.".into());
    }
    let original = dir.join("before_shared_1.bin");
    if backup::hash_file(&original, None)?
        != (deviceinfo::SHARED_SIZE as u64, record.before_sha256.clone())
    {
        return Err("DeviceInfo 원본 백업이 변경되었습니다.".into());
    }
    let before = std::fs::read(original).map_err(|e| e.to_string())?;
    let planned = deviceinfo::plan(&before, state.target_region, record.mode)?;
    if backup::digest_bytes(&planned) != record.planned_sha256 {
        return Err("DeviceInfo 변경 계획이 저장된 쓰기 의도와 다릅니다.".into());
    }
    // Older app versions retained a separate plan. Check it until successful
    // verification prunes the redundant files; never ignore a tampered plan.
    let legacy = dir.join("planned_shared.bin");
    if legacy.try_exists().map_err(|e| e.to_string())?
        && backup::hash_file(&legacy, None)?
            != (
                deviceinfo::SHARED_SIZE as u64,
                record.planned_sha256.clone(),
            )
    {
        return Err("DeviceInfo 예정 파일이 변경되었습니다.".into());
    }
    Ok((before, planned))
}

/// Both retained files and the durable plan hash are required before firmware.
fn verify_evidence(backup: &VerifiedBackup, state: &RunState) -> Result<(), String> {
    let record = state
        .recovery
        .as_ref()
        .and_then(|p| p.attempt.as_ref())
        .ok_or("DeviceInfo 검증 증거가 없습니다.")?;
    let dir = attempt_dir(backup, &record.directory)?;
    let (before, planned) = saved_plan(&dir, state, record)?;
    let final_read = dir.join("after_independent.bin");
    if backup::hash_file(&final_read, None)?
        != (
            deviceinfo::SHARED_SIZE as u64,
            record.planned_sha256.clone(),
        )
    {
        return Err("DeviceInfo 최종 재읽기 검증 증거가 변경되었습니다.".into());
    }
    let after = std::fs::read(final_read).map_err(|e| e.to_string())?;
    deviceinfo::validate_plan(&before, &after, state.target_region, record.mode)?;
    deviceinfo::verify(&planned, &after, &after)
}

/// Only known redundant files from older app versions are pruned after a
/// verified final read and durable RegionVerified checkpoint. Never recurse.
fn prune_intermediates(dir: &Path, state: &mut RunState, log: &mut Vec<String>) {
    for name in [
        "before_shared_2.bin",
        "before_logical.bin",
        "planned_shared.bin",
        "byte_changes.json",
        "after_same_session.bin",
        "after_new_session.bin",
        "read1.before.bin",
        "read2.before.bin",
        "apply.same.bin",
        "apply.fresh.bin",
        "independent.before.bin",
    ] {
        if let Err(error) = std::fs::remove_file(dir.join(name))
            && error.kind() != std::io::ErrorKind::NotFound
        {
            let warning = format!("DeviceInfo 중간 파일 정리 미완료 ({name}): {error}");
            live!(log, "경고: {warning}");
            state.warnings.push(warning);
        }
    }
}

fn original_bytes(backup: &VerifiedBackup, name: &str) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(backup.file_path(name)?).map_err(|e| e.to_string())?;
    let record = backup.entry(name)?;
    if bytes.len() as u64 != record.size_bytes || backup::digest_bytes(&bytes) != record.sha256 {
        return Err(format!("원본 백업 변경 감지: {name}"));
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
        return Err("TWRP 재부팅 전 GPT가 준비 완료 상태와 다릅니다.".into());
    }
    let rec = part(backup, &recovery_name(backup.manifest().slot), 4)?
        .prefix(crate::recovery_asset::SIZE)?;
    data(device, &rec, &work.path().join("recovery.img"), log)?;
    crate::recovery_asset::verify(&work.path().join("recovery.img"))?;
    let misc = part(backup, "misc", 0)?;
    let expected = deviceinfo::recovery_misc(&original_bytes(backup, "misc.img")?)?;
    if data(device, &misc, &work.path().join("misc.img"), log)? != expected {
        return Err("TWRP 재부팅 전 misc가 준비 완료 상태와 다릅니다.".into());
    }
    let persist = part(backup, "persist", 0)?;
    data(device, &persist, &work.path().join("persist.img"), log)?;
    if backup::hash_file(&work.path().join("persist.img"), None)?.1
        != backup.entry("persist.img")?.sha256
    {
        return Err("재부팅할 기기의 persist가 백업과 다릅니다.".into());
    }
    Ok(())
}

/// Caller already bound the backup/state and saved the active slot from ADB /
/// bootloader. An EDL-only AVB estimate is never used for a Recovery write.
pub fn stage(
    device: &mut impl StageIo,
    backup: &VerifiedBackup,
    twrp: &Path,
    state: &mut RunState,
    store: &StateStore,
    log: &mut Vec<String>,
) -> Result<(), String> {
    backup.verify()?;
    crate::recovery_asset::verify(twrp)?;
    if !matches!(backup.manifest().version, 3..=5) || backup.manifest().slot_estimated {
        return Err("활성 슬롯이 확정되지 않아 Recovery에 쓰지 않았습니다. ADB 또는 bootloader fastboot에서 시작하세요.".into());
    }
    if state.recovery.as_ref().map(|p| p.phase) != Some(Phase::BackedUp)
        || backup.transaction_stage()? != TransactionStage::BackupVerified
    {
        return Err("TWRP 준비를 새로 실행할 수 없는 상태입니다.".into());
    }
    device.ensure_single_target()?;
    device.validate_snapshot(&backup.manifest().snapshot)?;
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let original = original_bytes(backup, "gpt_main4.bin")?;
    let gpt = storage::validate_gpt(&original)?;
    let geometry = storage::primary_geometry(&original)?;
    let rec = part(backup, &recovery_name(backup.manifest().slot), 4)?;
    rec.prefix(crate::recovery_asset::SIZE)?;
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
                return Err("이전 준비 이미지가 예상값과 다릅니다.".into());
            }
        } else {
            backup::write_new(&artifact, &bytes)?;
        }
    }
    let twrp_copy = work.path().join("twrp.img");
    std::fs::copy(twrp, &twrp_copy).map_err(|e| e.to_string())?;
    crate::recovery_asset::verify(&twrp_copy)?;
    // Re-read all originals immediately before the first write.
    for (p, name) in [
        (&geometry, "gpt_main4.bin"),
        (&rec, &format!("{}.img", rec.name)),
        (&misc, "misc.img"),
    ] {
        let current = work.path().join(format!("before_{}", name));
        data(device, p, &current, log)?;
        if backup::hash_file(&current, None)?.1 != backup.entry(name)?.sha256 {
            return Err(format!("백업 이후 {}가 변경되었습니다.", p.name));
        }
    }
    checkpoint(state, store, Phase::Staging)?;
    backup.mark(TransactionStage::GptWriting)?;
    let result = (|| {
        live!(
            log,
            "TWRP 준비: 수정 GPT와 활성 슬롯 {} Recovery를 기록합니다.",
            backup.manifest().slot.suffix()
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
            "TWRP 준비 오류: {error}. 원본 GPT·Recovery 복구를 시도합니다. misc 덤프는 보존만 합니다."
        );
        match restore_originals(device, backup, log) {
            Ok(()) => {
                backup.mark(TransactionStage::Restored)?;
                checkpoint(state, store, Phase::Restored)?;
            }
            Err(restore) => {
                let _ = backup.mark(TransactionStage::RecoveryRequired);
                return Err(format!("{error}\n원본 구성 복구 실패: {restore}"));
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
    if !matches!(backup.manifest().version, 3..=5) {
        return Err("새 Recovery 백업이 아닙니다.".into());
    }
    let journal = backup.path().join("recovery_progress.json");
    if journal.exists() {
        let progress: Progress = backup::read_json(&journal)?;
        if matches!(
            progress.phase,
            Phase::FirmwareWriting | Phase::FinalReboot | Phase::Done
        ) {
            return Err("공식 펌웨어 설치를 시작해 이전 Recovery/GPT 복원은 차단했습니다. 설치를 이어가세요.".into());
        }
    } else if backup.transaction_stage()? != TransactionStage::BackupVerified {
        return Err("Recovery 진행 기록이 누락되어 과거 구성 복원을 차단했습니다.".into());
    }
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let (geometry, rec) = validate_restore_device(device, backup, work.path(), log)?;
    let mut errors = vec![];
    if backup.manifest().version >= 4 {
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
    }
    for (p, name) in [
        (&rec, format!("{}.img", rec.name)),
        (&geometry, "gpt_main4.bin".to_owned()),
    ] {
        let source = work.path().join(&name);
        std::fs::copy(backup.file_path(&name)?, &source).map_err(|e| e.to_string())?;
        if backup::hash_file(&source, None)?.1 != backup.entry(&name)?.sha256 {
            return Err("원본 복사 검증 실패".into());
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
        "원본 GPT·{} 복원 및 재읽기 검증 완료. misc 원본 덤프는 보존했습니다.",
        rec.name
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
        return Err("GPT disk GUID가 백업 기기와 다릅니다.".into());
    }
    // TWRP mounts persist RW for qseecomd/secure storage; its pre-boot hash
    // is not an invariant. Bind restoration by the unique original GPT GUID
    // and the still-intact model AVB, as the legacy manual restore does.
    let mut matched = false;
    for name in ["vbmeta_system_a", "vbmeta_system_b"] {
        if let Some(avb) = device.read_avb(name, log)? {
            matched |= avb.build_fingerprint().is_some_and(|fp| {
                ltbox_core::model::fingerprint_model_match(&fp, crate::firmware::MODEL)
            });
        }
    }
    if !matched {
        return Err("원복할 기기가 TB520FU로 확인되지 않았습니다.".into());
    }
    let rec = part(backup, &recovery_name(backup.manifest().slot), 4)?;
    let live_gpt = data(device, &geometry, &work.join("current_gpt.bin"), log)?;
    match storage::validate_gpt(&live_gpt) {
        Ok(table) => {
            storage::match_gpt_partition(&table, &rec)?;
            let mut targets = vec![rec.clone(), part(backup, "misc", 0)?];
            if backup.manifest().version >= 4 {
                targets.push(part(
                    backup,
                    &format!("vendor_boot{}", backup.manifest().slot.suffix()),
                    4,
                )?);
            }
            for p in targets.iter().filter(|p| p.lun == 4) {
                storage::match_gpt_partition(&table, p)?;
            }
            device.validate_snapshot(&targets)?;
        }
        Err(error) => live!(
            log,
            "현재 GPT 검사 실패: {error}. 원본 disk GUID를 확인했으므로 백업 좌표로 복구합니다."
        ),
    }
    Ok((geometry, rec))
}

fn attempt_dir(backup: &VerifiedBackup, name: &str) -> Result<PathBuf, String> {
    // Older checkpoints still refer to the generated deviceinfo-* directory.
    if name != "DeviceInfo"
        && (!name.starts_with("deviceinfo-")
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    {
        return Err("잘못된 DeviceInfo 시도 경로".into());
    }
    let path = backup.work_dir()?.join(name);
    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err("DeviceInfo 경로가 링크입니다.".into());
        }
    }
    if !meta.file_type().is_dir() {
        return Err("DeviceInfo 시도 폴더가 잘못되었습니다.".into());
    }
    Ok(path)
}
pub fn transact(
    device: &mut impl DeviceInfoIo,
    backup: &VerifiedBackup,
    firmware: &FirmwareReport,
    state: &mut RunState,
    store: &StateStore,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<(), String> {
    backup.verify()?;
    restore::ensure_conversion_allowed(backup)?;
    state.verify_binding(backup, firmware)?;
    stopped(ui)?;
    let phase = state
        .recovery
        .as_ref()
        .ok_or("DeviceInfo 작업 기록 없음")?
        .phase;
    if matches!(phase, Phase::WritePending | Phase::RegionVerified) {
        let record = state
            .recovery
            .as_ref()
            .and_then(|p| p.attempt.clone())
            .ok_or("DeviceInfo 의도 기록 없음")?;
        let dir = attempt_dir(backup, &record.directory)?;
        let (before, planned) = saved_plan(&dir, state, &record)?;
        let verify = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
        let a = device.read(&verify.path().join("resume_1"), log)?;
        let b = device.read(&verify.path().join("resume_2"), log)?;
        // An unresolved request whose result is still the exact original was
        // not applied. Return to Planned and stop; never write in this call.
        if phase == Phase::WritePending && a == before && b == before {
            checkpoint(state, store, Phase::Planned)?;
            live!(log, "{NOT_APPLIED_MESSAGE}");
            return Err(NOT_APPLIED_MESSAGE.into());
        }
        deviceinfo::verify(&planned,&a,&b).map_err(|_|"이전 DeviceInfo 쓰기 결과를 확인할 수 없습니다. 자동 재쓰기 없이 상태와 백업을 보존합니다.".to_owned())?;
        let evidence = dir.join("after_independent.bin");
        if evidence.exists() {
            if std::fs::read(&evidence).map_err(|e| e.to_string())? != planned {
                return Err("기존 독립 재읽기 증거가 변경되었습니다.".into());
            }
        } else {
            backup::write_new(&evidence, &b)?;
        }
        checkpoint(state, store, Phase::RegionVerified)?;
        prune_intermediates(&dir, state, log);
        return Ok(());
    }
    ui.status(
        "DeviceInfo 원본을 읽어 백업하고 변경 범위를 비교합니다.",
        false,
    );
    let attempt = backup.work_dir()?.join("DeviceInfo");
    match std::fs::create_dir(&attempt) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            attempt_dir(backup, "DeviceInfo")?;
        }
        Err(error) => return Err(error.to_string()),
    }
    // Transport copies are private scratch files, removed on scope exit.
    // Preserve the very first read immediately, even if a later check fails.
    let scratch = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    let before = device.read(&scratch.path().join("read1"), log)?;
    deviceinfo::region(&before)?;
    let original = attempt.join("before_shared_1.bin");
    if original.try_exists().map_err(|e| e.to_string())? {
        // A retry after a failed read must keep the very first original.
        if backup::hash_file(&original, None)?
            != (
                deviceinfo::SHARED_SIZE as u64,
                backup::digest_bytes(&before),
            )
        {
            return Err(
                "DeviceInfo가 최초 원본 백업과 다릅니다. 원본을 보존하고 쓰기를 중단합니다.".into(),
            );
        }
        if let Some(record) = state.recovery.as_ref().and_then(|p| p.attempt.as_ref())
            && record.directory == "DeviceInfo"
        {
            saved_plan(&attempt, state, record)?;
        }
    } else {
        backup::write_new(&original, &before)?;
    }
    let second = device.read(&scratch.path().join("read2"), log)?;
    if before != second {
        return Err("DeviceInfo 독립 읽기 두 개가 일치하지 않습니다. 쓰지 않았습니다.".into());
    }
    let mode = state.options.rollback_mode;
    if mode == RollbackMode::Keep {
        let indices = deviceinfo::indices(&before)?;
        crate::prepare::check_rollback(
            firmware,
            crate::rollback::DeviceFloors::PerLocation(crate::rollback::RollbackIndices {
                boot: indices[3],
                vbmeta_system: indices[2],
            }),
            log,
        )
        .map_err(|e| e.to_string())?;
    }
    let planned = deviceinfo::plan(&before, state.target_region, mode)?;
    let changed = before.iter().zip(&planned).filter(|(a, b)| a != b).count();
    live!(
        log,
        "DeviceInfo: {} → {}, 롤백 {:?}, 변경 {}바이트. 나머지 영역은 보존합니다.",
        deviceinfo::region(&before)?.token(),
        state.target_region.token(),
        mode,
        changed
    );
    state.recovery.as_mut().expect("checked").attempt = Some(Attempt {
        directory: attempt
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("시도 이름 없음")?
            .into(),
        before_sha256: backup::digest_bytes(&before),
        planned_sha256: backup::digest_bytes(&planned),
        original_region: deviceinfo::region(&before)?,
        mode,
    });
    if backup::hash_file(&attempt.join("before_shared_1.bin"), None)?
        != (before.len() as u64, backup::digest_bytes(&before))
    {
        return Err("쓰기 전 PC DeviceInfo 원본 백업 검증 실패".into());
    }
    checkpoint(state, store, Phase::Planned)?;
    stopped(ui)?;
    if before != planned {
        ui.status("DeviceInfo 쓰기 요청 및 독립 재읽기 검증 중입니다.", false);
        // Transfers and the dry-run cannot change DeviceInfo. Their failures
        // stay at Planned instead of becoming an unresolved write.
        device.prepare(&before, &planned, mode, log)?;
        checkpoint(state, store, Phase::WritePending)?;
        // A durable PC intent exists before the only write call.
        let (same, fresh) =
            match device.apply(&before, &planned, mode, &scratch.path().join("apply"), log) {
                Ok(readback) => readback,
                Err(error) if unapplied(device, &before, scratch.path(), log) => {
                    checkpoint(state, store, Phase::Planned)
                        .map_err(|checkpoint| format!("{error}\n{checkpoint}"))?;
                    live!(log, "{NOT_APPLIED_MESSAGE}");
                    return Err(format!("{error}\n{NOT_APPLIED_MESSAGE}"));
                }
                Err(error) => return Err(error),
            };
        deviceinfo::verify(&planned, &same, &fresh)?;
    } else {
        live!(
            log,
            "DeviceInfo가 목표 리전·롤백 설정과 같습니다. 쓰기를 생략합니다."
        );
    }
    // Independent helper process, freshly opened TA session, compared on PC too.
    let independent = device.read(&scratch.path().join("independent"), log)?;
    deviceinfo::verify(&planned, &independent, &independent)?;
    backup::write_new(&attempt.join("after_independent.bin"), &independent)?;
    checkpoint(state, store, Phase::RegionVerified)?;
    prune_intermediates(&attempt, state, log);
    live!(
        log,
        "DeviceInfo 목표 리전·롤백 및 전체 4096바이트 재읽기 검증 완료."
    );
    Ok(())
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
    ui.status(
        "EDL 연결을 기다립니다. PC와 태블릿의 USB 연결을 유지해 주세요.",
        true,
    );
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
/// Recovery/GPT restoration have completed.
pub fn finish(
    device: &mut impl FinishIo,
    backup: &VerifiedBackup,
    firmware: &FirmwareReport,
    state: &mut RunState,
    store: &StateStore,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<(), String> {
    backup.verify()?;
    if backup.transaction_stage()? != TransactionStage::S4Done {
        return Err("TWRP 준비 완료 기록이 없습니다. 원본 구성 복구가 필요합니다.".into());
    }
    state.verify_binding(backup, firmware)?;
    if !matches!(backup.manifest().version, 3..=5) || state.version != 2 {
        return Err("흐름/백업 형식 불일치".into());
    }
    let mut phase = state.recovery.as_ref().ok_or("Recovery 기록 없음")?.phase;
    ui.phase(phase);
    if phase == Phase::FinalReboot {
        state
            .warnings
            .push("재부팅 전달 직후 중단됐습니다. 기기의 부팅 상태를 확인하세요.".into());
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
        if phase == Phase::Staged {
            ui.phase(Phase::RecoveryRequested);
            ui.confirm(Transition::Twrp)?;
            stopped(ui)?;
            validate_staged(device, backup, log)?;
            checkpoint(state, store, Phase::RecoveryRequested)?;
            device.reboot_recovery(log)?;
            checkpoint(state, store, Phase::RecoveryWaiting)?;
        }
        ui.phase(Phase::RecoveryWaiting);
        let mut recovery = native::Recovery::wait(backup, ui, log)?;
        ui.phase(Phase::Planned);
        transact(&mut recovery, backup, firmware, state, store, ui, log)?;
        ui.phase(Phase::EdlRequested);
        ui.confirm(Transition::Edl)?;
        stopped(ui)?;
        // Confirmation can be open for minutes. Re-read, never assume old bytes.
        transact(&mut recovery, backup, firmware, state, store, ui, log)?;
        recovery.verify_identity(backup)?;
        checkpoint(state, store, Phase::EdlRequested)?;
        if let Err(e) = recovery.reboot_edl() {
            live!(log, "EDL 재부팅 응답: {e}. 연결을 확인합니다.");
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
            &firmware.loader,
            &mut workflow::native::SystemClock::default(),
            ui,
            log,
        )?;
        stopped(ui)?;
    } else {
        return Err("이어갈 수 없는 Recovery 단계입니다. 작업 기록을 확인하세요.".into());
    }
    if !ui.finish_wait() {
        return Err("작업 중지 요청을 처리했습니다. 쓰기는 시작하지 않았습니다.".into());
    }
    verify_evidence(backup, state)?;
    ui.phase(Phase::Restoring);
    ui.status("원본 GPT·Recovery를 복원하고 검증합니다.", false);
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
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    workflow::confirm_identity(device, backup, state, store, work.path(), log)?;
    backup.verify()?;
    restore::ensure_conversion_allowed(backup)?;
    state.verify_binding(backup, firmware)?;
    checkpoint(state, store, Phase::FirmwareWriting)?;
    ui.phase(Phase::FirmwareWriting);
    ui.status(
        "공식 펌웨어를 설치하고 사용자 데이터를 초기화합니다.",
        false,
    );
    device.flash_firmware(firmware, true, log)?;
    ui.phase(Phase::FinalReboot);
    ui.status(
        "GPT·국가 코드·활성 슬롯을 확인한 뒤 시스템 부팅을 요청합니다.",
        false,
    );
    workflow::restore_gpt_if_needed(device, backup, work.path(), log)?;
    if let Some(code) = &state.options.country {
        state
            .warnings
            .extend(workflow::country_change(device, code, work.path(), log));
    }
    device.set_active_a(log)?;
    checkpoint(state, store, Phase::FinalReboot)?;
    if let Some(warning) = device.final_reboot(log) {
        state.warnings.push(warning);
    }
    checkpoint(state, store, Phase::Done)?;
    live!(
        log,
        "DeviceInfo 검증·원본 구성 복원·펌웨어 설치 완료. 시스템 부팅을 요청했습니다."
    );
    Ok(())
}
