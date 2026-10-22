//! LTBox backup naming and manifest fields, extended for verified restoration.
//! Unlike LTBox's descriptive manifest, every recorded hash is checked before
//! our S4 or restore operations. Device dumps must never enter source control.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::prepare::{Connection, PrepareReport};
use crate::slot::Slot;
use crate::storage::{self, Partition};

pub const MANIFEST_NAME: &str = "manifest.json";
/// ADB model/serial/slot record of an Android start, kept beside the manifest.
pub const IDENTITY_NAME: &str = "adb_identity.txt";
pub const PARTITION_DIR: &str = "Partition";
/// The only backup format this version writes, verifies or restores.
pub const MANIFEST_VERSION: u8 = 8;
pub const MANIFEST_OPERATION: &str = "recovery_deviceinfo_v6";

/// Keep image dumps and scratch files inside the backup, never through a
/// substituted directory link.
pub(crate) fn partition_directory(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join(PARTITION_DIR);
    match std::fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("{}: {error}", path.display())),
    }
    check_partition_directory(&path)?;
    Ok(path)
}

fn check_partition_directory(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Partition은 링크가 아닌 일반 폴더여야 합니다.".into());
        }
    }
    if !metadata.file_type().is_dir() {
        return Err("Partition은 링크가 아닌 일반 폴더여야 합니다.".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupFile {
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub fingerprint: Option<String>,
    pub rollback_index: Option<u64>,
    pub partition: Option<Partition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub version: u8,
    pub operation: String,
    pub model: String,
    pub fingerprint: String,
    pub recorded_at: String,
    pub slot: Slot,
    pub started_in_edl: bool,
    pub device_key: String,
    pub tool_version: String,
    pub snapshot: Vec<Partition>,
    pub absent_partitions: Vec<String>,
    pub files: Vec<BackupFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransactionStage {
    BackupVerified,
    GptWriting,
    BootWriting,
    S4Done,
    RecoveryRequired,
    Restored,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    version: u8,
    stage: TransactionStage,
    recorded_at: String,
}

/// Read the journal even when backup payloads are damaged. Decisions about
/// possible writes must not turn a hash failure into "no transaction".
pub fn transaction_stage_in(dir: &Path) -> Result<Option<TransactionStage>, String> {
    let path = dir.join("transaction.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.file_type().is_file() || metadata.len() > 4096 {
        return Err("작업 기록 파일이 잘못되었습니다.".into());
    }
    let state: Transaction = read_json(&path)?;
    if state.version != 1 {
        return Err("지원하지 않는 작업 기록 형식".into());
    }
    Ok(Some(state.stage))
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    // Close the Windows file handle before parsing (from_reader reads byte by
    // byte and can hold the old checkpoint across an atomic replacement).
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("부모 폴더 없음")?)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n")
        .and_then(|_| file.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    // Windows scanners/readers can briefly deny MoveFileEx replacement. Retry
    // the same flushed temporary file; never remove the last valid checkpoint.
    let mut delays = [25, 50, 100, 150, 250, 400, 500, 500].into_iter();
    loop {
        match file.persist(path) {
            Ok(_) => break,
            Err(error) => {
                let transient =
                    cfg!(windows) && matches!(error.error.raw_os_error(), Some(5 | 32 | 33));
                if let Some(delay) = delays.next().filter(|_| transient) {
                    file = error.file;
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                } else {
                    return Err(format!(
                        "JSON 파일 교체 실패 (기존 파일 보존): {}: {}",
                        path.display(),
                        error.error
                    ));
                }
            }
        }
    }
    #[cfg(unix)]
    File::open(path.parent().expect("parent checked"))
        .and_then(|dir| dir.sync_all())
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Hash only regular, non-symlink files. An optional limit verifies an image
/// prefix after sector-padded reads without accepting a truncated transfer.
pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn hash_file(path: &Path, limit: Option<u64>) -> Result<(u64, String), String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!("일반 파일이 아닙니다: {}", path.display()));
    }
    let size = limit.unwrap_or(metadata.len());
    if size > metadata.len() {
        return Err(format!("파일이 잘렸습니다: {}", path.display()));
    }
    let mut file = File::open(path)
        .map_err(|error| error.to_string())?
        .take(size);
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let len = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
        count += len as u64;
    }
    if count != size {
        return Err(format!("파일 읽기 길이가 다릅니다: {}", path.display()));
    }
    let hex = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((size, hex))
}

pub struct BackupBuilder {
    dir: PathBuf,
    files: Vec<BackupFile>,
}

impl BackupBuilder {
    pub fn create(output_root: &Path) -> Result<Self, String> {
        let parent = output_root.join("backup").join("region_change");
        std::fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
        let stem = format!(
            "{}_{}",
            crate::device::model(),
            chrono::Local::now().format("%Y-%m-%d_%H%M%S")
        );
        for attempt in 1..u32::MAX {
            let name = if attempt == 1 {
                stem.clone()
            } else {
                format!("{stem}_{attempt}")
            };
            let dir = parent.join(name);
            match std::fs::create_dir(&dir) {
                Ok(()) => {
                    partition_directory(&dir)?;
                    return Ok(Self { dir, files: vec![] });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("백업 폴더 이름을 만들 수 없습니다.".into())
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    pub fn partition_dir(&self) -> PathBuf {
        self.dir.join(PARTITION_DIR)
    }

    pub fn save_identity(&self, raw: &str) -> Result<(), String> {
        let path = self.dir.join(IDENTITY_NAME);
        if path.exists() {
            if std::fs::read(&path).map_err(|error| error.to_string())? != raw.as_bytes() {
                return Err(format!("기존 {IDENTITY_NAME}가 현재 작업과 다릅니다."));
            }
            return Ok(());
        }
        write_new(&path, raw.as_bytes())
    }

    pub(crate) fn record(
        &mut self,
        filename: &str,
        partition: Option<Partition>,
    ) -> Result<(), String> {
        if self.files.iter().any(|entry| entry.filename == filename) {
            return Err("백업 항목 중복".into());
        }
        let path = if filename == IDENTITY_NAME {
            self.dir.join(filename)
        } else {
            self.partition_dir().join(filename)
        };
        // The native dumper closes its file; explicitly sync before rereading.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .and_then(|file| file.sync_all())
            .map_err(|error| error.to_string())?;
        let (size_bytes, sha256) = hash_file(&path, None)?;
        if let Some(part) = &partition
            && size_bytes != part.size_bytes()?
        {
            return Err(format!("백업 길이가 파티션 크기와 다릅니다: {filename}"));
        }
        let avb = if path.extension().is_some_and(|extension| extension == "img") {
            crate::avb_info::read_avb(&path).ok()
        } else {
            None
        };
        self.files.push(BackupFile {
            filename: filename.into(),
            size_bytes,
            sha256,
            fingerprint: avb
                .as_ref()
                .and_then(crate::avb_info::AvbSummary::build_fingerprint),
            rollback_index: avb.as_ref().map(|info| info.rollback_index),
            partition,
        });
        Ok(())
    }

    pub(crate) fn finish(
        self,
        prepared: &PrepareReport,
        fingerprint: String,
        device_key: String,
        snapshot: Vec<Partition>,
        proinfo_absent: bool,
    ) -> Result<VerifiedBackup, String> {
        let manifest = BackupManifest {
            version: MANIFEST_VERSION,
            operation: MANIFEST_OPERATION.into(),
            model: crate::device::model().into(),
            fingerprint,
            recorded_at: chrono::Local::now().to_rfc3339(),
            slot: prepared.slot,
            started_in_edl: prepared.started_in == Connection::Edl,
            device_key,
            tool_version: env!("CARGO_PKG_VERSION").into(),
            snapshot,
            absent_partitions: if proinfo_absent {
                vec!["proinfo".into()]
            } else {
                vec![]
            },
            files: self.files,
        };
        write_json_atomic(&self.dir.join(MANIFEST_NAME), &manifest)?;
        let backup = VerifiedBackup::open(&self.dir)?;
        backup.mark(TransactionStage::BackupVerified)?;
        Ok(backup)
    }
}

/// Constructible only by loading and verifying the complete manifest.
pub struct VerifiedBackup {
    dir: PathBuf,
    manifest: BackupManifest,
}

impl VerifiedBackup {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let path = dir.join(MANIFEST_NAME);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
            return Err("잘못된 백업 manifest".into());
        }
        let manifest: BackupManifest = read_json(&path)?;
        let backup = Self {
            dir: dir.to_path_buf(),
            manifest,
        };
        backup.verify()?;
        Ok(backup)
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
    pub fn manifest(&self) -> &BackupManifest {
        &self.manifest
    }

    /// Resolve only a recorded basename: the identity record sits beside the
    /// manifest and every .img/.bin original in Partition. Never guess.
    pub fn file_path(&self, name: &str) -> Result<PathBuf, String> {
        self.entry(name)?;
        Ok(if name == IDENTITY_NAME {
            self.dir.join(name)
        } else {
            self.dir.join(PARTITION_DIR).join(name)
        })
    }

    pub(crate) fn work_dir(&self) -> Result<PathBuf, String> {
        partition_directory(&self.dir)
    }

    pub fn verify(&self) -> Result<(), String> {
        let manifest = &self.manifest;
        if manifest.version != MANIFEST_VERSION
            || manifest.operation != MANIFEST_OPERATION
            || manifest.model != crate::device::model()
        {
            return Err(
                "지원하지 않는 백업 형식 또는 모델입니다. 이 버전에서 만든 백업만 사용할 수 있습니다."
                    .into(),
            );
        }
        check_partition_directory(&self.dir.join(PARTITION_DIR))?;
        if manifest.absent_partitions != Vec::<String>::new()
            && manifest.absent_partitions != ["proinfo"]
        {
            return Err("허용하지 않는 누락 파티션 기록입니다.".into());
        }
        let mut required = HashSet::from([
            "gpt_main4.bin".to_owned(),
            "devinfo.img".into(),
            "persist.img".into(),
            format!("recovery{}.img", manifest.slot.suffix()),
            "misc.img".into(),
            format!("vendor_boot{}.img", manifest.slot.suffix()),
        ]);
        if manifest.absent_partitions.is_empty() {
            required.insert("proinfo.img".into());
        }
        if !manifest.started_in_edl {
            required.insert(IDENTITY_NAME.into());
        }
        let mut seen = HashSet::new();
        for entry in &manifest.files {
            if !required.contains(&entry.filename) || !seen.insert(entry.filename.clone()) {
                return Err(format!(
                    "예상하지 않은 백업 파일 또는 중복: {}",
                    entry.filename
                ));
            }
            if entry.filename == "gpt_main4.bin" && entry.size_bytes > 1024 * 1024 {
                return Err("GPT 백업이 허용 크기를 초과합니다.".into());
            }
            let (size, sha) = hash_file(&self.file_path(&entry.filename)?, None)?;
            if size == 0 || size != entry.size_bytes || sha != entry.sha256 {
                return Err(format!("백업 검증 실패: {}", entry.filename));
            }
            if let Some(part) = &entry.partition {
                if size != part.size_bytes()? {
                    return Err(format!("백업 좌표/길이 불일치: {}", entry.filename));
                }
                if entry.filename != format!("{}.img", part.name)
                    && entry.filename != "gpt_main4.bin"
                {
                    return Err("백업 파티션 이름 불일치".into());
                }
            } else if entry.filename != IDENTITY_NAME {
                return Err("백업 좌표 누락".into());
            }
        }
        if seen != required {
            return Err("필수 백업 파일이 누락되었습니다.".into());
        }
        let bytes =
            std::fs::read(self.file_path("gpt_main4.bin")?).map_err(|error| error.to_string())?;
        let gpt = storage::validate_gpt(&bytes)?;
        storage::disk_guid(&bytes)?;
        let geometry = storage::primary_geometry(&bytes)?;
        if self.entry("gpt_main4.bin")?.partition.as_ref() != Some(&geometry) {
            return Err("GPT 백업 좌표 불일치".into());
        }
        // The staged table must be derivable from this original.
        gpt.remove_region_vbmeta(manifest.slot)
            .map_err(|error| error.to_string())?;
        for (name, lun) in [
            ("devinfo".to_owned(), 4),
            ("persist".into(), 0),
            (format!("recovery{}", manifest.slot.suffix()), 4),
            ("misc".into(), 0),
            (format!("vendor_boot{}", manifest.slot.suffix()), 4),
        ] {
            let part = storage::find_partition(&manifest.snapshot, &name, lun)?;
            if self.entry(&format!("{name}.img"))?.partition.as_ref() != Some(part) {
                return Err(format!("백업 스냅샷 불일치: {name}"));
            }
            if lun == 4 {
                storage::match_gpt_partition(&gpt, part)?;
            }
        }
        if manifest.absent_partitions.is_empty() {
            let part = storage::find_partition(&manifest.snapshot, "proinfo", 0)?;
            if self.entry("proinfo.img")?.partition.as_ref() != Some(part) {
                return Err("proinfo 백업 좌표 불일치".into());
            }
        } else if manifest.snapshot.iter().any(|part| part.name == "proinfo") {
            return Err("proinfo 존재 여부가 모순됩니다.".into());
        }
        Ok(())
    }

    pub(crate) fn entry(&self, name: &str) -> Result<&BackupFile, String> {
        self.manifest
            .files
            .iter()
            .find(|file| file.filename == name)
            .ok_or_else(|| format!("백업 항목 없음: {name}"))
    }

    pub fn mark(&self, stage: TransactionStage) -> Result<(), String> {
        write_json_atomic(
            &self.dir.join("transaction.json"),
            &Transaction {
                version: 1,
                stage,
                recorded_at: chrono::Local::now().to_rfc3339(),
            },
        )
    }

    pub fn transaction_stage(&self) -> Result<TransactionStage, String> {
        transaction_stage_in(&self.dir)?.ok_or_else(|| "작업 기록 파일이 없습니다.".into())
    }
}

#[cfg(all(test, windows))]
mod atomic_save_tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;

    fn hold_without_delete_sharing(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(path)
            .unwrap()
    }

    #[test]
    fn temporary_windows_replacement_denial_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        write_json_atomic(&path, &serde_json::json!({"stage": "S4"})).unwrap();
        let held = hold_without_delete_sharing(&path);
        // Reproduce the old one-shot persist failure with native Windows I/O.
        let probe = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let denial = probe.persist(&path).unwrap_err();
        assert!(matches!(denial.error.raw_os_error(), Some(5 | 32 | 33)));
        drop(denial);
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(held);
        });
        write_json_atomic(&path, &serde_json::json!({"stage": "S4_DONE"})).unwrap();
        release.join().unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["stage"], "S4_DONE");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn permanent_windows_denial_preserves_original_and_names_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transaction.json");
        let original = b"{\"stage\":\"S4_DONE\"}\n";
        std::fs::write(&path, original).unwrap();
        let held = hold_without_delete_sharing(&path);
        let started = std::time::Instant::now();
        let error = write_json_atomic(&path, &serde_json::json!({"stage":"S5"})).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(error.contains(path.to_str().unwrap()));
        assert!(error.contains("JSON 파일 교체 실패"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        drop(held);
    }

    #[test]
    fn concurrent_readers_only_see_complete_json_checkpoints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let payload = "complete".repeat(8192);
        write_json_atomic(&path, &serde_json::json!({"sequence":0,"payload":payload})).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let reader_path = path.clone();
        let reader_stop = stop.clone();
        let reader = std::thread::spawn(move || {
            let mut reads = 0;
            loop {
                let value: serde_json::Value = read_json(&reader_path).unwrap();
                assert_eq!(value["payload"].as_str().unwrap().len(), 8 * 8192);
                assert!(value["sequence"].as_u64().unwrap() <= 30);
                reads += 1;
                if reader_stop.load(Ordering::Acquire) {
                    return reads;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        for sequence in 1..=30 {
            write_json_atomic(
                &path,
                &serde_json::json!({"sequence":sequence,"payload":payload}),
            )
            .unwrap();
        }
        stop.store(true, Ordering::Release);
        assert!(reader.join().unwrap() > 0);
    }
}
