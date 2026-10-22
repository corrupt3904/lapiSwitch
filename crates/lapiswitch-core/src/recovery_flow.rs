//! TWRP/DeviceInfo conversion and verified original restoration.
use crate::{
    backup::{self, TransactionStage, VerifiedBackup},
    deviceinfo::{self, map},
    firmware::FirmwareReport,
    region::Region,
    rollback::{self, Downgrade, RollbackIndices},
    stage1::StageIo,
    state::{FirmwareBinding, Request, ResumeAction, RunState, Stage, StateStore},
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
    fn cancelled(&self) -> bool;
    fn finish_wait(&mut self) -> bool;
}
/// Field values chosen in the editor. A changed region needs the folder of
/// that region's official firmware, which is then checked and installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRequest {
    pub edits: map::Edits,
    pub firmware_dir: Option<PathBuf>,
}
pub trait DeviceInfoIo {
    fn read(&mut self, out: &Path, log: &mut Vec<String>) -> Result<Vec<u8>, String>;
    /// Transfer and dry-run this exact plan without any write request. A
    /// failure here happens before the durable write intent is recorded.
    fn prepare(
        &mut self,
        before: &[u8],
        planned: &[u8],
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
pub const ROLLBACK_LOWER_MESSAGE: &str =
    "설치할 펌웨어의 롤백인덱스가 기기 DeviceInfo에 저장된 값보다 낮습니다.";
pub const ROLLBACK_CANCELLED_MESSAGE: &str =
    "롤백인덱스 경고에서 작업을 취소했습니다. DeviceInfo는 변경하지 않았습니다.";
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
        return Err("DeviceInfo 원본 백업이 변경되었습니다.".into());
    }
    let before = std::fs::read(original).map_err(|e| e.to_string())?;
    let planned = map::apply(layout, &before, &record.edits)?;
    if backup::digest_bytes(&planned) != record.planned_sha256 {
        return Err("DeviceInfo 변경 계획이 저장된 쓰기 의도와 다릅니다.".into());
    }
    Ok((before, planned))
}

/// Both retained files and the durable plan hash are required before firmware.
fn verify_evidence(backup: &VerifiedBackup, state: &RunState) -> Result<(), String> {
    let record = state
        .recovery
        .attempt
        .as_ref()
        .ok_or("DeviceInfo 검증 증거가 없습니다.")?;
    let dir = attempt_dir(backup, &record.directory)?;
    let (before, planned) = saved_plan(backup.device().layout(), &dir, record)?;
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
    map::check_plan(backup.device().layout(), &before, &after)?;
    deviceinfo::verify(&planned, &after, &after)
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
        .prefix(crate::recovery_asset::size(backup.device()))?;
    data(device, &rec, &work.path().join("recovery.img"), log)?;
    crate::recovery_asset::verify(backup.device(), &work.path().join("recovery.img"))?;
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
        return Err("TWRP 준비를 새로 실행할 수 없는 상태입니다.".into());
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
                return Err("이전 준비 이미지가 예상값과 다릅니다.".into());
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
        return Err(format!(
            "원복할 기기가 백업의 기기({})로 확인되지 않았습니다.",
            backup.device()
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
            "현재 GPT 검사 실패: {error}. 원본 disk GUID를 확인했으므로 백업 좌표로 복구합니다."
        ),
    }
    Ok((geometry, rec))
}

fn attempt_dir(backup: &VerifiedBackup, name: &str) -> Result<PathBuf, String> {
    if name != ATTEMPT_DIR {
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
/// Edits of this run: the conversion preset, or the user's choice in the
/// editor. An edit that changes the region binds that region's firmware.
fn choose_edits(
    before: &[u8],
    state: &mut RunState,
    firmware: &mut Option<FirmwareReport>,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Option<map::Edits>, String> {
    let layout = state.device()?.layout();
    match state.request {
        Request::Convert {
            target,
            rollback_mode,
        } => {
            if firmware.as_ref().map(|firmware| firmware.region) != Some(target) {
                return Err("변환 대상 리전의 펌웨어가 없습니다.".into());
            }
            Ok(Some(deviceinfo::conversion_edits(
                layout,
                before,
                target,
                rollback_mode,
            )?))
        }
        Request::Edit => {
            ui.status(
                "DeviceInfo를 읽었습니다. 수정할 값을 입력하고 변경 내용을 확인하세요.",
                false,
            );
            let Some(request) = ui.edit_deviceinfo(layout, before)? else {
                live!(log, "DeviceInfo 수정 창에서 작업 취소를 선택했습니다.");
                return Ok(None);
            };
            let planned = map::apply(layout, before, &request.edits)?;
            let target = layout
                .region(&planned)
                .ok_or("리전 필드는 PRC 또는 ROW여야 합니다.")?;
            if layout.region(before) == Some(target) {
                state.firmware = None;
                *firmware = None;
            } else {
                let dir = request.firmware_dir.ok_or_else(|| {
                    format!(
                        "리전을 {}(으)로 바꾸려면 해당 리전의 공식 펌웨어 폴더가 필요합니다.",
                        target.token()
                    )
                })?;
                let checked = crate::firmware::check_firmware(&dir, target)
                    .map_err(|error| error.to_string())?;
                if checked.device != state.device()? {
                    return Err(format!(
                        "선택한 펌웨어는 {}용입니다. {}의 공식 펌웨어를 선택하세요.",
                        checked.device,
                        state.device()?
                    ));
                }
                live!(
                    log,
                    "리전 변경 {} → {}: 펌웨어를 설치하고 사용자 데이터를 초기화합니다.",
                    layout.region(before).map_or("?", Region::token),
                    target.token()
                );
                state.firmware = Some(FirmwareBinding::new(&checked)?);
                *firmware = Some(checked);
            }
            Ok(Some(map::edits_between(layout, before, &planned)))
        }
    }
}

/// A firmware below the planned rollback indices would not boot. The stored
/// values decide; the user may reset all 32 or cancel and restore.
fn check_planned_rollback(
    layout: map::Layout,
    before: &[u8],
    edits: map::Edits,
    firmware: &FirmwareReport,
    state: &mut RunState,
    ui: &mut impl Ui,
    log: &mut Vec<String>,
) -> Result<Option<map::Edits>, String> {
    let planned = map::apply(layout, before, &edits)?;
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
        live!(log, "롤백 비교 {partition}: 펌웨어 {image}, 기기 {floor}");
    }
    let Err(downgrades) = rollback::check(firmware.indices, device) else {
        return Ok(Some(edits));
    };
    let details = rollback::describe(&downgrades);
    live!(log, "{ROLLBACK_LOWER_MESSAGE} {details}");
    if !ui.confirm_rollback_reset(&downgrades)? {
        live!(log, "{ROLLBACK_CANCELLED_MESSAGE}");
        return Ok(None);
    }
    state.warnings.push(format!(
        "{ROLLBACK_LOWER_MESSAGE} 사용자 확인 후 32개 롤백인덱스를 0으로 초기화했습니다. {details}"
    ));
    live!(log, "사용자가 롤백인덱스 0 초기화 후 계속을 선택했습니다.");
    Ok(Some(deviceinfo::with_rollback_reset(
        layout, before, &edits,
    )?))
}

/// Field changes for the log. Long opaque values (keys, device caches) are
/// named but never printed.
fn log_changes(layout: map::Layout, before: &[u8], planned: &[u8], log: &mut Vec<String>) {
    let changes = map::diff(layout, before, planned);
    live!(
        log,
        "DeviceInfo 변경 필드 {}개. 나머지 바이트는 보존합니다.",
        changes.len()
    );
    for change in changes {
        let field = &change.field;
        if field.kind == map::Kind::Bytes && field.size > 8 {
            live!(
                log,
                "  {} (0x{:04X}, {}바이트) 변경",
                field.key,
                field.offset,
                field.size
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
    state.verify_binding(backup, firmware.as_ref())?;
    stopped(ui)?;
    let phase = state.recovery.phase;
    if matches!(phase, Phase::WritePending | Phase::RegionVerified) {
        let record = state
            .recovery
            .attempt
            .clone()
            .ok_or("DeviceInfo 의도 기록 없음")?;
        let dir = attempt_dir(backup, &record.directory)?;
        let (before, planned) = saved_plan(backup.device().layout(), &dir, &record)?;
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
        deviceinfo::verify(&planned, &a, &b).map_err(|_| {
            "이전 DeviceInfo 쓰기 결과를 확인할 수 없습니다. 자동 재쓰기 없이 상태와 백업을 보존합니다.".to_owned()
        })?;
        let evidence = dir.join("after_independent.bin");
        if evidence.exists() {
            if std::fs::read(&evidence).map_err(|e| e.to_string())? != planned {
                return Err("기존 독립 재읽기 증거가 변경되었습니다.".into());
            }
        } else {
            backup::write_new(&evidence, &b)?;
        }
        checkpoint(state, store, Phase::RegionVerified)?;
        return Ok(Outcome::Verified);
    }
    ui.status(
        "DeviceInfo 원본을 읽어 백업하고 변경 범위를 비교합니다.",
        false,
    );
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
    let layout = backup.device().layout();
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
            return Err(
                "DeviceInfo가 최초 원본 백업과 다릅니다. 원본을 보존하고 쓰기를 중단합니다.".into(),
            );
        }
    } else {
        backup::write_new(&original, &before)?;
    }
    let second = device.read(&scratch.path().join("read2"), log)?;
    if before != second {
        return Err("DeviceInfo 독립 읽기 두 개가 일치하지 않습니다. 쓰지 않았습니다.".into());
    }
    // A plan recorded before an unapplied write is reused as chosen.
    let edits = if let Some(record) = state.recovery.attempt.clone() {
        saved_plan(layout, &attempt, &record)?;
        record.edits
    } else {
        let Some(edits) = choose_edits(&before, state, firmware, ui, log)? else {
            return Ok(Outcome::Cancelled);
        };
        let edits = match firmware.as_ref() {
            Some(chosen) => {
                match check_planned_rollback(layout, &before, edits, chosen, state, ui, log)? {
                    Some(edits) => edits,
                    None => return Ok(Outcome::Cancelled),
                }
            }
            None => edits,
        };
        stopped(ui)?;
        edits
    };
    let planned = map::apply(layout, &before, &edits)?;
    log_changes(layout, &before, &planned, log);
    state.recovery.attempt = Some(Attempt {
        directory: attempt
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("시도 이름 없음")?
            .into(),
        before_sha256: backup::digest_bytes(&before),
        planned_sha256: backup::digest_bytes(&planned),
        original_region: layout.region(&before),
        edits,
    });
    if backup::hash_file(&attempt.join("before_shared_1.bin"), None)?
        != (before.len() as u64, backup::digest_bytes(&before))
    {
        return Err("쓰기 전 PC DeviceInfo 원본 백업 검증 실패".into());
    }
    // The plan, and any firmware it requires, become durable here.
    checkpoint(state, store, Phase::Planned)?;
    stopped(ui)?;
    if before != planned {
        ui.status("DeviceInfo 쓰기 요청 및 독립 재읽기 검증 중입니다.", false);
        // Transfers and the dry-run cannot change DeviceInfo. Their failures
        // stay at Planned instead of becoming an unresolved write.
        device.prepare(&before, &planned, log)?;
        checkpoint(state, store, Phase::WritePending)?;
        // A durable PC intent exists before the only write call.
        let (same, fresh) =
            match device.apply(&before, &planned, &scratch.path().join("apply"), log) {
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
        live!(log, "DeviceInfo가 선택한 값과 같습니다. 쓰기를 생략합니다.");
    }
    // Independent helper process, freshly opened TA session, compared on PC too.
    let independent = device.read(&scratch.path().join("independent"), log)?;
    deviceinfo::verify(&planned, &independent, &independent)?;
    backup::write_new(&attempt.join("after_independent.bin"), &independent)?;
    checkpoint(state, store, Phase::RegionVerified)?;
    live!(log, "DeviceInfo 변경 및 전체 4096바이트 재읽기 검증 완료.");
    Ok(Outcome::Verified)
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
    ui.status(
        "작업을 취소하고 원본 구성으로 복구합니다. EDL로 전환합니다.",
        false,
    );
    recovery.verify_identity(backup)?;
    if let Err(e) = recovery.reboot_edl() {
        live!(log, "원본 복구 EDL 재부팅 응답: {e}. 연결을 확인합니다.");
    }
    wait_for_edl(device, loader, clock, ui, log)?;
    if !ui.finish_wait() {
        return Err("연결 대기를 중지했습니다. 백업과 작업 기록을 보존했습니다. EDL에서 원본 복구를 진행하세요.".into());
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
            "복구 완료된 작업 기록을 보관했습니다: {}",
            path.display()
        ),
        Ok(None) => {}
        Err(error) => warnings.push(format!(
            "원본 복구는 완료했지만 이전 작업 기록 정리를 마치지 못했습니다: {error}"
        )),
    }
    Ok(warnings)
}

/// The run ends as an error either way: the conversion did not happen.
pub fn cancelled(restore: Result<Vec<String>, String>) -> String {
    match restore {
        Ok(warnings) => {
            let mut text = format!(
                "{ROLLBACK_CANCELLED_MESSAGE} 원본 GPT·Recovery·vendor_boot를 자동으로 복구·검증했습니다."
            );
            for warning in warnings {
                text.push('\n');
                text.push_str(&warning);
            }
            text
        }
        Err(error) => format!(
            "{ROLLBACK_CANCELLED_MESSAGE}\n원본 구성 자동 복구를 완료하지 못했습니다: {error}\n백업과 작업 기록을 보존했습니다. 기기를 EDL로 연결해 원본 복구를 진행하세요."
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
        return Err("TWRP 준비 완료 기록이 없습니다. 원본 구성 복구가 필요합니다.".into());
    }
    state.verify_binding(backup, firmware.as_ref())?;
    let loader = state.loader_path().to_path_buf();
    let mut phase = state.recovery.phase;
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
        ui.phase(Phase::EdlRequested);
        ui.confirm(Transition::Edl)?;
        stopped(ui)?;
        // Confirmation can be open for minutes. Re-read, never assume old bytes.
        // Only the verified-result path is reachable here, so no new prompt.
        if transact(&mut recovery, backup, &mut firmware, state, store, ui, log)?
            == Outcome::Cancelled
        {
            return Err(
                "DeviceInfo 검증 후 예상하지 않은 취소 결과입니다. 작업 기록을 보존했습니다."
                    .into(),
            );
        }
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
            &loader,
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
    let Some(firmware) = firmware else {
        // DeviceInfo edit without a region change: nothing is installed.
        ui.phase(Phase::FinalReboot);
        ui.status(
            "원본 GPT·Recovery를 복원했습니다. 펌웨어 설치 없이 시스템 부팅을 요청합니다.",
            false,
        );
        checkpoint(state, store, Phase::FinalReboot)?;
        if let Some(warning) = device.final_reboot(log) {
            state.warnings.push(warning);
        }
        checkpoint(state, store, Phase::Done)?;
        live!(
            log,
            "DeviceInfo 검증·원본 구성 복원 완료. 펌웨어 설치와 데이터 초기화 없이 시스템 부팅을 요청했습니다."
        );
        return Ok(());
    };
    let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
    workflow::confirm_identity(device, backup, state, work.path(), log)?;
    backup.verify()?;
    restore::ensure_conversion_allowed(backup)?;
    state.verify_binding(backup, Some(&firmware))?;
    checkpoint(state, store, Phase::FirmwareWriting)?;
    ui.phase(Phase::FirmwareWriting);
    ui.status(
        "공식 펌웨어를 설치하고 사용자 데이터를 초기화합니다.",
        false,
    );
    device.flash_firmware(&firmware, true, log)?;
    ui.phase(Phase::FinalReboot);
    ui.status(
        "GPT·국가 코드·활성 슬롯을 확인한 뒤 시스템 부팅을 요청합니다.",
        false,
    );
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
    live!(
        log,
        "DeviceInfo 검증·원본 구성 복원·펌웨어 설치 완료. 시스템 부팅을 요청했습니다."
    );
    Ok(())
}
