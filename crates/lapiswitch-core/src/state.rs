//! Durable full-run state. A process-held file lock prevents concurrent runs;
//! atomic replacement preserves the last complete JSON on interruption.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backup::{self, TransactionStage, VerifiedBackup};
use crate::country;
use crate::firmware::FirmwareReport;
use crate::region::Region;

pub const LEGACY_CONVERSION_ENDED: &str = "커스텀 BOOT 방식의 변환·이어하기 지원을 종료했습니다. 새 변환은 TWRP/DeviceInfo 방식을 사용하세요. 기존 백업의 원본 복구는 계속 사용할 수 있습니다.";
use crate::rollback::RollbackIndices;
use crate::slot::Slot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Stage {
    S1,
    S3,
    S4,
    S4Done,
    S5Resetting,
    S5,
    S5Done,
    S6,
    FinalReboot,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeAction {
    Restart,
    Recovery,
    ResetUncertain,
    ResetThenWait,
    Wait,
    Flash,
    Finalize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashOptions {
    pub wipe: bool,
    pub country: Option<String>,
    #[serde(default)]
    pub rollback_mode: crate::deviceinfo::RollbackMode,
}

impl FlashOptions {
    pub fn new(wipe: bool, country: Option<&str>) -> Result<Self, String> {
        if !wipe {
            return Err("리전 변환은 사용자 데이터 초기화가 필수입니다. 이전 데이터 유지 작업은 원본 복구 후 다시 시작해 주세요.".into());
        }
        let country = country.map(country::validate).transpose()?;
        Ok(Self {
            wipe,
            country,
            rollback_mode: crate::deviceinfo::RollbackMode::Keep,
        })
    }
    pub fn require_wipe(&self) -> Result<(), String> {
        Self::new(self.wipe, self.country.as_deref()).map(|_| ())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedFile {
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
}

/// Seal every existing referenced payload as well as XML, required AVB images
/// and loader. Missing optional images remain pinned as missing too.
pub fn seal_firmware(firmware: &FirmwareReport) -> Result<Vec<SealedFile>, String> {
    let mut paths = BTreeSet::from([firmware.loader.clone()]);
    for name in [
        "boot.img",
        "vbmeta_system.img",
        "vbmeta.img",
        "vendor_boot.img",
    ] {
        paths.insert(firmware.image_dir.join(name));
    }
    paths.extend(firmware.raw_xmls.iter().cloned());
    paths.extend(firmware.patch_xmls.iter().cloned());
    for xml in &firmware.raw_xmls {
        let text = std::fs::read_to_string(xml).map_err(|error| error.to_string())?;
        let doc = roxmltree::Document::parse(&text).map_err(|error| error.to_string())?;
        for node in doc
            .descendants()
            .filter(|node| node.has_tag_name("program"))
        {
            let Some(filename) = node
                .attribute("filename")
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let path = ltbox_core::safe_path::safe_join(
                xml.parent().ok_or("XML 부모 폴더 없음")?,
                filename,
            )
            .map_err(|error| error.to_string())?;
            paths.insert(path);
        }
    }
    let mut sealed = paths
        .into_iter()
        .map(|path| {
            if !path.exists() {
                let absolute = std::path::absolute(&path).map_err(|error| error.to_string())?;
                let ancestor = absolute
                    .ancestors()
                    .find(|candidate| candidate.exists())
                    .ok_or("경로의 상위 폴더 없음")?;
                let normalized = ancestor
                    .canonicalize()
                    .map_err(|error| error.to_string())?
                    .join(
                        absolute
                            .strip_prefix(ancestor)
                            .map_err(|error| error.to_string())?,
                    );
                return Ok(SealedFile {
                    path: normalized,
                    size: 0,
                    sha256: "MISSING".into(),
                });
            }
            let (size, sha256) = backup::hash_file(&path, None)?;
            Ok(SealedFile {
                path: path.canonicalize().map_err(|error| error.to_string())?,
                size,
                sha256,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    sealed.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(sealed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunState {
    pub version: u8,
    #[serde(default)]
    pub gpt_policy: crate::gpt::GptPolicy,
    #[serde(default)]
    pub recovery: Option<crate::recovery_flow::Progress>,
    pub run_id: String,
    pub stage: Stage,
    pub target_region: Region,
    pub firmware_dir: PathBuf,
    pub fingerprint: String,
    pub indices: RollbackIndices,
    pub files: Vec<SealedFile>,
    pub options: FlashOptions,
    pub backup_dir: PathBuf,
    pub backup_sha256: Option<String>,
    pub slot: Option<Slot>,
    pub persist_fingerprint: Option<String>,
    pub warnings: Vec<String>,
}

impl RunState {
    pub fn new(
        firmware: &FirmwareReport,
        options: FlashOptions,
        backup_dir: &Path,
    ) -> Result<Self, String> {
        options.require_wipe()?;
        Ok(Self {
            version: 1,
            gpt_policy: crate::gpt::CURRENT_POLICY,
            recovery: None,
            run_id: format!(
                "{}-{}",
                chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
                std::process::id()
            ),
            stage: Stage::S1,
            target_region: firmware.region,
            firmware_dir: firmware
                .image_dir
                .canonicalize()
                .map_err(|error| error.to_string())?,
            fingerprint: firmware.fingerprint.clone(),
            indices: firmware.indices,
            files: seal_firmware(firmware)?,
            options,
            backup_dir: backup_dir
                .canonicalize()
                .map_err(|error| error.to_string())?,
            backup_sha256: None,
            slot: None,
            persist_fingerprint: None,
            warnings: vec![],
        })
    }

    pub fn bind_backup(&mut self, backup: &VerifiedBackup) -> Result<(), String> {
        if self.recovery.is_some() != (backup.manifest().version >= 3)
            || self.gpt_policy != crate::gpt::CURRENT_POLICY
            || backup.manifest().gpt_policy != self.gpt_policy
            || self.backup_dir
                != backup
                    .path()
                    .canonicalize()
                    .map_err(|error| error.to_string())?
            || backup.manifest().target_region != self.target_region
            || backup.manifest().firmware_indices != self.indices
        {
            return Err("백업과 실행 설정이 다릅니다.".into());
        }
        self.backup_sha256 =
            Some(backup::hash_file(&backup.path().join(backup::MANIFEST_NAME), None)?.1);
        self.slot = Some(backup.manifest().slot);
        Ok(())
    }

    pub fn verify_binding(
        &self,
        backup: &VerifiedBackup,
        firmware: &FirmwareReport,
    ) -> Result<(), String> {
        self.options.require_wipe()?;
        if self.recovery.is_some() != (backup.manifest().version >= 3)
            || self.gpt_policy != crate::gpt::CURRENT_POLICY
            || backup.manifest().gpt_policy != self.gpt_policy
            || self.target_region != firmware.region
            || self.indices != firmware.indices
            || self.fingerprint != firmware.fingerprint
            || self.backup_dir
                != backup
                    .path()
                    .canonicalize()
                    .map_err(|error| error.to_string())?
            || self.backup_sha256.as_deref()
                != Some(&backup::hash_file(&backup.path().join(backup::MANIFEST_NAME), None)?.1)
            || self.slot != Some(backup.manifest().slot)
        {
            return Err("저장된 상태·펌웨어·백업이 일치하지 않습니다.".into());
        }
        if seal_firmware(firmware)? != self.files {
            return Err("펌웨어 파일 또는 XML 구성이 변경되었습니다.".into());
        }
        Ok(())
    }

    pub fn resume_action(&self, transaction: Option<TransactionStage>) -> ResumeAction {
        if self.recovery.is_some() {
            return crate::recovery_flow::resume_action(self, transaction);
        }
        if matches!(
            transaction,
            Some(
                TransactionStage::GptWriting
                    | TransactionStage::BootWriting
                    | TransactionStage::RecoveryRequired
            )
        ) {
            return ResumeAction::Recovery;
        }
        if transaction == Some(TransactionStage::Restored) {
            return ResumeAction::Restart;
        }
        // Earlier policies removed different entries, including fixed pvmfw_a
        // on active slot b. Never resume under another policy. Final
        // reboot handoff and already completed/pre-write runs remain usable.
        if self.gpt_policy != crate::gpt::CURRENT_POLICY
            && (matches!(
                self.stage,
                Stage::S4Done | Stage::S5Resetting | Stage::S5 | Stage::S5Done | Stage::S6
            ) || (self.stage == Stage::S4
                && transaction != Some(TransactionStage::BackupVerified))
                || (matches!(self.stage, Stage::S1 | Stage::S3)
                    && transaction == Some(TransactionStage::S4Done)))
        {
            return ResumeAction::Recovery;
        }
        if matches!(self.stage, Stage::S1 | Stage::S3)
            && transaction == Some(TransactionStage::S4Done)
        {
            return ResumeAction::Recovery;
        }
        if matches!(
            self.stage,
            Stage::S4Done
                | Stage::S5Resetting
                | Stage::S5
                | Stage::S5Done
                | Stage::S6
                | Stage::FinalReboot
        ) && transaction != Some(TransactionStage::S4Done)
        {
            return ResumeAction::Recovery;
        }
        let action = match self.stage {
            Stage::S1 | Stage::S3 | Stage::Done => ResumeAction::Restart,
            Stage::S4 if transaction == Some(TransactionStage::S4Done) => {
                ResumeAction::ResetThenWait
            }
            // GptWriting is journaled (synced) before the first write, so a
            // backup still at BackupVerified proves S4 never wrote anything.
            Stage::S4 if transaction == Some(TransactionStage::BackupVerified) => {
                ResumeAction::Restart
            }
            Stage::S4 => ResumeAction::Recovery,
            Stage::S4Done => ResumeAction::ResetThenWait,
            Stage::S5Resetting => ResumeAction::ResetUncertain,
            Stage::S5 => ResumeAction::Wait,
            Stage::S5Done | Stage::S6 => ResumeAction::Flash,
            Stage::FinalReboot => ResumeAction::Finalize,
        };
        // Read legacy keep-data states for status/recovery, never silently
        // change consent or resume a conversion under different erase rules.
        if !self.options.wipe
            && matches!(
                action,
                ResumeAction::ResetThenWait
                    | ResumeAction::ResetUncertain
                    | ResumeAction::Wait
                    | ResumeAction::Flash
            )
        {
            ResumeAction::Recovery
        } else {
            action
        }
    }
}

pub struct StateStore {
    path: PathBuf,
}
pub struct RunLock {
    _file: File,
}

impl StateStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub fn default_path() -> Result<PathBuf, String> {
        let base = if cfg!(windows) {
            dirs::data_local_dir()
        } else {
            dirs::data_dir()
        }
        .ok_or("앱 데이터 폴더를 찾을 수 없습니다.")?;
        Ok(base.join("tb520-region-tool").join("state.json"))
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn lock(&self) -> Result<RunLock, String> {
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let name = self
            .path
            .file_name()
            .ok_or("상태 파일 이름 없음")?
            .to_string_lossy();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(parent.join(format!("{name}.lock")))
            .map_err(|error| error.to_string())?;
        file.try_lock()
            .map_err(|error| format!("다른 실행이 이 상태 파일을 사용 중입니다: {error}"))?;
        Ok(RunLock { _file: file })
    }
    pub fn load(&self) -> Result<Option<RunState>, String> {
        let metadata = match std::fs::symlink_metadata(&self.path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
            return Err("잘못된 상태 파일".into());
        }
        let state: RunState = backup::read_json(&self.path)?;
        if !matches!(
            (state.version, state.recovery.is_some()),
            (1, false) | (2, true)
        ) {
            return Err("지원하지 않는 상태 파일 버전".into());
        }
        // Older keep-data records remain readable so recovery cannot lose its
        // binding to an interrupted run. Execution rejects them separately.
        if !state.options.wipe && state.options.country.is_some() {
            return Err("이전 데이터 유지 작업에 잘못된 국가 코드 설정이 있습니다.".into());
        }
        if let Some(code) = state.options.country.as_deref() {
            country::validate(code)?;
        }
        Ok(Some(state))
    }
    pub fn save(&self, state: &RunState) -> Result<(), String> {
        backup::write_json_atomic(&self.path, state)
    }
    /// Caller holds this store's run lock. Only a verified RESTORED journal
    /// for the active state's own backup permits removing the active pointer.
    /// Preserve its exact JSON first so recovery history is never discarded.
    pub fn archive_restored(&self, backup: &VerifiedBackup) -> Result<Option<PathBuf>, String> {
        if backup.transaction_stage()? != TransactionStage::Restored {
            return Err("원본 복구 완료가 확인되지 않아 작업 기록을 보존합니다.".into());
        }
        let Some(state) = self.load()? else {
            return Ok(None);
        };
        if state
            .backup_dir
            .canonicalize()
            .map_err(|error| error.to_string())?
            != backup
                .path()
                .canonicalize()
                .map_err(|error| error.to_string())?
        {
            return Ok(None);
        }
        let history = self
            .path
            .parent()
            .ok_or("작업 기록 폴더 없음")?
            .join("history");
        std::fs::create_dir_all(&history).map_err(|error| error.to_string())?;
        let original = std::fs::read(&self.path).map_err(|error| error.to_string())?;
        let mut archive = tempfile::Builder::new()
            .prefix("state-restored-")
            .suffix(".json")
            .tempfile_in(history)
            .map_err(|error| error.to_string())?;
        archive
            .write_all(&original)
            .and_then(|_| archive.as_file().sync_all())
            .map_err(|error| error.to_string())?;
        let (_, archived_path) = archive.keep().map_err(|error| error.to_string())?;
        std::fs::remove_file(&self.path).map_err(|error| error.to_string())?;
        Ok(Some(archived_path))
    }
    /// Explicit user action, independent of device/backup availability. Never
    /// label this as a verified restore or change the backup's write journal.
    /// Take the same lock as device workflows and reject a stale confirmation.
    pub fn archive_for_new_run(&self, expected: &RunState) -> Result<PathBuf, String> {
        let _held = self.lock()?;
        let current = self.load()?.ok_or("보관할 이전 작업 기록이 없습니다.")?;
        if serde_json::to_value(&current).map_err(|e| e.to_string())?
            != serde_json::to_value(expected).map_err(|e| e.to_string())?
        {
            return Err("이전 작업 기록이 변경되었습니다. 작업 확인을 다시 열어 주세요.".into());
        }
        let original = std::fs::read(&self.path).map_err(|e| e.to_string())?;
        let history = self
            .path
            .parent()
            .ok_or("작업 기록 폴더 없음")?
            .join("history");
        std::fs::create_dir_all(&history).map_err(|e| e.to_string())?;
        let archive = tempfile::Builder::new()
            .prefix("state-user-archived-")
            .tempdir_in(history)
            .map_err(|e| e.to_string())?;
        backup::write_new(&archive.path().join("state.json"), &original)?;
        let metadata = serde_json::json!({
            "version": 1,
            "reason": "USER_REQUESTED_NEW_RUN",
            "recorded_at": chrono::Utc::now().to_rfc3339(),
            "device_restoration_verified": false,
            "state_sha256": backup::digest_bytes(&original),
        });
        backup::write_new(
            &archive.path().join("archive.json"),
            &serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
        )?;
        if std::fs::read(archive.path().join("state.json")).map_err(|e| e.to_string())? != original
        {
            return Err("작업 기록 보관 검증에 실패했습니다. 이전 기록을 유지합니다.".into());
        }
        // Keep the durable history before touching the active pointer. A failed
        // remove leaves the active guard in place and the archive recoverable.
        let archived = archive.keep();
        if std::fs::read(&self.path).map_err(|e| e.to_string())? != original {
            return Err("이전 작업 기록이 변경되었습니다. 활성 기록을 유지합니다.".into());
        }
        std::fs::remove_file(&self.path).map_err(|e| e.to_string())?;
        Ok(archived)
    }

    pub fn begin(&self, state: &RunState) -> Result<(), String> {
        state.options.require_wipe()?;
        if let Some(previous) = self.load()? {
            let transaction = backup::transaction_stage_in(&previous.backup_dir)?;
            let restartable = previous.resume_action(transaction) == ResumeAction::Restart;
            if !restartable {
                return Err(
                    "미완료 작업이 있습니다. status/resume 또는 restore로 확인해 주세요.".into(),
                );
            }
        }
        self.save(state)
    }
}
