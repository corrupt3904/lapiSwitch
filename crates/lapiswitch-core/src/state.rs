//! Durable full-run state. A process-held file lock prevents concurrent runs;
//! atomic replacement preserves the last complete JSON on interruption.

use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backup::{self, TransactionStage, VerifiedBackup};
use crate::country;
use crate::device::Device;
use crate::deviceinfo::RollbackMode;
use crate::firmware::FirmwareReport;
use crate::recovery_flow::Progress;
use crate::region::Region;
use crate::rollback::RollbackIndices;
use crate::slot::Slot;

/// The only state format this version reads or writes.
pub const STATE_VERSION: u8 = 5;

/// Coarse step mirrored from the recovery `Phase` for status display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Stage {
    S1,
    S3,
    S4,
    S4Done,
    S5,
    S6,
    FinalReboot,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeAction {
    Restart,
    Recovery,
    Wait,
    Flash,
    Finalize,
}

/// What the run changes in DeviceInfo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum Request {
    /// Region change chosen before the run, with the target firmware.
    Convert {
        target: Region,
        rollback_mode: RollbackMode,
    },
    /// Fields chosen in TWRP after reading the live DeviceInfo. Firmware is
    /// required (and bound then) only when the region changes.
    Edit,
    /// DeviceInfo is read in TWRP and saved under `dump_root`; nothing is
    /// written and the original configuration is restored right after.
    Dump,
}

/// Options of the firmware installation. Installing firmware always erases
/// user data; a DeviceInfo edit without a region change installs nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashOptions {
    pub country: Option<String>,
}

impl FlashOptions {
    pub fn new(country: Option<&str>) -> Result<Self, String> {
        Ok(Self {
            country: country.map(country::validate_choice).transpose()?,
        })
    }
}

/// The firmware a run installs, pinned by content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirmwareBinding {
    pub region: Region,
    pub firmware_dir: PathBuf,
    pub fingerprint: String,
    pub indices: RollbackIndices,
    pub files: Vec<SealedFile>,
}

impl FirmwareBinding {
    pub fn new(firmware: &FirmwareReport) -> Result<Self, String> {
        Ok(Self {
            region: firmware.region,
            firmware_dir: firmware
                .image_dir
                .canonicalize()
                .map_err(|error| error.to_string())?,
            fingerprint: firmware.fingerprint.clone(),
            indices: firmware.indices,
            files: seal_firmware(firmware)?,
        })
    }
    /// Same package by its identity only (region, rollback indices,
    /// fingerprint); no file is read.
    pub fn matches(&self, firmware: &FirmwareReport) -> Result<(), String> {
        if self.region != firmware.region
            || self.indices != firmware.indices
            || self.fingerprint != firmware.fingerprint
        {
            return Err("저장된 펌웨어와 현재 펌웨어가 일치하지 않습니다.".into());
        }
        Ok(())
    }
    /// Every pinned payload, XML and the loader by size and SHA-256. This
    /// reads the whole package (several GB), so it runs only when a binding
    /// is resumed and immediately before flashing.
    pub fn verify(&self, firmware: &FirmwareReport) -> Result<(), String> {
        self.matches(firmware)?;
        if seal_firmware(firmware)? != self.files {
            return Err("펌웨어 파일 또는 XML 구성이 변경되었습니다.".into());
        }
        Ok(())
    }
}

/// Seal one existing file by canonical path, size and SHA-256.
pub fn seal_file(path: &Path) -> Result<SealedFile, String> {
    let (size, sha256) = backup::hash_file(path, None)?;
    Ok(SealedFile {
        path: path.canonicalize().map_err(|error| error.to_string())?,
        size,
        sha256,
    })
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
    pub recovery: Progress,
    pub run_id: String,
    pub stage: Stage,
    pub request: Request,
    /// Known from the start for a conversion (its firmware) or an ADB edit;
    /// an edit started in EDL learns it at S1 and binds it with the backup.
    pub device: Option<Device>,
    /// The EDL programmer every session of this run uploads.
    pub loader: SealedFile,
    /// Present from the start for a conversion; bound in TWRP when an edit
    /// changes the region. None means no firmware is installed.
    pub firmware: Option<FirmwareBinding>,
    pub options: FlashOptions,
    pub backup_dir: PathBuf,
    pub backup_sha256: Option<String>,
    pub slot: Option<Slot>,
    pub persist_fingerprint: Option<String>,
    pub warnings: Vec<String>,
    /// Folder a dump is saved under, in its own `Deviceinfo_dump_*` folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dump_root: Option<PathBuf>,
}

impl RunState {
    /// A region conversion with its firmware chosen and checked up front.
    pub fn convert(
        firmware: &FirmwareReport,
        rollback_mode: RollbackMode,
        options: FlashOptions,
        backup_dir: &Path,
    ) -> Result<Self, String> {
        Self::build(
            Request::Convert {
                target: firmware.region,
                rollback_mode,
            },
            Some(firmware.device),
            &firmware.loader,
            Some(FirmwareBinding::new(firmware)?),
            options,
            backup_dir,
        )
    }

    /// A DeviceInfo edit: only the EDL loader, and the device when ADB
    /// reported it, are known before TWRP.
    pub fn edit(device: Option<Device>, loader: &Path, backup_dir: &Path) -> Result<Self, String> {
        Self::build(
            Request::Edit,
            device,
            loader,
            None,
            FlashOptions::new(None)?,
            backup_dir,
        )
    }

    /// A DeviceInfo dump into `dump_root`; works without a field map.
    pub fn dump(
        device: Option<Device>,
        loader: &Path,
        backup_dir: &Path,
        dump_root: &Path,
    ) -> Result<Self, String> {
        let mut state = Self::build(
            Request::Dump,
            device,
            loader,
            None,
            FlashOptions::new(None)?,
            backup_dir,
        )?;
        state.dump_root = Some(dump_root.to_path_buf());
        Ok(state)
    }

    fn build(
        request: Request,
        device: Option<Device>,
        loader: &Path,
        firmware: Option<FirmwareBinding>,
        options: FlashOptions,
        backup_dir: &Path,
    ) -> Result<Self, String> {
        Ok(Self {
            version: STATE_VERSION,
            recovery: Progress::default(),
            run_id: format!(
                "{}-{}",
                chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
                std::process::id()
            ),
            stage: Stage::S1,
            request,
            device,
            loader: seal_file(loader)?,
            firmware,
            options,
            backup_dir: backup_dir
                .canonicalize()
                .map_err(|error| error.to_string())?,
            backup_sha256: None,
            slot: None,
            persist_fingerprint: None,
            warnings: vec![],
            dump_root: None,
        })
    }

    pub fn loader_path(&self) -> &Path {
        &self.loader.path
    }

    /// The device of this run, bound with the backup at S3 at the latest.
    pub fn device(&self) -> Result<Device, String> {
        self.device
            .ok_or_else(|| "저장된 작업에 기기 정보가 없습니다.".into())
    }

    pub fn bind_backup(&mut self, backup: &VerifiedBackup) -> Result<(), String> {
        if self.backup_dir
            != backup
                .path()
                .canonicalize()
                .map_err(|error| error.to_string())?
        {
            return Err("백업과 실행 설정이 다릅니다.".into());
        }
        if self.device.is_some_and(|device| device != backup.device()) {
            return Err("백업의 기기가 작업의 기기와 다릅니다.".into());
        }
        self.device = Some(backup.device());
        self.backup_sha256 =
            Some(backup::hash_file(&backup.path().join(backup::MANIFEST_NAME), None)?.1);
        self.slot = Some(backup.manifest().slot);
        Ok(())
    }

    /// The backup, loader and (when bound) firmware are exactly the ones this
    /// run started with, including every firmware payload's hash. Use on
    /// resume and right before flashing.
    pub fn verify_binding(
        &self,
        backup: &VerifiedBackup,
        firmware: Option<&FirmwareReport>,
    ) -> Result<(), String> {
        self.check_binding(backup, firmware)?;
        match (&self.firmware, firmware) {
            (Some(binding), Some(firmware)) => binding.verify(firmware),
            _ => Ok(()),
        }
    }

    /// As `verify_binding`, but the firmware only by identity: for the steps
    /// between binding and flashing, which use its region and indices only.
    pub fn check_binding(
        &self,
        backup: &VerifiedBackup,
        firmware: Option<&FirmwareReport>,
    ) -> Result<(), String> {
        if self.backup_dir
            != backup
                .path()
                .canonicalize()
                .map_err(|error| error.to_string())?
            || self.backup_sha256.as_deref()
                != Some(&backup::hash_file(&backup.path().join(backup::MANIFEST_NAME), None)?.1)
            || self.slot != Some(backup.manifest().slot)
            || self.device != Some(backup.device())
            || firmware.is_some_and(|firmware| self.device != Some(firmware.device))
        {
            return Err("저장된 상태·백업이 일치하지 않습니다.".into());
        }
        if seal_file(&self.loader.path)? != self.loader {
            return Err("EDL 로더 파일이 변경되었습니다.".into());
        }
        match (&self.firmware, firmware) {
            (Some(binding), Some(firmware)) => binding.matches(firmware),
            (None, None) => Ok(()),
            _ => Err("저장된 작업의 펌웨어 설치 여부와 선택한 펌웨어가 다릅니다.".into()),
        }
    }

    pub fn resume_action(&self, transaction: Option<TransactionStage>) -> ResumeAction {
        crate::recovery_flow::resume_action(self, transaction)
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
        Ok(base.join("lapiSwitch").join("state.json"))
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
        if state.version != STATE_VERSION {
            return Err("지원하지 않는 상태 파일 버전".into());
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
