//! DeviceInfo faults and restore-before-flash sequencing. Synthetic data only.

#[test]
fn same_guid_with_a_moved_recovery_partition_is_rejected_before_writing() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    let mut changed = s.disk.snapshot.clone();
    changed
        .iter_mut()
        .find(|p| p.name == "recovery_b")
        .unwrap()
        .start_sector += 10;
    let live = gpt(&changed);
    s.disk.disk[4][..live.len()].copy_from_slice(&live);
    assert!(recovery_flow::restore_originals(&mut s.disk, &s.backup, &mut vec![]).is_err());
    assert!(s.disk.writes.is_empty());
}

#[test]
fn recovery_rw_persist_changes_are_accepted_but_another_gpt_guid_is_not() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    let offset = 50 * 4096 + 10;
    s.disk.disk[0][offset] ^= 1;
    recovery_flow::restore_originals(&mut s.disk, &s.backup, &mut vec![]).unwrap();
    let before = s.disk.writes.len();
    s.disk.disk[4][4096 + 56] ^= 1;
    assert!(recovery_flow::restore_originals(&mut s.disk, &s.backup, &mut vec![]).is_err());
    assert_eq!(s.disk.writes.len(), before);
}

#[test]
#[ignore = "Requires pinned upstream TWRP file; set LAPISWITCH_TWRP_TEST_IMAGE. Uses memory/disk only."]
fn pinned_twrp_stages_only_active_recovery_and_rolls_back_a_failed_transfer() {
    let image =
        PathBuf::from(std::env::var_os("LAPISWITCH_TWRP_TEST_IMAGE").expect("TWRP fixture path"));
    for (slot, fail) in [(Slot::A, false), (Slot::B, false), (Slot::B, true)] {
        let mut s = Setup::with_recovery_size(slot, RollbackMode::Reset, true);
        s.backup.mark(TransactionStage::BackupVerified).unwrap();
        recovery_flow::checkpoint(&mut s.state, &s.store, Phase::BackedUp).unwrap();
        let misc = s.disk.bytes("misc").to_vec();
        if fail {
            s.disk.corrupt_writes.insert(2);
        }
        let result = recovery_flow::stage(
            &mut s.disk,
            &s.backup,
            &image,
            &mut s.state,
            &s.store,
            &mut vec![],
        );
        s.backup.verify().unwrap();
        assert!(
            s.disk
                .bytes(&recovery_flow::recovery_name(slot.other()))
                .iter()
                .all(|b| *b == 0x77)
        );
        assert_eq!(
            std::fs::read(s.backup.file_path("misc.img").unwrap()).unwrap(),
            misc
        );
        assert!(!s.disk.writes.iter().any(|p| p.name.starts_with("boot_")));
        if fail {
            assert!(result.is_err());
            assert_eq!(
                s.backup.transaction_stage().unwrap(),
                TransactionStage::Restored
            );
        } else {
            result.unwrap();
            assert_eq!(
                s.disk
                    .writes
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>(),
                [
                    "gpt_main4".to_owned(),
                    recovery_flow::recovery_name(slot),
                    "misc".to_owned()
                ]
            );
            assert_eq!(
                s.disk.bytes("misc"),
                deviceinfo::recovery_misc(&misc).unwrap()
            );
            recovery_flow::restore_originals(&mut s.disk, &s.backup, &mut vec![]).unwrap();
            assert_eq!(
                s.disk.bytes("misc"),
                deviceinfo::recovery_misc(&misc).unwrap()
            );
        }
        assert_eq!(
            &s.disk.disk[4][..s.disk.original_gpt.len()],
            &s.disk.original_gpt
        );
        assert!(
            s.disk
                .bytes(&recovery_flow::recovery_name(slot))
                .iter()
                .all(|b| *b == 0x77)
        );
    }
}

#[test]
fn legacy_restore_rejects_new_backup_and_firmware_boundary_is_preserved() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    assert!(stage1::restore(&mut s.disk, &s.backup, &mut vec![]).is_err());
    assert!(s.disk.writes.is_empty());
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::FirmwareWriting).unwrap();
    assert!(recovery_flow::restore_originals(&mut s.disk, &s.backup, &mut vec![]).is_err());
    assert!(s.disk.writes.is_empty());
    assert_eq!(
        s.state.resume_action(None),
        lapiswitch_core::state::ResumeAction::Recovery
    );
}
#[path = "support/memory_device.rs"]
mod memory_device;
use lapiswitch_core::{
    avb_info::AvbSummary,
    backup::{BackupBuilder, TransactionStage, VerifiedBackup},
    deviceinfo::{self, RollbackMode},
    firmware::FirmwareReport,
    recovery_flow::{self, DeviceInfoIo, Phase, Transition},
    slot::Slot,
    stage1::{self, ResetMode, StageIo},
    state::{FlashOptions, RunState, StateStore},
    storage::Partition,
    workflow::FinishIo,
};
use memory_device::*;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Ui {
    cancel: bool,
    deny_handoff: bool,
    phases: Vec<Phase>,
}
impl recovery_flow::Ui for Ui {
    fn phase(&mut self, phase: Phase) {
        self.phases.push(phase);
    }
    fn confirm(&mut self, _: Transition) -> Result<(), String> {
        Ok(())
    }
    fn status(&mut self, _: &str, _: bool) {}
    fn cancelled(&self) -> bool {
        self.cancel
    }
    fn finish_wait(&mut self) -> bool {
        !self.deny_handoff && !self.cancel
    }
}
fn sample(r: lapiswitch_core::region::Region) -> Vec<u8> {
    let mut b = vec![0x5A; deviceinfo::SHARED_SIZE];
    b[..13].copy_from_slice(b"ANDROID-BOOT!");
    b[0xCC8..0xCD0].copy_from_slice(b"deadbeef");
    b[0xD20..0xD28].copy_from_slice(b"beefdead");
    b[0xD10..0xD13].copy_from_slice(r.token().as_bytes());
    b[0xD13] = 0;
    b
}
struct Info {
    bytes: Vec<u8>,
    writes: usize,
    reads: usize,
    fail_after_apply: bool,
    corrupt: bool,
    unstable: bool,
    /// Transfer or dry-run failure: no write request is possible.
    fail_prepare: bool,
    /// The write call fails without changing DeviceInfo (e.g. helper exit 1).
    fail_before_write: bool,
    /// The failed write call also disconnects USB for the following reads.
    lose_usb_on_write: bool,
    fail_reads: bool,
    prepared: bool,
    state_file: PathBuf,
    backup_dir: PathBuf,
}
impl Info {
    fn phase(&self) -> Phase {
        StateStore::new(self.state_file.clone())
            .load()
            .unwrap()
            .unwrap()
            .recovery
            .unwrap()
            .phase
    }
}
impl DeviceInfoIo for Info {
    fn read(&mut self, out: &Path, _: &mut Vec<String>) -> Result<Vec<u8>, String> {
        if self.fail_reads {
            return Err("USB read failed".into());
        }
        self.reads += 1;
        let mut b = self.bytes.clone();
        if self.unstable && self.reads == 2 {
            b[0x9B] ^= 1;
        }
        std::fs::write(PathBuf::from(format!("{}.before.bin", out.display())), &b)
            .map_err(|e| e.to_string())?;
        Ok(b)
    }
    fn prepare(
        &mut self,
        before: &[u8],
        planned: &[u8],
        mode: RollbackMode,
        _: &mut Vec<String>,
    ) -> Result<(), String> {
        // No durable write intent may exist while transfers can still fail.
        assert_eq!(self.phase(), Phase::Planned);
        assert_eq!(before, self.bytes);
        deviceinfo::validate_plan(before, planned, deviceinfo::region(planned)?, mode)?;
        if self.fail_prepare {
            return Err("push/dry-run failed".into());
        }
        self.prepared = true;
        Ok(())
    }
    fn apply(
        &mut self,
        before: &[u8],
        planned: &[u8],
        mode: RollbackMode,
        out: &Path,
        _: &mut Vec<String>,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        assert!(std::mem::take(&mut self.prepared), "apply without prepare");
        assert_eq!(self.phase(), Phase::WritePending);
        assert!(self.backup_dir.join("recovery_progress.json").is_file());
        let state = StateStore::new(self.state_file.clone()).load()?.unwrap();
        let record = state.recovery.unwrap().attempt.unwrap();
        let original = self
            .backup_dir
            .join("Partition")
            .join(record.directory)
            .join("before_shared_1.bin");
        assert_eq!(std::fs::read(original).unwrap(), before);
        assert_eq!(
            record.planned_sha256,
            lapiswitch_core::backup::hash_file(
                &{
                    let path = out.with_extension("expected-test-plan.bin");
                    std::fs::write(&path, planned).unwrap();
                    path
                },
                None
            )?
            .1
        );
        assert_eq!(before, self.bytes);
        let p = planned.to_vec();
        deviceinfo::validate_plan(&self.bytes, &p, deviceinfo::region(&p)?, mode)?;
        if self.fail_before_write {
            self.fail_reads = self.lose_usb_on_write;
            return Err("DeviceInfo helper가 쓰기 요청 전에 종료했습니다 (exit 1).".into());
        }
        self.writes += 1;
        self.bytes = p;
        if self.fail_after_apply {
            return Err("USB disconnect after write".into());
        }
        let mut a = self.bytes.clone();
        if self.corrupt {
            a[0xFFF] ^= 1;
        }
        std::fs::write(PathBuf::from(format!("{}.same.bin", out.display())), &a)
            .map_err(|e| e.to_string())?;
        std::fs::write(
            PathBuf::from(format!("{}.fresh.bin", out.display())),
            &self.bytes,
        )
        .map_err(|e| e.to_string())?;
        Ok((a, self.bytes.clone()))
    }
}
struct Setup {
    _dir: tempfile::TempDir,
    disk: Fake,
    backup: VerifiedBackup,
    firmware: FirmwareReport,
    state: RunState,
    store: StateStore,
    info: Info,
}
impl Setup {
    fn new(slot: Slot, mode: RollbackMode) -> Self {
        Self::with_recovery_size(slot, mode, false)
    }
    fn with_recovery_size(slot: Slot, mode: RollbackMode, large: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let fwdir = dir.path().join("firmware");
        std::fs::create_dir(&fwdir).unwrap();
        for name in [
            "boot.img",
            "vbmeta_system.img",
            "vbmeta.img",
            "vendor_boot.img",
            "loader.melf",
        ] {
            std::fs::write(fwdir.join(name), b"synthetic").unwrap();
        }
        let mut fw = firmware();
        fw.image_dir = fwdir.clone();
        fw.loader = fwdir.join("loader.melf");
        let mut disk = Fake::new(600, true);
        disk.snapshot.extend([
            part("recovery_a", 4, 1600, 2),
            part("recovery_b", 4, 1602, 2),
            part("misc", 0, 70, 2),
            part("vendor_boot_a", 4, 1700, 4),
            part("vendor_boot_b", 4, 1710, 4),
        ]);
        if large {
            for p in &mut disk.snapshot {
                if p.name == "recovery_a" {
                    p.start_sector = 1600;
                    p.num_sectors = 25600;
                }
                if p.name == "recovery_b" {
                    p.start_sector = 27200;
                    p.num_sectors = 25600;
                }
            }
            disk.disk[4].resize(60002 * 4096, 0x77);
            for p in &mut disk.snapshot {
                if p.name == "vendor_boot_a" {
                    p.start_sector = 59000;
                }
                if p.name == "vendor_boot_b" {
                    p.start_sector = 59010;
                }
            }
        }
        for name in ["vendor_boot_a", "vendor_boot_b"] {
            let p = disk.snapshot.iter().find(|p| p.name == name).unwrap();
            let start = p.start_sector as usize * 4096;
            disk.disk[4][start..start + 16384].copy_from_slice(&vendor_image());
        }
        disk.original_gpt = gpt(&disk.snapshot);
        if large {
            disk.original_gpt[4096 + 32..4096 + 40].copy_from_slice(&60001u64.to_le_bytes());
            disk.original_gpt[4096 + 48..4096 + 56].copy_from_slice(&60000u64.to_le_bytes());
            refresh_header(&mut disk.original_gpt);
        }
        disk.disk[4][..disk.original_gpt.len()].copy_from_slice(&disk.original_gpt);
        let builder = BackupBuilder::create(dir.path()).unwrap().for_recovery();
        let mut options = FlashOptions::new(true, None).unwrap();
        options.rollback_mode = mode;
        let mut state = RunState::new(&fw, options, builder.path()).unwrap();
        state.version = 2;
        state.recovery = Some(Default::default());
        let store = StateStore::new(dir.path().join("state.json"));
        store.begin(&state).unwrap();
        let backup = stage1::take_backup(
            &mut disk,
            &prepared(slot, lapiswitch_core::prepare::Connection::Fastboot),
            &fw,
            builder,
            &mut vec![],
        )
        .unwrap();
        state.bind_backup(&backup).unwrap();
        backup.mark(TransactionStage::S4Done).unwrap();
        recovery_flow::checkpoint(&mut state, &store, Phase::RecoveryWaiting).unwrap();
        let info = Info {
            bytes: sample(lapiswitch_core::region::Region::Prc),
            writes: 0,
            reads: 0,
            fail_after_apply: false,
            corrupt: false,
            unstable: false,
            fail_prepare: false,
            fail_before_write: false,
            lose_usb_on_write: false,
            fail_reads: false,
            prepared: false,
            state_file: store.path().into(),
            backup_dir: backup.path().into(),
        };
        Self {
            _dir: dir,
            disk,
            backup,
            firmware: fw,
            state,
            store,
            info,
        }
    }
    fn transact(&mut self) -> Result<(), String> {
        recovery_flow::transact(
            &mut self.info,
            &self.backup,
            &self.firmware,
            &mut self.state,
            &self.store,
            &mut Ui::default(),
            &mut vec![],
        )
    }
}
#[test]
fn backup_v5_records_active_recovery_vendor_boot_and_misc_without_boot() {
    for slot in [Slot::A, Slot::B] {
        let s = Setup::new(slot, RollbackMode::Reset);
        assert_eq!(s.backup.manifest().version, 5);
        assert!(s.backup.file_path("boot_a.img").is_err());
        assert!(s.backup.file_path("boot_b.img").is_err());
        assert!(!s.backup.path().join("Partition/boot_a.img").exists());
        assert!(!s.backup.path().join("Partition/boot_b.img").exists());
        assert!(
            s.backup
                .file_path(&format!("recovery{}.img", slot.suffix()))
                .unwrap()
                .is_file()
        );
        assert!(
            s.backup
                .file_path(&format!("recovery{}.img", slot.other().suffix()))
                .is_err()
        );
        assert!(s.backup.file_path("misc.img").unwrap().is_file());
        assert!(s.state.persist_fingerprint.is_none());
    }
}
#[test]
fn reset_backs_up_before_one_write_and_preserves_all_other_bytes() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    let before = s.info.bytes.clone();
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
    assert_eq!(deviceinfo::indices(&s.info.bytes).unwrap(), [0; 32]);
    deviceinfo::validate_plan(
        &before,
        &s.info.bytes,
        s.state.target_region,
        RollbackMode::Reset,
    )
    .unwrap();
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
}
#[test]
fn keep_rejects_lower_firmware_using_live_deviceinfo() {
    let mut s = Setup::new(Slot::A, RollbackMode::Keep);
    assert!(
        s.transact()
            .unwrap_err()
            .contains(lapiswitch_core::prepare::ROLLBACK_REJECTED_MESSAGE)
    );
    assert_eq!(s.info.writes, 0);
}
#[test]
fn keep_allows_valid_indices_and_never_changes_them() {
    let mut s = Setup::new(Slot::A, RollbackMode::Keep);
    s.info.bytes[0x898..0x998].fill(0);
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
    assert_eq!(deviceinfo::indices(&s.info.bytes).unwrap(), [0; 32]);
}
#[test]
fn same_target_keep_is_noop_but_reset_must_clear_existing_indices() {
    for mode in [RollbackMode::Keep, RollbackMode::Reset] {
        let mut s = Setup::new(Slot::A, mode);
        s.info.bytes = sample(s.state.target_region);
        if mode == RollbackMode::Keep {
            s.info.bytes[0x898..0x998].fill(0);
        }
        s.transact().unwrap();
        assert_eq!(s.info.writes, usize::from(mode == RollbackMode::Reset));
    }
}
#[test]
fn usb_loss_after_write_recovers_by_reading_never_rewriting() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.fail_after_apply = true;
    assert!(s.transact().is_err());
    assert_eq!(
        s.state.recovery.as_ref().unwrap().phase,
        Phase::WritePending
    );
    s.info.fail_after_apply = false;
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
    assert_eq!(
        s.state.recovery.as_ref().unwrap().phase,
        Phase::RegionVerified
    );
}
#[test]
fn unknown_or_corrupt_readback_stays_pending_without_auto_retry() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.corrupt = true;
    assert!(s.transact().is_err());
    s.info.bytes[0xFFF] ^= 1;
    assert!(s.transact().is_err());
    assert_eq!(s.info.writes, 1);
    assert_eq!(
        s.state.recovery.as_ref().unwrap().phase,
        Phase::WritePending
    );
}
#[test]
fn unstable_reads_and_disk_save_failure_prevent_write() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.unstable = true;
    assert!(s.transact().is_err());
    assert_eq!(s.info.writes, 0);
    s.info.unstable = false;
    let old = std::fs::read(s.store.path()).unwrap();
    std::fs::remove_file(s.store.path()).unwrap();
    std::fs::create_dir(s.store.path()).unwrap();
    assert!(s.transact().is_err());
    assert_eq!(s.info.writes, 0);
    std::fs::remove_dir(s.store.path()).unwrap();
    std::fs::write(s.store.path(), old).unwrap();
}
#[test]
fn tampered_saved_plan_is_never_replayed() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.fail_after_apply = true;
    assert!(s.transact().is_err());
    let a = s.state.recovery.as_ref().unwrap().attempt.as_ref().unwrap();
    let p = s
        .backup
        .path()
        .join("Partition")
        .join(&a.directory)
        .join("planned_shared.bin");
    let before = std::fs::read(p.with_file_name("before_shared_1.bin")).unwrap();
    let mut b = deviceinfo::plan(&before, s.state.target_region, a.mode).unwrap();
    b[0x94] ^= 1;
    std::fs::write(p, b).unwrap();
    assert!(s.transact().is_err());
    assert_eq!(s.info.writes, 1);
}

fn phase(s: &Setup) -> Phase {
    let phase = s.state.recovery.as_ref().unwrap().phase;
    // The durable checkpoint matches the in-memory state.
    assert_eq!(s.info.phase(), phase);
    phase
}

#[test]
fn transfer_or_dry_run_failure_never_records_a_write_intent() {
    use recovery_flow::restore::{self, Assessment};
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.fail_prepare = true;
    assert!(s.transact().is_err());
    assert_eq!(s.info.writes, 0);
    assert_eq!(phase(&s), Phase::Planned);
    assert_eq!(restore::assess(&s.backup).unwrap(), Assessment::BeforeWrite);
    s.info.fail_prepare = false;
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
    assert_eq!(phase(&s), Phase::RegionVerified);
}

#[test]
fn failed_write_request_with_two_original_reads_returns_to_planned_without_retry() {
    use recovery_flow::restore::{self, Assessment};
    for slot in [Slot::A, Slot::B] {
        let mut s = Setup::new(slot, RollbackMode::Reset);
        let original = s.info.bytes.clone();
        s.info.fail_before_write = true;
        let error = s.transact().unwrap_err();
        assert!(error.contains("쓰기 요청 전"));
        assert!(error.contains(recovery_flow::NOT_APPLIED_MESSAGE));
        assert_eq!(s.info.writes, 0);
        assert_eq!(s.info.bytes, original);
        assert_eq!(phase(&s), Phase::Planned);
        assert_eq!(restore::assess(&s.backup).unwrap(), Assessment::BeforeWrite);
        // Only a new, explicit resume issues the single write request.
        s.info.fail_before_write = false;
        s.transact().unwrap();
        assert_eq!(s.info.writes, 1);
        assert_eq!(phase(&s), Phase::RegionVerified);
    }
}

#[test]
fn unprovable_write_stays_pending_until_resume_reads_the_exact_original_twice() {
    use recovery_flow::restore::{self, Assessment};
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.fail_before_write = true;
    s.info.lose_usb_on_write = true;
    let error = s.transact().unwrap_err();
    assert!(!error.contains(recovery_flow::NOT_APPLIED_MESSAGE));
    assert_eq!(phase(&s), Phase::WritePending);
    assert!(matches!(
        restore::assess(&s.backup).unwrap(),
        Assessment::Unknown(_)
    ));
    // Reconnected: two exact original reads prove no write; stop, never write.
    s.info.fail_reads = false;
    s.info.fail_before_write = false;
    assert_eq!(
        s.transact().unwrap_err(),
        recovery_flow::NOT_APPLIED_MESSAGE
    );
    assert_eq!(s.info.writes, 0);
    assert_eq!(phase(&s), Phase::Planned);
    assert_eq!(restore::assess(&s.backup).unwrap(), Assessment::BeforeWrite);
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
}

#[test]
fn verified_region_is_never_downgraded_by_original_reads() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    let original = s.info.bytes.clone();
    s.transact().unwrap();
    s.info.bytes = original;
    let error = s.transact().unwrap_err();
    assert!(!error.contains(recovery_flow::NOT_APPLIED_MESSAGE));
    assert_eq!(phase(&s), Phase::RegionVerified);
    assert_eq!(s.info.writes, 1);
}

#[derive(Default)]
struct EdlScript {
    ports: std::collections::VecDeque<Option<&'static str>>,
    opens: std::collections::VecDeque<Result<bool, String>>,
    attempts: usize,
}
struct Finish {
    edl: EdlScript,
    inner: Fake,
    flashed: usize,
    active: usize,
    reboots: usize,
    slot: Slot,
}
impl StageIo for Finish {
    fn ensure_single_target(&mut self) -> Result<(), String> {
        self.inner.ensure_single_target()
    }
    fn read_avb(&mut self, n: &str, l: &mut Vec<String>) -> Result<Option<AvbSummary>, String> {
        self.inner.read_avb(n, l)
    }
    fn scan(&mut self, l: &mut Vec<String>) -> Result<Vec<Partition>, String> {
        self.inner.scan(l)
    }
    fn validate_snapshot(&mut self, p: &[Partition]) -> Result<(), String> {
        self.inner.validate_snapshot(p)
    }
    fn confirm_proinfo_absent(&mut self, l: &mut Vec<String>) -> Result<(), String> {
        self.inner.confirm_proinfo_absent(l)
    }
    fn dump_at(&mut self, p: &Partition, out: &Path, l: &mut Vec<String>) -> Result<(), String> {
        self.inner.dump_at(p, out, l)
    }
    fn write_at(&mut self, p: &Partition, path: &Path, l: &mut Vec<String>) -> Result<(), String> {
        self.inner.write_at(p, path, l)
    }
    fn reset(&mut self, m: ResetMode, l: &mut Vec<String>) -> Result<(), String> {
        self.inner.reset(m, l)
    }
}
impl FinishIo for Finish {
    fn probe_edl(&mut self) -> Result<Option<String>, String> {
        Ok(self
            .edl
            .ports
            .pop_front()
            .unwrap_or(Some("fake"))
            .map(str::to_owned))
    }
    fn reboot_recovery(&mut self, _: &mut Vec<String>) -> Result<(), String> {
        panic!("no recovery reboot in post-verified tests")
    }
    fn open_edl(&mut self, _: &Path, _: &mut Vec<String>) -> Result<(), String> {
        Ok(())
    }
    fn try_open_edl(&mut self, _: &Path, _: &mut Vec<String>) -> Result<bool, String> {
        self.edl.attempts += 1;
        self.edl.opens.pop_front().unwrap_or(Ok(true))
    }
    fn flash_firmware(
        &mut self,
        _: &FirmwareReport,
        wipe: bool,
        _: &mut Vec<String>,
    ) -> Result<(), String> {
        assert!(wipe);
        assert_eq!(
            self.inner
                .writes
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            [
                recovery_flow::recovery_name(self.slot).as_str(),
                "gpt_main4"
            ]
        );
        assert_eq!(
            &self.inner.disk[4][..self.inner.original_gpt.len()],
            &self.inner.original_gpt
        );
        assert!(
            self.inner
                .bytes(&recovery_flow::recovery_name(self.slot))
                .iter()
                .all(|b| *b == 0x77)
        );
        self.flashed += 1;
        Ok(())
    }
    fn set_active_a(&mut self, _: &mut Vec<String>) -> Result<(), String> {
        self.active += 1;
        Ok(())
    }
    fn final_reboot(&mut self, _: &mut Vec<String>) -> Option<String> {
        self.reboots += 1;
        None
    }
}
#[test]
fn firmware_requires_verified_deviceinfo_and_original_recovery_gpt_first() {
    for slot in [Slot::A, Slot::B] {
        let mut s = Setup::new(slot, RollbackMode::Reset);
        s.transact().unwrap();
        s.disk.disk[0][50 * 4096 + 10] ^= 1; // Recovery secure services legitimately update persist.
        recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
        let mut d = Finish {
            edl: EdlScript {
                opens: [Ok(false), Ok(true)].into(),
                ..Default::default()
            },
            inner: s.disk,
            flashed: 0,
            active: 0,
            reboots: 0,
            slot,
        };
        let mut ui = Ui::default();
        recovery_flow::finish(
            &mut d,
            &s.backup,
            &s.firmware,
            &mut s.state,
            &s.store,
            &mut ui,
            &mut vec![],
        )
        .unwrap();
        assert_eq!(d.edl.attempts, 2);
        assert_eq!((d.flashed, d.active, d.reboots), (1, 1, 1));
        assert_eq!(
            ui.phases,
            vec![
                Phase::EdlWaiting,
                Phase::EdlWaiting,
                Phase::Restoring,
                Phase::FirmwareWriting,
                Phase::FinalReboot
            ]
        );
        assert_eq!(s.state.recovery.unwrap().phase, Phase::Done);
        assert!(
            !d.inner
                .writes
                .iter()
                .any(|p| p.name == "misc" || p.name.starts_with("boot_"))
        );
    }
}
#[test]
fn failed_restore_or_missing_evidence_blocks_firmware() {
    for corrupt in [true, false] {
        let mut s = Setup::new(Slot::B, RollbackMode::Reset);
        if corrupt {
            s.transact().unwrap();
            s.disk.corrupt_writes.insert(1);
        }
        recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
        let mut d = Finish {
            edl: EdlScript::default(),
            inner: s.disk,
            flashed: 0,
            active: 0,
            reboots: 0,
            slot: Slot::B,
        };
        assert!(
            recovery_flow::finish(
                &mut d,
                &s.backup,
                &s.firmware,
                &mut s.state,
                &s.store,
                &mut Ui::default(),
                &mut vec![]
            )
            .is_err()
        );
        assert_eq!(d.flashed, 0);
    }
}
#[test]
fn stop_wins_before_any_original_restore_or_firmware_write() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.transact().unwrap();
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
    let mut d = Finish {
        edl: EdlScript::default(),
        inner: s.disk,
        flashed: 0,
        active: 0,
        reboots: 0,
        slot: Slot::A,
    };
    assert!(
        recovery_flow::finish(
            &mut d,
            &s.backup,
            &s.firmware,
            &mut s.state,
            &s.store,
            &mut Ui {
                deny_handoff: true,
                ..Default::default()
            },
            &mut vec![]
        )
        .is_err()
    );
    assert!(d.inner.writes.is_empty());
    assert_eq!(d.flashed, 0);
}
#[test]
fn restored_phase_can_resume_after_failure_without_touching_inactive_recovery() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    s.transact().unwrap();
    let original = s.backup.file_path("misc.img").unwrap();
    let b = std::fs::read(&original).unwrap();
    recovery_flow::restore_originals(&mut s.disk, &s.backup, &mut vec![]).unwrap();
    assert_eq!(
        s.disk
            .writes
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["recovery_b", "gpt_main4"]
    );
    assert_eq!(std::fs::read(original).unwrap(), b);
}

#[test]
fn success_and_resume_keep_only_first_original_and_verified_final_read() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    let original = s.info.bytes.clone();
    s.transact().unwrap();
    let record = s.state.recovery.as_ref().unwrap().attempt.as_ref().unwrap();
    assert_eq!(record.directory, "DeviceInfo");
    let dir = s.backup.path().join("Partition").join(&record.directory);
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["after_independent.bin", "before_shared_1.bin"]);
    assert_eq!(
        std::fs::read(dir.join("before_shared_1.bin")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read(dir.join("after_independent.bin")).unwrap(),
        s.info.bytes
    );
    assert_eq!(s.info.reads, 3);
    s.transact().unwrap();
    assert_eq!(s.info.reads, 5);
    assert_eq!(s.info.writes, 1);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
}

#[test]
fn legacy_intermediates_are_pruned_only_after_successful_independent_verification() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.fail_after_apply = true;
    assert!(s.transact().is_err());
    let record = s.state.recovery.as_ref().unwrap().attempt.as_ref().unwrap();
    let dir = s.backup.path().join("Partition").join(&record.directory);
    let before = std::fs::read(dir.join("before_shared_1.bin")).unwrap();
    let plan = deviceinfo::plan(&before, s.state.target_region, record.mode).unwrap();
    std::fs::write(dir.join("planned_shared.bin"), &plan).unwrap();
    for name in [
        "before_shared_2.bin",
        "before_logical.bin",
        "byte_changes.json",
        "after_same_session.bin",
        "after_new_session.bin",
        "read1.before.bin",
        "read2.before.bin",
        "apply.same.bin",
        "apply.fresh.bin",
        "independent.before.bin",
    ] {
        std::fs::write(dir.join(name), b"legacy intermediate").unwrap();
    }
    let planned = s.info.bytes.clone();
    s.info.bytes[0xFFF] ^= 1;
    assert!(s.transact().is_err());
    assert!(dir.join("planned_shared.bin").exists());
    assert!(!dir.join("after_independent.bin").exists());
    s.info.bytes = planned;
    s.info.fail_after_apply = false;
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
}

#[test]
fn compact_plan_hash_tampering_stops_resume_without_a_second_write() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.fail_after_apply = true;
    assert!(s.transact().is_err());
    s.state
        .recovery
        .as_mut()
        .unwrap()
        .attempt
        .as_mut()
        .unwrap()
        .planned_sha256 = "0".repeat(64);
    s.info.fail_after_apply = false;
    assert!(s.transact().unwrap_err().contains("쓰기 의도"));
    assert_eq!(s.info.writes, 1);
}

#[test]
fn compact_final_readback_tampering_blocks_original_restore_and_firmware() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    s.transact().unwrap();
    let record = s.state.recovery.as_ref().unwrap().attempt.as_ref().unwrap();
    let path = s
        .backup
        .path()
        .join("Partition")
        .join(&record.directory)
        .join("after_independent.bin");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0xFFF] ^= 1;
    std::fs::write(path, bytes).unwrap();
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
    let mut d = Finish {
        edl: EdlScript::default(),
        inner: s.disk,
        flashed: 0,
        active: 0,
        reboots: 0,
        slot: Slot::B,
    };
    assert!(
        recovery_flow::finish(
            &mut d,
            &s.backup,
            &s.firmware,
            &mut s.state,
            &s.store,
            &mut Ui::default(),
            &mut vec![]
        )
        .is_err()
    );
    assert_eq!(d.flashed, 0);
    assert!(d.inner.writes.is_empty());
}

#[test]
fn fixed_deviceinfo_directory_preserves_original_across_a_failed_read_retry() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    let original = s.info.bytes.clone();
    s.info.unstable = true;
    assert!(s.transact().is_err());
    let folder = s.backup.path().join("Partition/DeviceInfo");
    assert_eq!(
        std::fs::read(folder.join("before_shared_1.bin")).unwrap(),
        original
    );
    s.info.unstable = false;
    s.transact().unwrap();
    assert_eq!(
        s.state
            .recovery
            .as_ref()
            .unwrap()
            .attempt
            .as_ref()
            .unwrap()
            .directory,
        "DeviceInfo"
    );
    assert_eq!(
        std::fs::read(folder.join("before_shared_1.bin")).unwrap(),
        original
    );
    assert_eq!(std::fs::read_dir(folder).unwrap().count(), 2);
    assert_eq!(s.info.writes, 1);
}

#[test]
fn changed_deviceinfo_after_failed_read_never_overwrites_the_fixed_original() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    let original = s.info.bytes.clone();
    s.info.unstable = true;
    assert!(s.transact().is_err());
    s.info.unstable = false;
    s.info.bytes[0xFFF] ^= 1;
    assert!(s.transact().unwrap_err().contains("최초 원본"));
    assert_eq!(s.info.writes, 0);
    assert_eq!(
        std::fs::read(
            s.backup
                .path()
                .join("Partition/DeviceInfo/before_shared_1.bin")
        )
        .unwrap(),
        original
    );
}

#[test]
fn previous_random_deviceinfo_directory_can_still_resume_without_rewriting() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    s.transact().unwrap();
    let partition = s.backup.path().join("Partition");
    std::fs::rename(
        partition.join("DeviceInfo"),
        partition.join("deviceinfo-Legacy1"),
    )
    .unwrap();
    s.state
        .recovery
        .as_mut()
        .unwrap()
        .attempt
        .as_mut()
        .unwrap()
        .directory = "deviceinfo-Legacy1".into();
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::RegionVerified).unwrap();
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
    assert_eq!(
        std::fs::read_dir(partition.join("deviceinfo-Legacy1"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn recovery_backup_restores_only_gpt_and_the_backed_up_recovery_slot() {
    for slot in [Slot::A, Slot::B] {
        let mut s = Setup::new(slot, RollbackMode::Reset);
        let backup = VerifiedBackup::open_recovery(s.backup.path()).unwrap();
        recovery_flow::restore_originals(&mut s.disk, &backup, &mut vec![]).unwrap();
        assert_eq!(s.disk.writes.len(), 2);
        assert!(
            s.disk
                .writes
                .iter()
                .any(|p| p.name == recovery_flow::recovery_name(slot))
        );
        assert!(
            !s.disk
                .writes
                .iter()
                .any(|p| p.name.starts_with("boot") || p.name == "misc")
        );
    }
}

#[derive(Default)]
struct WaitClock(u64);
impl lapiswitch_core::workflow::Clock for WaitClock {
    fn seconds(&self) -> u64 {
        self.0
    }
    fn sleep_one_second(&mut self) {
        self.0 += 1;
    }
}

#[test]
fn edl_wait_survives_long_absence_and_a_port_lost_between_probe_and_open() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.transact().unwrap();
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
    let state = std::fs::read(s.store.path()).unwrap();
    let mut d = Finish {
        edl: EdlScript {
            ports: std::iter::repeat_n(None, 600)
                .chain([Some("stale"), None, None, Some("reconnected")])
                .collect(),
            opens: [Ok(false), Ok(true)].into(),
            ..Default::default()
        },
        inner: s.disk,
        flashed: 0,
        active: 0,
        reboots: 0,
        slot: Slot::A,
    };
    let mut clock = WaitClock::default();
    let mut log = vec![];
    recovery_flow::wait_for_edl(
        &mut d,
        &s.firmware.loader,
        &mut clock,
        &mut Ui::default(),
        &mut log,
    )
    .unwrap();
    assert!(clock.0 >= 603);
    assert_eq!(d.edl.attempts, 2);
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("EDL 연결이 늦어지고"))
            .count(),
        1
    );
    assert!(!log.iter().any(|line| line.contains("작업 중지")));
    assert!(d.inner.writes.is_empty());
    assert_eq!((d.flashed, d.active, d.reboots), (0, 0, 0));
    assert_eq!(std::fs::read(s.store.path()).unwrap(), state);
    s.backup.verify().unwrap();
}

#[test]
fn edl_wait_repeats_unstable_port_attempts_but_never_retries_fatal_handshake_errors() {
    for fatal in [false, true] {
        let s = Setup::new(Slot::B, RollbackMode::Reset);
        let mut opens =
            std::iter::repeat_n(Ok(false), 200).collect::<std::collections::VecDeque<_>>();
        opens.push_back(if fatal {
            Err("Firehose configuration failed".into())
        } else {
            Ok(true)
        });
        let mut d = Finish {
            edl: EdlScript {
                opens,
                ..Default::default()
            },
            inner: s.disk,
            flashed: 0,
            active: 0,
            reboots: 0,
            slot: Slot::B,
        };
        let mut clock = WaitClock::default();
        let result = recovery_flow::wait_for_edl(
            &mut d,
            &s.firmware.loader,
            &mut clock,
            &mut Ui::default(),
            &mut vec![],
        );
        assert_eq!(result.is_err(), fatal);
        if fatal {
            assert!(result.unwrap_err().contains("Firehose"));
        }
        assert_eq!(d.edl.attempts, 201);
        assert_eq!(clock.0, 200);
        assert!(d.inner.writes.is_empty());
        assert_eq!(d.flashed, 0);
    }
}

fn vendor_image() -> Vec<u8> {
    let mut st = vec![];
    for n in [1u32, 0, 3, 4, 0] {
        st.extend(n.to_be_bytes());
    }
    st.extend(b"PRC\0");
    st.extend(2u32.to_be_bytes());
    st.extend(9u32.to_be_bytes());
    let names = b"region_country\0";
    let size = 40 + st.len() + names.len();
    let mut tree = vec![];
    for n in [
        0xd00dfeed,
        size as u32,
        40,
        (40 + st.len()) as u32,
        40,
        17,
        16,
        0,
        names.len() as u32,
        st.len() as u32,
    ] {
        tree.extend(n.to_be_bytes());
    }
    tree.extend(st);
    tree.extend(names);
    let mut b = vec![0; 16384];
    b[..8].copy_from_slice(b"VNDRBOOT");
    for (o, v) in [
        (8, 4u32),
        (12, 4096),
        (2096, 2128),
        (2100, tree.len() as u32),
    ] {
        b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    b[4096..4096 + tree.len()].copy_from_slice(&tree);
    b
}

#[test]
fn restoration_classifies_durable_intent_and_real_final_dumps_not_file_presence() {
    use recovery_flow::restore::{self, Assessment};
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    assert_eq!(restore::assess(&s.backup).unwrap(), Assessment::BeforeWrite);
    s.info.fail_after_apply = true;
    assert!(s.transact().is_err());
    assert!(matches!(
        restore::assess(&s.backup).unwrap(),
        Assessment::Unknown(_)
    ));
    assert!(
        restore::preflight(&s.backup)
            .unwrap_err()
            .contains(restore::UNKNOWN_MESSAGE)
    );
    s.info.fail_after_apply = false;
    s.transact().unwrap();
    assert_eq!(
        restore::preflight(&s.backup).unwrap(),
        Assessment::Verified {
            original: lapiswitch_core::region::Region::Prc,
            current: lapiswitch_core::region::Region::Row
        }
    );
    let final_path = s
        .backup
        .path()
        .join("Partition/DeviceInfo/after_independent.bin");
    let good = std::fs::read(&final_path).unwrap();
    let mut corrupt = good.clone();
    corrupt[123] ^= 1;
    std::fs::write(&final_path, &corrupt).unwrap();
    assert!(matches!(
        restore::assess(&s.backup).unwrap(),
        Assessment::Unknown(_)
    ));
    std::fs::write(&final_path, good).unwrap();
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::Planned).unwrap();
    assert!(matches!(
        restore::assess(&s.backup).unwrap(),
        Assessment::Unknown(_)
    ));
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::FirmwareWriting).unwrap();
    assert!(restore::assess(&s.backup).is_err());
}

struct NeverConnect;
impl recovery_flow::restore::Connector for NeverConnect {
    type Recovery = RestoreInfo;
    fn connect(
        &mut self,
        _: &VerifiedBackup,
        _: &mut impl recovery_flow::Ui,
        _: &mut Vec<String>,
    ) -> Result<RestoreInfo, String> {
        panic!("must not connect/read DeviceInfo")
    }
}
struct RestoreInfo {
    shared: std::rc::Rc<std::cell::RefCell<(Vec<u8>, usize)>>,
    fail: bool,
    journal: PathBuf,
}
impl DeviceInfoIo for RestoreInfo {
    fn read(&mut self, _: &Path, _: &mut Vec<String>) -> Result<Vec<u8>, String> {
        Ok(self.shared.borrow().0.clone())
    }
    fn prepare(
        &mut self,
        b: &[u8],
        p: &[u8],
        mode: RollbackMode,
        _: &mut Vec<String>,
    ) -> Result<(), String> {
        assert_eq!(mode, RollbackMode::Keep);
        assert_eq!(b, self.shared.borrow().0);
        // The reverse write intent is recorded only after the dry-run.
        let j: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.journal).unwrap()).unwrap();
        assert_eq!(j["phase"], "BootRequested");
        deviceinfo::validate_plan(b, p, deviceinfo::region(p)?, mode)
    }
    fn apply(
        &mut self,
        b: &[u8],
        p: &[u8],
        mode: RollbackMode,
        _: &Path,
        _: &mut Vec<String>,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        assert_eq!(mode, RollbackMode::Keep);
        assert_eq!(b, self.shared.borrow().0);
        let j: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.journal).unwrap()).unwrap();
        assert_eq!(j["phase"], "RegionWritePending");
        assert_eq!(
            std::fs::read(self.journal.with_file_name("deviceinfo_before.bin")).unwrap(),
            b
        );
        self.shared.borrow_mut().0 = p.to_vec();
        self.shared.borrow_mut().1 += 1;
        if self.fail {
            return Err("USB loss after restoring region".into());
        }
        Ok((p.to_vec(), p.to_vec()))
    }
}
impl recovery_flow::restore::Session for RestoreInfo {
    fn verify_identity(&mut self, _: &VerifiedBackup) -> Result<(), String> {
        Ok(())
    }
    fn reboot_edl(&mut self) -> Result<(), String> {
        Ok(())
    }
}
struct RestoreConnector {
    shared: std::rc::Rc<std::cell::RefCell<(Vec<u8>, usize)>>,
    fail: bool,
    journal: PathBuf,
}
impl recovery_flow::restore::Connector for RestoreConnector {
    type Recovery = RestoreInfo;
    fn connect(
        &mut self,
        _: &VerifiedBackup,
        _: &mut impl recovery_flow::Ui,
        _: &mut Vec<String>,
    ) -> Result<RestoreInfo, String> {
        Ok(RestoreInfo {
            shared: self.shared.clone(),
            fail: self.fail,
            journal: self.journal.clone(),
        })
    }
}
struct RestoreDisk(Finish);
impl StageIo for RestoreDisk {
    fn ensure_single_target(&mut self) -> Result<(), String> {
        self.0.ensure_single_target()
    }
    fn read_avb(&mut self, n: &str, l: &mut Vec<String>) -> Result<Option<AvbSummary>, String> {
        self.0.read_avb(n, l)
    }
    fn scan(&mut self, l: &mut Vec<String>) -> Result<Vec<Partition>, String> {
        self.0.scan(l)
    }
    fn validate_snapshot(&mut self, p: &[Partition]) -> Result<(), String> {
        self.0.validate_snapshot(p)
    }
    fn confirm_proinfo_absent(&mut self, l: &mut Vec<String>) -> Result<(), String> {
        self.0.confirm_proinfo_absent(l)
    }
    fn dump_at(&mut self, p: &Partition, o: &Path, l: &mut Vec<String>) -> Result<(), String> {
        self.0.dump_at(p, o, l)
    }
    fn write_at(&mut self, p: &Partition, o: &Path, l: &mut Vec<String>) -> Result<(), String> {
        self.0.write_at(p, o, l)
    }
    fn reset(&mut self, m: ResetMode, l: &mut Vec<String>) -> Result<(), String> {
        self.0.reset(m, l)
    }
}
impl FinishIo for RestoreDisk {
    fn probe_edl(&mut self) -> Result<Option<String>, String> {
        self.0.probe_edl()
    }
    fn reboot_recovery(&mut self, _: &mut Vec<String>) -> Result<(), String> {
        self.0.reboots += 1;
        Ok(())
    }
    fn open_edl(&mut self, p: &Path, l: &mut Vec<String>) -> Result<(), String> {
        self.0.open_edl(p, l)
    }
    fn flash_firmware(
        &mut self,
        _: &FirmwareReport,
        _: bool,
        _: &mut Vec<String>,
    ) -> Result<(), String> {
        panic!("original restore never flashes firmware")
    }
    fn set_active_a(&mut self, _: &mut Vec<String>) -> Result<(), String> {
        panic!("original restore never changes slot")
    }
    fn final_reboot(&mut self, _: &mut Vec<String>) -> Option<String> {
        panic!("use system reset after verification")
    }
}
fn restore_disk(disk: Fake, slot: Slot) -> RestoreDisk {
    RestoreDisk(Finish {
        edl: EdlScript::default(),
        inner: disk,
        flashed: 0,
        active: 0,
        reboots: 0,
        slot,
    })
}

#[test]
fn prewrite_restoration_never_opens_twrp_and_unknown_never_writes_or_reboots() {
    use recovery_flow::restore;
    for unknown in [false, true] {
        let mut s = Setup::new(Slot::B, RollbackMode::Reset);
        if unknown {
            s.info.fail_after_apply = true;
            assert!(s.transact().is_err());
        }
        let mut d = restore_disk(s.disk, Slot::B);
        let r = restore::run(
            &mut d,
            &s.backup,
            &s.firmware.loader,
            None,
            &mut NeverConnect,
            &mut WaitClock::default(),
            &mut Ui::default(),
            &mut vec![],
        );
        assert_eq!(r.is_err(), unknown);
        if unknown {
            assert!(d.0.inner.writes.is_empty());
            assert!(d.0.inner.resets.is_empty());
        } else {
            assert_eq!(d.0.inner.resets, vec![ResetMode::System]);
            assert_eq!(
                s.backup.transaction_stage().unwrap(),
                TransactionStage::Restored
            );
        }
    }
}

#[test]
fn legacy_without_vendor_boot_is_accepted_only_before_a_region_change() {
    use recovery_flow::restore;
    for changed in [false, true] {
        let mut s = Setup::new(Slot::A, RollbackMode::Reset);
        if changed {
            s.transact().unwrap();
        }
        let path = s.backup.path().join("manifest.json");
        let mut m: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let boot = s
            .disk
            .snapshot
            .iter()
            .find(|p| p.name == "boot_a")
            .unwrap()
            .clone();
        let boot_path = s.backup.path().join("Partition/boot_a.img");
        std::fs::write(&boot_path, s.disk.bytes("boot_a")).unwrap();
        let (size, sha) = lapiswitch_core::backup::hash_file(&boot_path, None).unwrap();
        m["files"].as_array_mut().unwrap().push(serde_json::json!({"filename":"boot_a.img", "size_bytes":size, "sha256":sha, "partition":boot}));
        m["version"] = 3.into();
        m["operation"] = "recovery_deviceinfo_v1".into();
        m["files"]
            .as_array_mut()
            .unwrap()
            .retain(|e| !e["filename"].as_str().unwrap().starts_with("vendor_boot"));
        std::fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
        let old = VerifiedBackup::open_recovery(s.backup.path()).unwrap();
        assert_eq!(restore::preflight(&old).is_err(), changed);
    }
}

#[test]
#[ignore = "Pinned TWRP fixture; RAM/disk only. Set LAPISWITCH_TWRP_TEST_IMAGE"]
fn reverse_region_restore_preserves_indices_handles_both_slots_and_blocks_uncertain_retry() {
    use recovery_flow::restore;
    let twrp = PathBuf::from(std::env::var_os("LAPISWITCH_TWRP_TEST_IMAGE").unwrap());
    for (slot, fail) in [(Slot::A, false), (Slot::B, false), (Slot::B, true)] {
        let mut s = Setup::with_recovery_size(slot, RollbackMode::Reset, true);
        s.transact().unwrap();
        let shared = std::rc::Rc::new(std::cell::RefCell::new((s.info.bytes.clone(), 0)));
        let journal = s
            .backup
            .path()
            .join("Partition/OriginalRestore/original_restore.json");
        let mut connector = RestoreConnector {
            shared: shared.clone(),
            fail,
            journal,
        };
        let mut disk = restore_disk(s.disk, slot);
        let untouched = disk
            .0
            .inner
            .bytes(&format!("vendor_boot{}", slot.other().suffix()))
            .to_vec();
        let result = restore::run(
            &mut disk,
            &s.backup,
            &s.firmware.loader,
            Some(&twrp),
            &mut connector,
            &mut WaitClock::default(),
            &mut Ui::default(),
            &mut vec![],
        );
        assert_eq!(shared.borrow().1, 1);
        assert_eq!(
            deviceinfo::region(&shared.borrow().0).unwrap(),
            lapiswitch_core::region::Region::Prc
        );
        assert_eq!(deviceinfo::indices(&shared.borrow().0).unwrap(), [0; 32]);
        assert_eq!(
            disk.0
                .inner
                .bytes(&format!("vendor_boot{}", slot.other().suffix())),
            untouched
        );
        if fail {
            assert!(result.is_err());
            assert!(disk.0.inner.resets.is_empty());
            let writes = disk.0.inner.writes.len();
            assert!(
                restore::preflight(&s.backup)
                    .unwrap_err()
                    .contains(restore::UNKNOWN_MESSAGE)
            );
            assert!(
                restore::run(
                    &mut disk,
                    &s.backup,
                    &s.firmware.loader,
                    Some(&twrp),
                    &mut connector,
                    &mut WaitClock::default(),
                    &mut Ui::default(),
                    &mut vec![]
                )
                .is_err()
            );
            assert_eq!(disk.0.inner.writes.len(), writes);
            assert_eq!(shared.borrow().1, 1);
        } else {
            result.unwrap();
            assert_eq!(disk.0.inner.resets, vec![ResetMode::System]);
            assert_eq!(
                disk.0.inner.bytes(&format!("vendor_boot{}", slot.suffix())),
                vendor_image()
            );
            assert!(
                disk.0
                    .inner
                    .bytes(&recovery_flow::recovery_name(slot))
                    .iter()
                    .all(|b| *b == 0x77)
            );
            assert_eq!(
                s.backup.transaction_stage().unwrap(),
                TransactionStage::Restored
            );
            assert!(restore::ensure_conversion_allowed(&s.backup).is_err());
            // Retry finalization only: no TWRP boot or DeviceInfo rewrite.
            restore::run(
                &mut disk,
                &s.backup,
                &s.firmware.loader,
                None,
                &mut NeverConnect,
                &mut WaitClock::default(),
                &mut Ui::default(),
                &mut vec![],
            )
            .unwrap();
            assert_eq!(shared.borrow().1, 1);
        }
    }
}

#[test]
fn reverse_proof_resumes_only_partition_restoration_and_corruption_blocks_all_io() {
    use lapiswitch_core::region::Region;
    use recovery_flow::restore;
    for (phase, corrupt) in [
        ("RegionWritePending", false),
        ("RegionVerified", false),
        ("RegionVerified", true),
    ] {
        let mut s = Setup::new(Slot::B, RollbackMode::Reset);
        s.transact().unwrap();
        let before = s.info.bytes.clone();
        let after = deviceinfo::plan(&before, Region::Prc, RollbackMode::Keep).unwrap();
        let dir = s.backup.path().join("Partition/OriginalRestore");
        std::fs::create_dir(&dir).unwrap();
        let before_path = dir.join("deviceinfo_before.bin");
        let after_path = dir.join("deviceinfo_after.bin");
        std::fs::write(&before_path, &before).unwrap();
        std::fs::write(&after_path, &after).unwrap();
        let j = serde_json::json!({"version":1,
            "manifest_sha256":lapiswitch_core::backup::hash_file(&s.backup.path().join("manifest.json"),None).unwrap().1,
            "progress_sha256":lapiswitch_core::backup::hash_file(&s.backup.path().join("recovery_progress.json"),None).unwrap().1,
            "phase":phase,
            "before_sha256":lapiswitch_core::backup::hash_file(&before_path,None).unwrap().1,
            "planned_sha256":lapiswitch_core::backup::hash_file(&after_path,None).unwrap().1});
        std::fs::write(
            dir.join("original_restore.json"),
            serde_json::to_vec(&j).unwrap(),
        )
        .unwrap();
        if corrupt {
            let mut bad = after.clone();
            bad[100] ^= 1;
            std::fs::write(&after_path, bad).unwrap();
        }
        let vb = s
            .disk
            .snapshot
            .iter()
            .find(|p| p.name == "vendor_boot_b")
            .unwrap();
        let start = vb.start_sector as usize * 4096;
        s.disk.disk[4][start..start + 16384].copy_from_slice(
            &lapiswitch_core::vendor_boot::patch(&vendor_image(), Region::Row).unwrap(),
        );
        s.backup.mark(TransactionStage::RecoveryRequired).unwrap();
        let mut disk = restore_disk(s.disk, Slot::B);
        let result = restore::run(
            &mut disk,
            &s.backup,
            &s.firmware.loader,
            None,
            &mut NeverConnect,
            &mut WaitClock::default(),
            &mut Ui::default(),
            &mut vec![],
        );
        assert_eq!(result.is_err(), corrupt);
        if corrupt {
            assert!(disk.0.inner.writes.is_empty());
            assert!(disk.0.inner.resets.is_empty());
        } else {
            assert_eq!(disk.0.inner.bytes("vendor_boot_b"), vendor_image());
            assert_eq!(disk.0.inner.resets, vec![ResetMode::System]);
        }
    }
}

#[test]
fn missing_reverse_journal_or_conversion_journal_is_never_a_clean_prewrite_state() {
    use recovery_flow::restore;
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.transact().unwrap();
    let dir = s.backup.path().join("Partition/OriginalRestore");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("deviceinfo_before.bin"), &s.info.bytes).unwrap();
    assert!(
        restore::preflight(&s.backup)
            .unwrap_err()
            .contains(restore::UNKNOWN_MESSAGE)
    );
    std::fs::remove_file(s.backup.path().join("recovery_progress.json")).unwrap();
    assert!(matches!(
        restore::assess(&s.backup).unwrap(),
        restore::Assessment::Unknown(_)
    ));
}

#[test]
fn v4_vendor_boot_is_mandatory_and_bound_to_the_backed_up_slot() {
    for slot in [Slot::A, Slot::B] {
        let s = Setup::new(slot, RollbackMode::Reset);
        let p = s.backup.path().join("manifest.json");
        let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        let name = format!("vendor_boot{}.img", slot.suffix());
        assert!(s.backup.file_path(&name).unwrap().is_file());
        assert!(
            s.backup
                .file_path(&format!("vendor_boot{}.img", slot.other().suffix()))
                .is_err()
        );
        m["files"]
            .as_array_mut()
            .unwrap()
            .retain(|e| e["filename"] != name);
        std::fs::write(&p, serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(VerifiedBackup::open_recovery(s.backup.path()).is_err());
    }
}

#[test]
fn s3_device_version_gate_uses_active_slot_before_partition_backup() {
    for slot in [Slot::A, Slot::B] {
        for (version, allowed) in [
            ("ZUXOS_1.5.10.186_260408_PRC", true),
            ("ZUXOS_1.5.10.187_260408_PRC", false),
            ("ZUI_18.5.10.037_260408_ROW", true),
            ("ZUI_18.5.10.038_260408_ROW", false),
            ("ZUI_16.1.10.027_241129_PRC", true),
            ("UNKNOWN_ROW", false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut d = Fake::new(600, true);
            d.snapshot.extend([
                part("recovery_a", 4, 1600, 2),
                part("recovery_b", 4, 1602, 2),
                part("misc", 0, 70, 2),
                part("vendor_boot_a", 4, 1700, 4),
                part("vendor_boot_b", 4, 1710, 4),
            ]);
            d.original_gpt = gpt(&d.snapshot);
            d.disk[4][..d.original_gpt.len()].copy_from_slice(&d.original_gpt);
            // Fail immediately if any retained boot backup is attempted.
            d.fail_dump = Some(format!("boot{}", slot.suffix()));
            d.fingerprints.insert(
                format!("vbmeta_system{}", slot.suffix()),
                format!("qti/TB520FU/TB520FU:16/BUILD/{version}:user/release-keys"),
            );
            d.fingerprints.insert(
                format!("vbmeta_system{}", slot.other().suffix()),
                "qti/TB520FU/TB520FU:16/BUILD/ZUI_99.0.0.0_260408_ROW:user/release-keys".into(),
            );
            let builder = BackupBuilder::create(dir.path()).unwrap().for_recovery();
            let folder = builder.partition_dir().to_owned();
            let result = stage1::take_backup(
                &mut d,
                &prepared(slot, lapiswitch_core::prepare::Connection::Fastboot),
                &firmware(),
                builder,
                &mut vec![],
            );
            assert_eq!(
                result.is_ok(),
                allowed,
                "{slot:?} {version}: {:?}",
                result.as_ref().err()
            );
            assert!(d.writes.is_empty());
            if !allowed {
                assert_eq!(std::fs::read_dir(folder).unwrap().count(), 0);
            }
        }
    }
}
#[test]
fn prior_v4_backups_still_require_boot_and_remain_readable() {
    let s = Setup::new(Slot::B, RollbackMode::Reset);
    let path = s.backup.path().join("manifest.json");
    let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    m["version"] = 4.into();
    m["operation"] = "recovery_deviceinfo_v2".into();
    std::fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(VerifiedBackup::open_recovery(s.backup.path()).is_err());
    let boot = s
        .disk
        .snapshot
        .iter()
        .find(|p| p.name == "boot_b")
        .unwrap()
        .clone();
    let file = s.backup.path().join("Partition/boot_b.img");
    std::fs::write(&file, s.disk.bytes("boot_b")).unwrap();
    let (size, sha) = lapiswitch_core::backup::hash_file(&file, None).unwrap();
    m["files"].as_array_mut().unwrap().push(serde_json::json!({"filename":"boot_b.img", "size_bytes":size, "sha256":sha, "partition":boot}));
    std::fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    let legacy = VerifiedBackup::open_recovery(s.backup.path()).unwrap();
    assert!(recovery_flow::restore::preflight(&legacy).is_ok());
}

#[test]
fn edl_backups_bind_the_gpt_slot_and_do_not_require_fastboot_getvar() {
    for slot in [Slot::A, Slot::B] {
        for changed in [false, true] {
            let s = Setup::new(slot, RollbackMode::Reset);
            let mut disk = s.disk;
            let mut gpt = disk.original_gpt.clone();
            let actual = if changed { slot.other() } else { slot };
            let boot = format!("boot{}", actual.suffix());
            let index = disk
                .snapshot
                .iter()
                .filter(|p| p.lun == 4)
                .position(|p| p.name == boot)
                .unwrap();
            let attr = (3u64 << 48) | (1 << 50) | (7 << 51);
            gpt[8192 + index * 128 + 48..8192 + index * 128 + 56]
                .copy_from_slice(&attr.to_le_bytes());
            let crc = crc32fast::hash(&gpt[8192..5 * 4096]);
            gpt[4096 + 88..4096 + 92].copy_from_slice(&crc.to_le_bytes());
            refresh_header(&mut gpt);
            disk.disk[4][..gpt.len()].copy_from_slice(&gpt);
            let builder = BackupBuilder::create(s._dir.path()).unwrap().for_recovery();
            let backup = stage1::take_backup(
                &mut disk,
                &prepared(slot, lapiswitch_core::prepare::Connection::Edl),
                &s.firmware,
                builder,
                &mut vec![],
            );
            assert_eq!(backup.is_ok(), !changed);
            if let Ok(backup) = backup {
                assert!(backup.manifest().started_in_edl);
                assert!(!backup.manifest().slot_estimated);
                assert_eq!(backup.manifest().slot, slot);
                assert!(backup.file_path("getvar.txt").is_err());
                backup.verify().unwrap();
            }
        }
    }
}
