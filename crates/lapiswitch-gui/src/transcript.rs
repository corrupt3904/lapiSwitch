use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

use crate::worker::{Event, POLL_INTERVAL};
use ltbox_core::live_sink::{Entry, Kind};

/// Display canonical Windows paths without changing the paths used for I/O.
/// UNC needs its leading pair of slashes; other device namespaces stay intact.
pub(crate) fn readable_paths(value: &str) -> String {
    static PREFIX: OnceLock<regex::Regex> = OnceLock::new();
    PREFIX
        .get_or_init(|| {
            regex::Regex::new(r"\\\\\?\\(?:(?i:UNC)\\|([A-Za-z]:\\))").expect("constant regex")
        })
        .replace_all(value, |captures: &regex::Captures<'_>| {
            captures
                .get(1)
                .map_or(r"\\", |root| root.as_str())
                .to_owned()
        })
        .into_owned()
}

/// Masks explicitly labelled serials and ADB device-list identifiers. It does
/// not redact backup/identity payloads, which are needed for verified recovery.
pub fn redact(line: &str) -> String {
    static SERIAL: OnceLock<regex::Regex> = OnceLock::new();
    static ADB: OnceLock<regex::Regex> = OnceLock::new();
    let serial = SERIAL.get_or_init(|| {
        regex::Regex::new(
        r"(?i)(\b(?:serialno|serial_number|serial\s+number|serial|ro\.serialno)\s*[:=]\s*)[^\s,;]+"
    ).expect("constant regex")
    });
    let adb = ADB.get_or_init(|| {
        regex::Regex::new(r"(?m)^(\s*)[^\s]+(\s+(?:device|unauthorized|offline)(?:\s|$))")
            .expect("constant regex")
    });
    adb.replace_all(
        &serial.replace_all(line, "${1}[masked]"),
        "${1}[masked]${2}",
    )
    .into_owned()
}

/// All new transcripts share the root selected for logs and backups.
pub(crate) fn log_dir(storage_root: &Path) -> PathBuf {
    storage_root.join("logs")
}

pub struct Transcript {
    path: PathBuf,
    writer: BufWriter<File>,
    mask: bool,
}
impl Transcript {
    pub fn create(dir: &Path, mask: bool) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis();
        for suffix in 0..1000 {
            let path = dir.join(format!("session_{stamp}_{suffix}.log"));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        writer: BufWriter::new(file),
                        mask,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("로그 파일을 만들 수 없습니다.".into())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn line(&mut self, line: &str) -> Result<String, String> {
        let line = self.display(line);
        writeln!(self.writer, "{line}").map_err(|e| e.to_string())?;
        Ok(line)
    }
    fn display(&self, line: &str) -> String {
        let line = readable_paths(line);
        if self.mask { redact(&line) } else { line }
    }
    fn flush(&mut self) -> Result<(), String> {
        self.writer.flush().map_err(|e| e.to_string())
    }
}

/// Progress updates replace a transient UI row. Persist only the latest
/// update before the next transfer/message or shutdown, keeping diagnostics
/// and ordinary repeated messages in their original order.
struct LiveTranscript {
    transcript: Transcript,
    tx: Sender<Event>,
    progress: Option<(String, String)>,
    failure: Option<String>,
}
impl LiveTranscript {
    fn new(transcript: Transcript, tx: Sender<Event>) -> Self {
        Self {
            transcript,
            tx,
            progress: None,
            failure: None,
        }
    }
    fn line(&mut self, line: &str) {
        match self.transcript.line(line) {
            Ok(line) => {
                let _ = self.tx.send(Event::Log(line));
            }
            Err(error) => {
                self.failure.get_or_insert(error);
                let _ = self.tx.send(Event::Log(self.transcript.display(line)));
            }
        }
    }
    fn finish_transfer(&mut self) {
        if let Some((_, line)) = self.progress.take() {
            let _ = self.tx.send(Event::Transfer(None));
            self.line(&line);
        }
    }
    fn entry(&mut self, entry: Entry) {
        if let Kind::Progress { key } = entry.kind {
            if self
                .progress
                .as_ref()
                .is_some_and(|(previous, _)| previous != &key)
            {
                self.finish_transfer();
            }
            let _ = self
                .tx
                .send(Event::Transfer(Some(self.transcript.display(&entry.line))));
            self.progress = Some((key, entry.line));
        } else {
            self.finish_transfer();
            self.line(&entry.line);
        }
    }
    fn flush(&mut self) {
        if let Err(error) = self.transcript.flush() {
            self.failure.get_or_insert(error);
        }
    }
}

pub struct Pump {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<Result<(), String>>>,
}
impl Pump {
    pub fn start(transcript: Transcript, tx: Sender<Event>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut output = LiveTranscript::new(transcript, tx.clone());
            loop {
                let done = stopped.load(Ordering::Acquire);
                for entry in ltbox_core::live_sink::drain() {
                    output.entry(entry);
                }
                output.flush();
                if done {
                    output.finish_transfer();
                    output.flush();
                    break;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            output.failure.map_or(Ok(()), |e| {
                Err(format!(
                    "로그 저장 실패 (작업 상태와 백업을 확인하세요): {e}"
                ))
            })
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
    pub fn finish(mut self) -> Result<(), String> {
        self.join()
    }
    fn join(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        self.handle.take().map_or(Ok(()), |h| {
            h.join()
                .unwrap_or_else(|_| Err("로그 작업 종료 오류".into()))
        })
    }
}
impl Drop for Pump {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn product_log_folder_preserves_legacy_logs_and_writes_new_transcripts() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("tb520-region-tool");
        std::fs::create_dir_all(old.join("logs")).unwrap();
        let previous = old.join("logs/previous.log");
        std::fs::write(&previous, b"previous run").unwrap();
        let expected = root.path().join("lapiSwitch/logs");
        assert_eq!(log_dir(&root.path().join("lapiSwitch")), expected);
        assert_eq!(
            log_dir(&root.path().join("custom root")),
            root.path().join("custom root/logs")
        );
        let mut transcript = Transcript::create(&expected, false).unwrap();
        transcript.line("new run").unwrap();
        transcript.flush().unwrap();
        assert_eq!(
            std::fs::read_to_string(transcript.path()).unwrap(),
            "new run\n"
        );
        assert_eq!(std::fs::read(previous).unwrap(), b"previous run");
    }
    #[test]
    fn path_display_preserves_unc_and_other_device_namespaces() {
        let raw =
            r"read \\?\C:\Users\Demo\.tmp\staged_boot.img -> \\?\UNC\server\share\backup.json";
        let expected = r"read C:\Users\Demo\.tmp\staged_boot.img -> \\server\share\backup.json";
        assert_eq!(readable_paths(raw), expected);
        assert_eq!(readable_paths(expected), expected);
        assert_eq!(readable_paths(r"\\?\unc\server\share"), r"\\server\share");
        for unchanged in [
            r"\\?\Volume{example}\data",
            r"\\.\COM8",
            "/tmp/data",
            r"C:\normal",
        ] {
            assert_eq!(readable_paths(unchanged), unchanged);
        }
    }
    #[test]
    fn readable_paths_reach_saved_logs_and_live_progress_with_serial_masking() {
        for mask in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let transcript = Transcript::create(dir.path(), mask).unwrap();
            let path = transcript.path().to_owned();
            let (tx, rx) = std::sync::mpsc::channel();
            let mut output = LiveTranscript::new(transcript, tx);
            output.entry(Entry::info(
                r"read \\?\C:\backup\boot.img serialno:SECRET".into(),
            ));
            output.entry(Entry {
                line: r"write \\?\UNC\server\share\boot.img serialno:SECRET".into(),
                kind: Kind::Progress {
                    key: "write:boot".into(),
                },
            });
            output.finish_transfer();
            output.flush();
            let saved = std::fs::read_to_string(path).unwrap();
            assert!(!saved.contains(r"\\?\"));
            assert!(saved.contains(r"C:\backup\boot.img"));
            assert!(saved.contains(r"\\server\share\boot.img"));
            assert_eq!(saved.contains("SECRET"), !mask);
            for event in rx.try_iter() {
                if let Event::Log(line) | Event::Transfer(Some(line)) = event {
                    assert!(!line.contains(r"\\?\"));
                    assert_eq!(line.contains("SECRET"), !mask);
                }
            }
        }
    }
    #[test]
    fn transfer_updates_replace_ui_tail_and_persist_only_final_lines_without_losing_errors() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = Transcript::create(dir.path(), true).unwrap();
        let path = transcript.path().to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut output = LiveTranscript::new(transcript, tx);
        for percent in 0..=100 {
            output.entry(Entry {
                line: format!("쓰기 boot: {percent}% serialno:SECRET"),
                kind: Kind::Progress {
                    key: "write:1".into(),
                },
            });
            output.flush();
        }
        output.entry(Entry::info("쓰기 검증 실패".into()));
        output.entry(Entry::info("쓰기 검증 실패".into()));
        output.entry(Entry {
            line: "읽기 GPT: 100%".into(),
            kind: Kind::Progress {
                key: "read:2".into(),
            },
        });
        output.entry(Entry {
            line: "쓰기 GPT: 50%".into(),
            kind: Kind::Progress {
                key: "write:3".into(),
            },
        });
        output.finish_transfer();
        output.flush();
        assert!(output.failure.is_none());
        let saved = std::fs::read_to_string(path).unwrap();
        assert_eq!(saved.lines().count(), 5);
        assert!(
            saved
                .starts_with("쓰기 boot: 100% serialno:[masked]\n쓰기 검증 실패\n쓰기 검증 실패\n")
        );
        assert!(!saved.contains("SECRET"));
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Log(_)))
                .count(),
            5
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Transfer(Some(_))))
                .count(),
            103
        );
        assert!(
            events.iter().all(
                |event| !matches!(event, Event::Transfer(Some(line)) if line.contains("SECRET"))
            )
        );
    }
    #[test]
    fn masks_serial_fields_and_adb_rows_without_destroying_rollback_or_fingerprint() {
        assert_eq!(
            redact("serialno:HA289EBP\nHA289EBP\tdevice\nboot: 1775347200"),
            "serialno:[masked]\n[masked]\tdevice\nboot: 1775347200"
        );
        assert_eq!(
            redact("Serial number = 12345, model TB520FU"),
            "Serial number = [masked], model TB520FU"
        );
        assert_eq!(
            redact("Lenovo/TB520FU_ROW/user boot:2026-04-05"),
            "Lenovo/TB520FU_ROW/user boot:2026-04-05"
        );
    }
    #[test]
    fn transcript_uses_unique_files_and_masks_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Transcript::create(dir.path(), true).unwrap();
        let b = Transcript::create(dir.path(), true).unwrap();
        assert_ne!(a.path(), b.path());
        a.line("serialno:SECRET").unwrap();
        a.flush().unwrap();
        assert!(
            !std::fs::read_to_string(a.path())
                .unwrap()
                .contains("SECRET")
        );
    }
}
