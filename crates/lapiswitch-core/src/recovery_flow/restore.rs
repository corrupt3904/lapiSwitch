//! Original restoration is a separate, durable reverse transaction. Classify
//! conversion evidence on the PC before connecting to or writing any device.
use super::*;
use crate::{region::Region, vendor_boot};

pub const UNKNOWN_MESSAGE: &str = "DeviceInfo 쓰기 결과를 확인할 수 없어 원본 복구를 진행할 수 없습니다. 리전 불일치로 부팅되지 않을 수 있습니다. 기존 백업과 작업 기록은 보존됩니다.";
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
            Self::BeforeWrite => "쓰기 전".into(),
            Self::Verified { changed: false, .. } => "변경 검증 완료 · 변경 없음".into(),
            Self::Verified {
                original, current, ..
            } => format!(
                "변경 검증 완료 · 리전 {} → {}",
                token(original),
                token(current)
            ),
            Self::Unknown(_) => "결과 불명".into(),
        }
    }
    /// The original DeviceInfo must be written back through TWRP.
    pub fn needs_twrp(&self) -> bool {
        matches!(self, Self::Verified { changed: true, .. })
    }
}
fn evidence_json<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, String> {
    if backup::hash_file(p, None)?.0 > 65536 {
        return Err("작업 기록 크기 오류".into());
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
    let a = p.attempt.as_ref().ok_or("DeviceInfo 쓰기 의도 누락")?;
    let dir = attempt_dir(backup, &a.directory)?;
    let before_path = dir.join("before_shared_1.bin");
    if backup::hash_file(&before_path, None)?
        != (deviceinfo::SHARED_SIZE as u64, a.before_sha256.clone())
    {
        return Err("DeviceInfo 원본 백업 해시 불일치".into());
    }
    let before = std::fs::read(before_path).map_err(|e| e.to_string())?;
    if backup.device().field_map()?.region(&before) != a.original_region {
        return Err("원래 리전 기록 불일치".into());
    }
    let planned = map::apply(backup.device().field_map()?, &before, &a.edits)?;
    if backup::digest_bytes(&planned) != a.planned_sha256 {
        return Err("DeviceInfo 예정 값 해시 불일치".into());
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
                return Err("DeviceInfo 폴더가 있지만 작업 기록이 없습니다.".into());
            }
        }
        if backup.transaction_stage()? == TransactionStage::BackupVerified {
            return Ok(Assessment::BeforeWrite);
        }
        return Err("DeviceInfo 작업 기록이 누락되었습니다.".into());
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
            return Err("예정 작업의 원본 기록 누락".into());
        }
        if let Some(a) = &p.attempt {
            proof(backup, p)?;
            if attempt_dir(backup, &a.directory)?
                .join("after_independent.bin")
                .try_exists()
                .map_err(|e| e.to_string())?
            {
                return Err("쓰기 전 단계와 최종 재읽기 파일이 충돌합니다.".into());
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
                return Err("쓰기 전 기록에 최종 재읽기 파일이 남아 있습니다.".into());
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
        return Err("최종 DeviceInfo 재읽기 증거 누락/변조".into());
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
                return Err("공식 펌웨어 설치가 시작된 백업입니다. 과거 구성 원본 복구 대신 설치를 이어가세요.".into());
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
            return Err("원본 복구 폴더는 링크일 수 없습니다.".into());
        }
    }
    if !meta.file_type().is_dir() {
        return Err("원본 복구 폴더 오류".into());
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
            return Err(format!(
                "{UNKNOWN_MESSAGE}\n원본 복구 파일이 있지만 복구 기록이 없습니다."
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
        return Err("원본 복구 기록과 백업이 일치하지 않습니다.".into());
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
    let before_hash = j.before_sha256.as_ref().ok_or("복구 직전 원본 해시 누락")?;
    if backup::hash_file(&before_path, None)? != (4096, before_hash.clone()) {
        return Err("복구 직전 DeviceInfo 백업 변조".into());
    }
    let before = std::fs::read(before_path).map_err(|e| e.to_string())?;
    let p = progress(backup)?.ok_or("변환 기록 누락")?;
    let (original, converted) = proof(backup, &p)?;
    if before != converted {
        return Err("복구 직전 DeviceInfo와 검증된 변환 결과가 다릅니다.".into());
    }
    let planned = original;
    let expected = j.planned_sha256.as_ref().ok_or("복구 예정 해시 누락")?;
    if backup::digest_bytes(&planned) != *expected {
        return Err("원본 DeviceInfo 복구 계획 불일치".into());
    }
    let after_path = dir.join("deviceinfo_after.bin");
    if !after_path.try_exists().map_err(|e| e.to_string())? {
        return Ok(false);
    }
    if backup::hash_file(&after_path, None)? != (4096, expected.clone()) {
        return Err("원본 DeviceInfo 최종 재읽기 증거 변조".into());
    }
    let after = std::fs::read(after_path).map_err(|e| e.to_string())?;
    deviceinfo::verify(&planned, &after, &after)?;
    Ok(true)
}
/// Preflight for both backup selection and execution. No USB or DeviceInfo read.
pub fn preflight(backup: &VerifiedBackup) -> Result<Assessment, String> {
    let state = assess(backup)?;
    if let Assessment::Unknown(reason) = &state {
        return Err(format!("{UNKNOWN_MESSAGE}\n{reason}"));
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
            return Err("원본 vendor_boot DTB와 원래 DeviceInfo 리전이 다릅니다.".into());
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
                return Err(format!(
                    "{UNKNOWN_MESSAGE}\n원본 DeviceInfo 복구 쓰기의 최종 검증 증거가 없습니다."
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
        return Err("원본 복구를 시작한 백업입니다. 변환 대신 원본 복구를 완료하세요.".into());
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
        Err("DeviceInfo가 변경되지 않은 원본 복구에는 TWRP 연결이 필요하지 않습니다.".into())
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
            return Err("기존 원본 복구 파일이 변경되었습니다.".into());
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
        return Err("기기의 vendor_boot가 원본/검증된 임시 이미지와 다릅니다.".into());
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
    if let Assessment::Verified {
        current,
        changed: true,
        ..
    } = assessment
        && !restore_proof(backup, &j)?
    {
        let current = current.ok_or("변경된 DeviceInfo의 리전이 PRC/ROW가 아닙니다.")?;
        ui.status(
            "확인된 리전에 맞춰 vendor_boot DTB와 TWRP를 준비합니다.",
            false,
        );
        bootstrap(
            device,
            backup,
            twrp.ok_or("공식 TWRP 파일이 필요합니다.")?,
            current,
            log,
        )?;
        save(&dir, &mut j, RestorePhase::BootRequested)?;
        device.reboot_recovery(log)?;
        ui.phase(Phase::RecoveryWaiting);
        ui.status("원래 DeviceInfo를 복구할 TWRP ADB 연결을 기다립니다.", true);
        let mut recovery = connector.connect(backup, ui, log)?;
        recovery.verify_identity(backup)?;
        let scratch = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
        let before = recovery.read(&scratch.path().join("read1"), log)?;
        let second = recovery.read(&scratch.path().join("read2"), log)?;
        let p = progress(backup)?.ok_or("변환 기록 누락")?;
        let (planned, expected) = proof(backup, &p)?;
        deviceinfo::verify(&expected, &before, &second)?;
        preserve(&dir.join("deviceinfo_before.bin"), &before)?;
        j.before_sha256 = Some(backup::digest_bytes(&before));
        j.planned_sha256 = Some(backup::digest_bytes(&planned));
        // Transfers and the dry-run precede the durable write intent.
        recovery.prepare(&before, &planned, log)?;
        save(&dir, &mut j, RestorePhase::RegionWritePending)?;
        ui.phase(Phase::Planned);
        ui.status(
            "변환 전 DeviceInfo 4096바이트 전체를 복구하고 검증합니다.",
            false,
        );
        let (same, fresh) =
            recovery.apply(&before, &planned, &scratch.path().join("apply"), log)?;
        deviceinfo::verify(&planned, &same, &fresh)?;
        let independent = recovery.read(&scratch.path().join("independent"), log)?;
        deviceinfo::verify(&planned, &independent, &independent)?;
        preserve(&dir.join("deviceinfo_after.bin"), &independent)?;
        save(&dir, &mut j, RestorePhase::RegionVerified)?;
        recovery.verify_identity(backup)?;
        if let Err(e) = recovery.reboot_edl() {
            live!(log, "원본 복구 EDL 재부팅 응답: {e}. 연결을 확인합니다.");
        }
        drop(recovery);
        super::wait_for_edl(device, loader, clock, ui, log)?;
        if !ui.finish_wait() {
            return Err("연결 대기를 중지했습니다. 원본 복구 검증 기록을 보존했습니다.".into());
        }
    }
    ui.phase(Phase::Restoring);
    ui.status(
        "원본 vendor_boot·Recovery·GPT를 복원하고 재읽기 검증합니다.",
        false,
    );
    // Recheck the PC proof and binding after every wait, before final writes.
    preflight(backup)?;
    super::restore_originals(device, backup, log)?;
    save(&dir, &mut j, RestorePhase::Complete)?;
    backup.mark(TransactionStage::Restored)?;
    live!(
        log,
        "원본 구성 복구·검증 완료. DeviceInfo는 변환 전 값이며 misc 덤프는 보존했습니다."
    );
    let mut warnings = vec![];
    if let Err(e) = device.reset(crate::stage1::ResetMode::System, log) {
        warnings.push(format!("시스템 부팅 요청 실패: {e}"));
    }
    Ok(warnings)
}
