//! DeviceInfo helper policy and once-only transaction, testable without a device.
pub mod abi;
pub use abi::SHARED_SIZE;
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
};
pub const REGION_OFFSET: usize = 0xD10;
pub const ROLLBACK: std::ops::Range<usize> = 0x898..0x998;
pub type State = [u8; SHARED_SIZE];
pub type Result<T> = std::result::Result<T, String>;

#[derive(Debug)]
pub struct Options {
    pub ta: PathBuf,
    pub prefix: PathBuf,
    pub expected: Option<PathBuf>,
    pub planned: Option<PathBuf>,
    pub read_only: bool,
    pub apply: bool,
    pub reset: bool,
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let mut args = args.into_iter();
        let (mut ta, mut prefix, mut expected, mut planned) = (None, None, None, None);
        let (mut read_only, mut apply, mut reset) = (false, false, false);
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--ta") => ta = Some(PathBuf::from(args.next().ok_or("Missing --ta value")?)),
                Some("--out-prefix") => {
                    prefix = Some(PathBuf::from(
                        args.next().ok_or("Missing --out-prefix value")?,
                    ))
                }
                Some("--expected") => {
                    expected = Some(PathBuf::from(
                        args.next().ok_or("Missing --expected value")?,
                    ))
                }
                Some("--planned") => {
                    planned = Some(PathBuf::from(args.next().ok_or("Missing --planned value")?))
                }
                Some("--read") => read_only = true,
                Some("--apply-once") => apply = true,
                Some("--reset-rollback") => reset = true,
                _ => return Err(format!("Unknown argument: {}", arg.to_string_lossy())),
            }
        }
        if read_only && apply || !read_only && (expected.is_none() || planned.is_none()) {
            return Err(
                "Use --read or a complete --expected/--planned plan; --apply-once is explicit"
                    .into(),
            );
        }
        let ta = ta.ok_or("--ta must explicitly identify the confirmed active-slot Keymaster")?;
        if !matches!(
            ta.to_str(),
            Some(
                "/dev/block/bootdevice/by-name/keymaster_a"
                    | "/dev/block/bootdevice/by-name/keymaster_b"
            )
        ) {
            return Err("Unsupported Keymaster path; use the confirmed active-slot keymaster_a or keymaster_b".into());
        }
        Ok(Self {
            ta,
            prefix: prefix.ok_or("Missing --out-prefix")?,
            expected,
            planned,
            read_only,
            apply,
            reset,
        })
    }
}
pub fn phase(name: &str) {
    println!("{{\"protocol\":1,\"phase\":\"{name}\"}}");
    let _ = io::stdout().flush();
}
pub fn valid_state(data: &State) -> bool {
    &data[..13] == b"ANDROID-BOOT!"
        && &data[0xCC8..0xCD0] == b"deadbeef"
        && &data[0xD20..0xD28] == b"beefdead"
        && data[REGION_OFFSET + 3] == 0
        && matches!(&data[REGION_OFFSET..REGION_OFFSET + 3], b"PRC" | b"ROW")
}
pub fn validate_plan(before: &State, planned: &State, reset: bool) -> Result<()> {
    if !valid_state(before) || !valid_state(planned) {
        return Err("Invalid DeviceInfo profile".into());
    }
    for i in 0..SHARED_SIZE {
        if reset && ROLLBACK.contains(&i) {
            if planned[i] != 0 {
                return Err("All 32 rollback indices must be zero".into());
            }
        } else if !(REGION_OFFSET..REGION_OFFSET + 3).contains(&i) && before[i] != planned[i] {
            return Err(format!("Protected DeviceInfo byte changed at 0x{i:X}"));
        }
    }
    Ok(())
}
pub fn state_from_bytes(bytes: Vec<u8>) -> Result<State> {
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("Expected 4096 bytes, received {}", bytes.len()))
}
pub trait Transport {
    /// Load the TA and check the exact supported GET_VERSION response.
    fn start(&mut self) -> Result<()>;
    fn read(&mut self) -> Result<State>;
    /// Allocate/fill/synchronize the write buffer without issuing 0x203.
    fn prepare_write(&mut self, planned: &State) -> Result<()>;
    /// The only method allowed to issue 0x203. Never retry it.
    fn write_once(&mut self) -> Result<()>;
    fn reopen(&mut self) -> Result<()>;
}
pub trait Evidence {
    fn save(&mut self, suffix: &str, bytes: &[u8]) -> Result<()>;
}
pub struct DiskEvidence(pub PathBuf);
impl Evidence for DiskEvidence {
    fn save(&mut self, suffix: &str, bytes: &[u8]) -> Result<()> {
        let mut path = self.0.as_os_str().to_os_string();
        path.push(".");
        path.push(suffix);
        save_unique(Path::new(&path), bytes)
    }
}
pub fn save_unique(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(target_os = "android")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}
#[derive(Debug)]
pub struct Failure {
    pub exit: u8,
    pub message: String,
}
pub fn execute(
    options: &Options,
    expected: Option<&State>,
    planned: Option<&State>,
    transport: &mut impl Transport,
    evidence: &mut impl Evidence,
) -> std::result::Result<(), Failure> {
    let mut attempted = false;
    let run = (|| -> Result<()> {
        if !options.read_only {
            validate_plan(
                expected.ok_or("Missing original")?,
                planned.ok_or("Missing plan")?,
                options.reset,
            )?;
            phase("PLAN_VALIDATED");
        }
        transport.start()?;
        let live = transport.read()?;
        evidence.save("before.bin", &live)?;
        if options.read_only {
            phase("READ_COMPLETE");
            return Ok(());
        }
        if Some(&live) != expected {
            return Err("Current state differs from the PC backup. No write issued.".into());
        }
        phase("CURRENT_STATE_MATCHES_PC_BACKUP");
        if !options.apply {
            phase("DRY_RUN_VERIFIED_NO_WRITE");
            return Ok(());
        }
        let planned = planned.ok_or("Missing plan")?;
        transport.prepare_write(planned)?;
        evidence.save(
            "write-attempted.txt",
            b"0x203 will be sent exactly once. Do not automatically retry.\n",
        )?;
        phase("WRITE_REQUEST_BEGIN");
        attempted = true;
        let write_result = transport.write_once();
        phase(if write_result.is_ok() {
            "WRITE_RESPONSE_OK"
        } else {
            "WRITE_RESPONSE_ERROR_READBACK_REQUIRED"
        });
        let after = transport.read()?;
        evidence.save("after_same_session.bin", &after)?;
        if &after != planned {
            phase("READBACK_MISMATCH");
            return Err("Same-session readback differs from the full plan".into());
        }
        phase("SAME_SESSION_READBACK_MATCHES_ALL_4096_BYTES");
        transport.reopen()?;
        let fresh = transport.read()?;
        evidence.save("after_new_session.bin", &fresh)?;
        if &fresh != planned {
            phase("READBACK_MISMATCH");
            return Err("New-session readback differs from the full plan".into());
        }
        phase("NEW_SESSION_READBACK_MATCHES_ALL_4096_BYTES");
        write_result
    })();
    run.map_err(|message| Failure {
        exit: if attempted { 40 } else { 1 },
        message,
    })
}
pub fn self_test() -> Result<()> {
    for reset in [false, true] {
        for region in [b"PRC", b"ROW"] {
            let mut before = std::array::from_fn(|i| (i.wrapping_mul(17).wrapping_add(5)) as u8);
            before[..13].copy_from_slice(b"ANDROID-BOOT!");
            before[0xCC8..0xCD0].copy_from_slice(b"deadbeef");
            before[0xD20..0xD28].copy_from_slice(b"beefdead");
            before[REGION_OFFSET..REGION_OFFSET + 16].fill(0);
            before[REGION_OFFSET..REGION_OFFSET + 3].copy_from_slice(region);
            let mut planned = before;
            planned[REGION_OFFSET..REGION_OFFSET + 3].copy_from_slice(if region == b"PRC" {
                b"ROW"
            } else {
                b"PRC"
            });
            if reset {
                planned[ROLLBACK].fill(0);
            }
            validate_plan(&before, &planned, reset)?;
            for i in 0..SHARED_SIZE {
                if (REGION_OFFSET..REGION_OFFSET + 3).contains(&i) {
                    continue;
                }
                planned[i] ^= 1;
                let wrongly_accepted = validate_plan(&before, &planned, reset).is_ok();
                planned[i] ^= 1;
                if wrongly_accepted {
                    return Err(format!("Protected-byte self-test failed at 0x{i:X}"));
                }
            }
            planned[REGION_OFFSET..REGION_OFFSET + 3].copy_from_slice(region);
            validate_plan(&before, &planned, reset)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    fn sample() -> State {
        let mut data = [0x7a; SHARED_SIZE];
        data[..13].copy_from_slice(b"ANDROID-BOOT!");
        data[0xCC8..0xCD0].copy_from_slice(b"deadbeef");
        data[0xD20..0xD28].copy_from_slice(b"beefdead");
        data[REGION_OFFSET..REGION_OFFSET + 4].copy_from_slice(b"PRC\0");
        data
    }
    fn options(apply: bool) -> Options {
        Options {
            ta: "/dev/block/bootdevice/by-name/keymaster_b".into(),
            prefix: "unused".into(),
            expected: Some("before".into()),
            planned: Some("planned".into()),
            read_only: false,
            apply,
            reset: true,
        }
    }
    #[derive(Default)]
    struct MemoryEvidence {
        files: BTreeMap<String, Vec<u8>>,
        fail: Option<&'static str>,
    }
    impl Evidence for MemoryEvidence {
        fn save(&mut self, suffix: &str, bytes: &[u8]) -> Result<()> {
            if self.fail == Some(suffix) || self.files.contains_key(suffix) {
                return Err("Evidence write failed".into());
            }
            self.files.insert(suffix.into(), bytes.into());
            Ok(())
        }
    }
    struct Mock {
        reads: VecDeque<Result<State>>,
        writes: usize,
        fail: Option<&'static str>,
    }
    impl Transport for Mock {
        fn start(&mut self) -> Result<()> {
            if self.fail == Some("start") {
                Err("Start failed".into())
            } else {
                Ok(())
            }
        }
        fn read(&mut self) -> Result<State> {
            self.reads.pop_front().expect("Unexpected extra read")
        }
        fn prepare_write(&mut self, _: &State) -> Result<()> {
            if self.fail == Some("prepare") {
                Err("DMA setup failed".into())
            } else {
                Ok(())
            }
        }
        fn write_once(&mut self) -> Result<()> {
            self.writes += 1;
            if self.fail == Some("write") {
                Err("Write response failed".into())
            } else {
                Ok(())
            }
        }
        fn reopen(&mut self) -> Result<()> {
            if self.fail == Some("reopen") {
                Err("Reopen failed".into())
            } else {
                Ok(())
            }
        }
    }
    fn plan(before: &State) -> State {
        let mut p = *before;
        p[REGION_OFFSET..REGION_OFFSET + 3].copy_from_slice(b"ROW");
        p[ROLLBACK].fill(0);
        p
    }
    #[test]
    fn every_protected_byte_and_both_region_directions() {
        self_test().unwrap();
    }
    #[test]
    fn dry_run_and_read_never_issue_a_write() {
        let b = sample();
        let p = plan(&b);
        for read_only in [false, true] {
            let mut o = options(false);
            o.read_only = read_only;
            let mut t = Mock {
                reads: [Ok(b)].into(),
                writes: 0,
                fail: None,
            };
            let mut e = MemoryEvidence::default();
            execute(&o, Some(&b), Some(&p), &mut t, &mut e).unwrap();
            assert_eq!(t.writes, 0);
            assert_eq!(e.files.len(), 1);
        }
    }
    #[test]
    fn complete_write_requires_both_full_readbacks_and_writes_once() {
        let b = sample();
        let p = plan(&b);
        let mut t = Mock {
            reads: [Ok(b), Ok(p), Ok(p)].into(),
            writes: 0,
            fail: None,
        };
        let mut e = MemoryEvidence::default();
        execute(&options(true), Some(&b), Some(&p), &mut t, &mut e).unwrap();
        assert_eq!(t.writes, 1);
        assert_eq!(e.files.len(), 4);
        assert_eq!(e.files["after_new_session.bin"], p);
    }
    #[test]
    fn preparation_and_intent_failures_return_one_without_a_write() {
        let b = sample();
        let p = plan(&b);
        for fail in [Some("start"), Some("prepare"), None] {
            let mut t = Mock {
                reads: [Ok(b)].into(),
                writes: 0,
                fail,
            };
            let mut e = MemoryEvidence {
                fail: if fail.is_none() {
                    Some("write-attempted.txt")
                } else {
                    None
                },
                ..Default::default()
            };
            let err = execute(&options(true), Some(&b), Some(&p), &mut t, &mut e).unwrap_err();
            assert_eq!(err.exit, 1);
            assert_eq!(t.writes, 0);
        }
    }
    #[test]
    fn current_mismatch_and_invalid_plans_cannot_write() {
        let b = sample();
        let mut p = plan(&b);
        let mut live = b;
        live[100] ^= 1;
        for invalid_plan in [false, true] {
            if invalid_plan {
                p[4000] ^= 1;
            }
            let mut t = Mock {
                reads: [Ok(live)].into(),
                writes: 0,
                fail: None,
            };
            let err = execute(
                &options(true),
                Some(&b),
                Some(&p),
                &mut t,
                &mut MemoryEvidence::default(),
            )
            .unwrap_err();
            assert_eq!(err.exit, 1);
            assert_eq!(t.writes, 0);
        }
    }
    #[test]
    fn all_failures_after_the_single_request_remain_unknown() {
        let b = sample();
        let p = plan(&b);
        let mut corrupt = p;
        corrupt[SHARED_SIZE - 1] ^= 1;
        for (reads, failure, save_failure) in [
            (vec![Ok(b), Err("USB lost".into())], None, None),
            (vec![Ok(b), Ok(corrupt)], None, None),
            (vec![Ok(b), Ok(p), Ok(corrupt)], None, None),
            (vec![Ok(b), Ok(p)], Some("reopen"), None),
            (vec![Ok(b), Ok(p), Ok(p)], Some("write"), None),
            (vec![Ok(b), Ok(p)], None, Some("after_same_session.bin")),
            (
                vec![Ok(b), Ok(p), Ok(p)],
                None,
                Some("after_new_session.bin"),
            ),
        ] {
            let mut t = Mock {
                reads: reads.into(),
                writes: 0,
                fail: failure,
            };
            let mut e = MemoryEvidence {
                fail: save_failure,
                ..Default::default()
            };
            let err = execute(&options(true), Some(&b), Some(&p), &mut t, &mut e).unwrap_err();
            assert_eq!(err.exit, 40);
            assert_eq!(t.writes, 1);
        }
    }
    #[test]
    fn evidence_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("before.bin");
        save_unique(&path, b"original").unwrap();
        assert!(save_unique(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }
    #[test]
    fn keymaster_is_explicit_and_read_apply_is_rejected() {
        let args = |items: &[&str]| items.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(Options::parse(args(&["--read", "--out-prefix", "/tmp/a"])).is_err());
        for slot in ["a", "b"] {
            let ta = format!("/dev/block/bootdevice/by-name/keymaster_{slot}");
            let mut items = args(&["--read", "--ta", &ta, "--out-prefix", "/tmp/a"]);
            assert!(Options::parse(items.clone()).is_ok());
            items.push("--apply-once".into());
            assert!(Options::parse(items).is_err());
        }
        assert!(state_from_bytes(vec![0; SHARED_SIZE - 1]).is_err());
    }
}
