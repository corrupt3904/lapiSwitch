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
fn firmware_boundary_blocks_original_restore() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
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
    device::Device,
    deviceinfo::{self, RollbackMode, map},
    firmware::FirmwareReport,
    recovery_flow::{self, DeviceInfoIo, EditRequest, Outcome, Phase, Transition},
    rollback::Downgrade,
    slot::Slot,
    stage1::{self, ResetMode, StageIo},
    state::{FlashOptions, Request, RunState, StateStore},
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
    /// Answer to the Keep-mode rollback prompt; `None` closes it.
    rollback: Option<bool>,
    asked: Vec<Vec<Downgrade>>,
    /// Answer of the DeviceInfo editor; `None` cancels.
    edit: Option<EditRequest>,
    edits_shown: usize,
}
impl recovery_flow::Ui for Ui {
    fn edit_deviceinfo(
        &mut self,
        layout: map::Layout,
        before: &[u8],
    ) -> Result<Option<EditRequest>, String> {
        layout.check_buffer(before)?;
        self.edits_shown += 1;
        Ok(self.edit.clone())
    }
    fn phase(&mut self, phase: Phase) {
        self.phases.push(phase);
    }
    fn confirm(&mut self, _: Transition) -> Result<(), String> {
        Ok(())
    }
    fn confirm_rollback_reset(&mut self, downgrades: &[Downgrade]) -> Result<bool, String> {
        self.asked.push(downgrades.to_vec());
        self.rollback.ok_or_else(|| "rollback prompt closed".into())
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
    sample_for(tb520fu(), r)
}
/// Synthetic DeviceInfo shaped like `device`'s map.
fn sample_for(device: Device, r: lapiswitch_core::region::Region) -> Vec<u8> {
    let at = device.layout().region_offset();
    let mut b = vec![0x5A; deviceinfo::SHARED_SIZE];
    b[..13].copy_from_slice(b"ANDROID-BOOT!");
    b[0xCC8..0xCD0].copy_from_slice(b"deadbeef");
    b[at..at + 0x10].fill(0);
    b[at..at + 3].copy_from_slice(r.token().as_bytes());
    b[at + 0x10..at + 0x18].copy_from_slice(b"beefdead");
    b
}
fn tb522fu() -> Device {
    Device::find("TB522FU").unwrap()
}
fn mode(state: &RunState) -> RollbackMode {
    match state.request {
        Request::Convert { rollback_mode, .. } => rollback_mode,
        Request::Edit => panic!("not a conversion"),
    }
}
struct Info {
    layout: map::Layout,
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
        _: &mut Vec<String>,
    ) -> Result<(), String> {
        // No durable write intent may exist while transfers can still fail.
        assert_eq!(self.phase(), Phase::Planned);
        assert_eq!(before, self.bytes);
        map::check_plan(self.layout, before, planned)?;
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
        out: &Path,
        _: &mut Vec<String>,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        assert!(std::mem::take(&mut self.prepared), "apply without prepare");
        assert_eq!(self.phase(), Phase::WritePending);
        assert!(self.backup_dir.join("recovery_progress.json").is_file());
        let state = StateStore::new(self.state_file.clone()).load()?.unwrap();
        let record = state.recovery.attempt.unwrap();
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
        map::check_plan(self.layout, &self.bytes, &p)?;
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
    /// The firmware the run installs: set for conversions, chosen in the
    /// editor for an edit that changes the region.
    chosen: Option<FirmwareReport>,
    state: RunState,
    store: StateStore,
    info: Info,
    /// Rollback prompt answer and every prompt shown across `transact` calls.
    rollback: Option<bool>,
    asked: Vec<Vec<Downgrade>>,
    /// Editor answer and how often the editor was shown.
    edit: Option<EditRequest>,
    edits_shown: usize,
}
impl Setup {
    fn new(slot: Slot, mode: RollbackMode) -> Self {
        Self::with_recovery_size(slot, mode, false)
    }
    fn with_recovery_size(slot: Slot, mode: RollbackMode, large: bool) -> Self {
        Self::build(slot, Some(mode), large)
    }
    /// A DeviceInfo edit run; values are chosen through `edit`.
    fn edit(slot: Slot) -> Self {
        Self::build(slot, None, false)
    }
    fn build(slot: Slot, mode: Option<RollbackMode>, large: bool) -> Self {
        Self::build_for(tb520fu(), slot, mode, large)
    }
    /// A DeviceInfo edit on another supported device.
    fn edit_for(device: Device, slot: Slot) -> Self {
        Self::build_for(device, slot, None, false)
    }
    fn build_for(device: Device, slot: Slot, mode: Option<RollbackMode>, large: bool) -> Self {
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
        let mut disk = Fake::with_recovery(600, true);
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
        if device == tb522fu() {
            // Its country partition (LUN 0, field CNXX at +0xE4, with a LUN 0
            // GPT naming it) and build-date fingerprints.
            fw.fingerprint =
                "qti/TB522FU/TB522FU:17/CQ2A.260712.001/11.055.260904W:user/release-keys".into();
            disk.disk[0].resize(500 * 4096, 0xA7);
            let field = 100 * 4096 + 0xE4;
            disk.disk[0][field..field + 32].fill(0);
            disk.disk[0][field..field + 4].copy_from_slice(b"CNXX");
            disk.snapshot.push(part("oemowninfo", 0, 100, 256));
            let lun0: Vec<_> = disk
                .snapshot
                .iter()
                .filter(|p| p.lun == 0)
                .cloned()
                .map(|mut p| {
                    p.lun = 4;
                    p
                })
                .collect();
            let gpt0 = gpt(&lun0);
            disk.disk[0][..gpt0.len()].copy_from_slice(&gpt0);
            for s in ["a", "b"] {
                disk.fingerprints.insert(
                    format!("vbmeta_system_{s}"),
                    "qti/TB522FU/TB522FU:17/CQ2A.260712.001/10.028.260924C:user/release-keys"
                        .into(),
                );
            }
        }
        fw.device = device;
        let builder = BackupBuilder::create(dir.path(), Some(device)).unwrap();
        let mut state = match mode {
            Some(mode) => {
                RunState::convert(&fw, mode, FlashOptions::new(None).unwrap(), builder.path())
                    .unwrap()
            }
            None => RunState::edit(Some(device), &fw.loader, builder.path()).unwrap(),
        };
        let store = StateStore::new(dir.path().join("state.json"));
        store.begin(&state).unwrap();
        let mut report = prepared(slot, lapiswitch_core::prepare::Connection::Adb);
        report.device = device;
        report.identity = Some(lapiswitch_core::prepare::identity_record(
            device, slot, "FAKE0001",
        ));
        let backup = stage1::take_backup(&mut disk, &report, builder, &mut vec![]).unwrap();
        state.bind_backup(&backup).unwrap();
        backup.mark(TransactionStage::S4Done).unwrap();
        recovery_flow::checkpoint(&mut state, &store, Phase::RecoveryWaiting).unwrap();
        let info = Info {
            layout: device.layout(),
            bytes: sample_for(device, lapiswitch_core::region::Region::Prc),
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
            chosen: mode.map(|_| fw.clone()),
            firmware: fw,
            state,
            store,
            info,
            rollback: None,
            asked: vec![],
            edit: None,
            edits_shown: 0,
        }
    }
    fn outcome(&mut self) -> Result<Outcome, String> {
        let mut ui = Ui {
            rollback: self.rollback,
            edit: self.edit.clone(),
            ..Ui::default()
        };
        let result = recovery_flow::transact(
            &mut self.info,
            &self.backup,
            &mut self.chosen,
            &mut self.state,
            &self.store,
            &mut ui,
            &mut vec![],
        );
        self.asked.extend(ui.asked);
        self.edits_shown += ui.edits_shown;
        result
    }
    /// A cancelled rollback prompt is not a verified transaction.
    fn transact(&mut self) -> Result<(), String> {
        match self.outcome()? {
            Outcome::Verified => Ok(()),
            Outcome::Cancelled => Err("CANCELLED".into()),
        }
    }
}
#[test]
fn backup_records_active_recovery_vendor_boot_and_misc_without_boot() {
    for slot in [Slot::A, Slot::B] {
        let s = Setup::new(slot, RollbackMode::Reset);
        assert_eq!(
            s.backup.manifest().version,
            lapiswitch_core::backup::MANIFEST_VERSION
        );
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
    assert_eq!(
        deviceinfo::indices(tb520fu().layout(), &s.info.bytes),
        [0; 32]
    );
    assert_eq!(s.info.bytes, conversion(&before, &s, RollbackMode::Reset));
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
}
/// Region and optional rollback reset only; every other byte as read.
fn conversion(before: &[u8], s: &Setup, mode: RollbackMode) -> Vec<u8> {
    let edits =
        deviceinfo::conversion_edits(tb520fu().layout(), before, s.firmware.region, mode).unwrap();
    let planned = map::apply(tb520fu().layout(), before, &edits).unwrap();
    for (n, (a, b)) in before.iter().zip(&planned).enumerate() {
        if a != b {
            assert!(
                (0xD10..0xD20).contains(&n) || (0x898..0x998).contains(&n),
                "unexpected change at {n:#x}"
            );
        }
    }
    planned
}
/// boot = index 3, vbmeta_system = index 2, both little-endian u64 in DeviceInfo.
fn store_indices(bytes: &mut [u8], boot: u64, vbmeta_system: u64) {
    bytes[0x898 + 3 * 8..0x898 + 4 * 8].copy_from_slice(&boot.to_le_bytes());
    bytes[0x898 + 2 * 8..0x898 + 3 * 8].copy_from_slice(&vbmeta_system.to_le_bytes());
}
fn saved_state(s: &Setup) -> RunState {
    s.store.load().unwrap().unwrap()
}
#[test]
fn keep_lower_firmware_cancel_writes_nothing_and_stays_before_write() {
    use recovery_flow::restore::{self, Assessment};
    let mut s = Setup::new(Slot::A, RollbackMode::Keep);
    s.rollback = Some(false);
    assert_eq!(s.outcome().unwrap(), Outcome::Cancelled);
    assert_eq!(s.asked.len(), 1);
    // The sample stores 0x5A.. in every index, above the firmware's 20.
    assert_eq!(
        s.asked[0].iter().map(|d| d.partition).collect::<Vec<_>>(),
        ["boot", "vbmeta_system"]
    );
    assert!(s.asked[0].iter().all(|d| d.firmware == 20));
    assert_eq!(s.info.writes, 0);
    assert!(!s.info.prepared);
    let saved = saved_state(&s);
    assert_eq!(mode(&saved), RollbackMode::Keep);
    let progress = saved.recovery;
    assert_eq!(progress.phase, Phase::RecoveryWaiting);
    assert!(progress.attempt.is_none());
    assert_eq!(restore::assess(&s.backup).unwrap(), Assessment::BeforeWrite);
}
#[test]
fn keep_lower_firmware_reset_answer_persists_reset_and_zeroes_all_indices() {
    use recovery_flow::restore::{self, Assessment};
    let mut s = Setup::new(Slot::B, RollbackMode::Keep);
    let before = s.info.bytes.clone();
    s.rollback = Some(true);
    s.transact().unwrap();
    assert_eq!(s.asked.len(), 1);
    assert_eq!(s.info.writes, 1);
    assert_eq!(
        deviceinfo::indices(tb520fu().layout(), &s.info.bytes),
        [0; 32]
    );
    assert_eq!(s.info.bytes, conversion(&before, &s, RollbackMode::Reset));
    // The reset is part of the recorded plan; the request keeps its choice.
    let saved = saved_state(&s);
    assert_eq!(mode(&saved), RollbackMode::Keep);
    let edits = &saved.recovery.attempt.as_ref().unwrap().edits;
    assert!((0..32).all(|n| edits.contains_key(&format!("rollback_index[{n}]"))));
    assert!(
        saved
            .warnings
            .iter()
            .any(|w| w.contains(recovery_flow::ROLLBACK_LOWER_MESSAGE))
    );
    // The confirmation re-read after the EDL prompt never asks again.
    s.transact().unwrap();
    assert_eq!(s.asked.len(), 1);
    assert_eq!(s.info.writes, 1);
    assert!(matches!(
        restore::assess(&s.backup).unwrap(),
        Assessment::Verified { .. }
    ));
}
#[test]
fn closing_the_rollback_prompt_stops_without_writes_or_mode_change() {
    let mut s = Setup::new(Slot::A, RollbackMode::Keep);
    assert_eq!(s.transact().unwrap_err(), "rollback prompt closed");
    assert_eq!(s.asked.len(), 1);
    assert_eq!(s.info.writes, 0);
    let saved = saved_state(&s);
    assert_eq!(mode(&saved), RollbackMode::Keep);
    assert_eq!(saved.recovery.phase, Phase::RecoveryWaiting);
    // A later resume asks again and may continue.
    s.rollback = Some(true);
    s.transact().unwrap();
    assert_eq!(s.asked.len(), 2);
    assert_eq!(s.info.writes, 1);
}
#[test]
fn keep_with_equal_or_higher_firmware_never_prompts_and_preserves_indices() {
    for (boot, vbmeta_system) in [(20, 20), (0, 0), (19, 0), (0, 19)] {
        let mut s = Setup::new(Slot::A, RollbackMode::Keep);
        // Unrelated locations are not compared, even when far above 20.
        s.info.bytes[0x898..0x998].fill(0x5A);
        store_indices(&mut s.info.bytes, boot, vbmeta_system);
        let before = deviceinfo::indices(tb520fu().layout(), &s.info.bytes);
        s.transact().unwrap();
        assert!(s.asked.is_empty(), "{boot}/{vbmeta_system}");
        assert_eq!(s.info.writes, 1);
        assert_eq!(
            deviceinfo::indices(tb520fu().layout(), &s.info.bytes),
            before
        );
        assert_eq!(mode(&saved_state(&s)), RollbackMode::Keep);
    }
}
#[test]
fn one_lower_location_is_enough_to_prompt() {
    for (boot, vbmeta_system, partition) in [(21, 0, "boot"), (0, 21, "vbmeta_system")] {
        let mut s = Setup::new(Slot::A, RollbackMode::Keep);
        store_indices(&mut s.info.bytes, boot, vbmeta_system);
        s.rollback = Some(false);
        assert_eq!(s.outcome().unwrap(), Outcome::Cancelled);
        assert_eq!(s.asked[0].len(), 1);
        assert_eq!(s.asked[0][0].partition, partition);
        assert_eq!(s.asked[0][0].floor, 21);
    }
}
#[test]
fn reset_mode_never_compares_or_prompts() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.transact().unwrap();
    assert!(s.asked.is_empty());
    assert_eq!(
        deviceinfo::indices(tb520fu().layout(), &s.info.bytes),
        [0; 32]
    );
}
#[test]
fn same_target_keep_is_noop_but_reset_must_clear_existing_indices() {
    for mode in [RollbackMode::Keep, RollbackMode::Reset] {
        let mut s = Setup::new(Slot::A, mode);
        s.info.bytes = sample(s.firmware.region);
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
    assert_eq!(s.state.recovery.phase, Phase::WritePending);
    s.info.fail_after_apply = false;
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
    assert_eq!(s.state.recovery.phase, Phase::RegionVerified);
}
#[test]
fn unknown_or_corrupt_readback_stays_pending_without_auto_retry() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.corrupt = true;
    assert!(s.transact().is_err());
    s.info.bytes[0xFFF] ^= 1;
    assert!(s.transact().is_err());
    assert_eq!(s.info.writes, 1);
    assert_eq!(s.state.recovery.phase, Phase::WritePending);
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
fn phase(s: &Setup) -> Phase {
    let phase = s.state.recovery.phase;
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
        // Originals come first; a resumed install may follow a country write.
        assert_eq!(
            self.inner
                .writes
                .iter()
                .take(2)
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
            Some(s.firmware.clone()),
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
        assert_eq!(s.state.recovery.phase, Phase::Done);
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
                Some(s.firmware.clone()),
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
            Some(s.firmware.clone()),
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
    let record = s.state.recovery.attempt.as_ref().unwrap();
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
fn compact_plan_hash_tampering_stops_resume_without_a_second_write() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.info.fail_after_apply = true;
    assert!(s.transact().is_err());
    s.state.recovery.attempt.as_mut().unwrap().planned_sha256 = "0".repeat(64);
    s.info.fail_after_apply = false;
    assert!(s.transact().unwrap_err().contains("쓰기 의도"));
    assert_eq!(s.info.writes, 1);
}

#[test]
fn compact_final_readback_tampering_blocks_original_restore_and_firmware() {
    let mut s = Setup::new(Slot::B, RollbackMode::Reset);
    s.transact().unwrap();
    let record = s.state.recovery.attempt.as_ref().unwrap();
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
            Some(s.firmware.clone()),
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
        s.state.recovery.attempt.as_ref().unwrap().directory,
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
fn recovery_backup_restores_only_gpt_and_the_backed_up_recovery_slot() {
    for slot in [Slot::A, Slot::B] {
        let mut s = Setup::new(slot, RollbackMode::Reset);
        let backup = VerifiedBackup::open(s.backup.path()).unwrap();
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
fn edl_wait_recovers_a_port_lost_between_probe_and_open_before_the_deadline() {
    let mut s = Setup::new(Slot::A, RollbackMode::Reset);
    s.transact().unwrap();
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
    let state = std::fs::read(s.store.path()).unwrap();
    let mut d = Finish {
        edl: EdlScript {
            ports: std::iter::repeat_n(None, 35)
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
    assert_eq!(clock.0, 38);
    assert_eq!(d.edl.attempts, 2);
    assert_eq!(
        log.iter()
            .filter(|line| line.contains("EDL 연결이 늦어지고"))
            .count(),
        0
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
            std::iter::repeat_n(Ok(false), 40).collect::<std::collections::VecDeque<_>>();
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
        assert_eq!(d.edl.attempts, 41);
        assert_eq!(clock.0, 40);
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
            original: Some(lapiswitch_core::region::Region::Prc),
            current: Some(lapiswitch_core::region::Region::Row),
            changed: true,
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
    fn prepare(&mut self, b: &[u8], p: &[u8], _: &mut Vec<String>) -> Result<(), String> {
        assert_eq!(b, self.shared.borrow().0);
        // The reverse write intent is recorded only after the dry-run.
        let j: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.journal).unwrap()).unwrap();
        assert_eq!(j["phase"], "BootRequested");
        map::check_plan(tb520fu().layout(), b, p)
    }
    fn apply(
        &mut self,
        b: &[u8],
        p: &[u8],
        _: &Path,
        _: &mut Vec<String>,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
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

fn twrp_session(s: &Setup) -> RestoreInfo {
    RestoreInfo {
        shared: std::rc::Rc::new(std::cell::RefCell::new((s.info.bytes.clone(), 0))),
        fail: false,
        journal: s.backup.path().join("unused.json"),
    }
}

#[test]
fn rollback_cancel_reboots_to_edl_restores_originals_and_archives_the_run() {
    for slot in [Slot::A, Slot::B] {
        let mut s = Setup::new(slot, RollbackMode::Keep);
        s.rollback = Some(false);
        assert_eq!(s.outcome().unwrap(), Outcome::Cancelled);
        let mut twrp = twrp_session(&s);
        let mut d = restore_disk(s.disk, slot);
        let mut ui = Ui::default();
        let warnings = recovery_flow::cancel_to_original(
            &mut d,
            &mut twrp,
            &s.backup,
            &s.firmware.loader,
            &s.store,
            &mut WaitClock::default(),
            &mut ui,
            &mut vec![],
        )
        .unwrap();
        // Only verified originals are written; DeviceInfo was never touched.
        let written: Vec<_> = d.0.inner.writes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            written,
            [recovery_flow::recovery_name(slot).as_str(), "gpt_main4"]
        );
        assert_eq!(d.0.inner.resets, vec![ResetMode::System]);
        assert_eq!(twrp.shared.borrow().1, 0);
        assert_eq!(
            s.backup.transaction_stage().unwrap(),
            TransactionStage::Restored
        );
        assert!(ui.phases.contains(&Phase::EdlWaiting));
        assert!(ui.phases.contains(&Phase::Restoring));
        // The active run is archived, so a new conversion can start.
        assert!(s.store.load().unwrap().is_none());
        assert!(
            std::fs::read_dir(s.store.path().parent().unwrap().join("history"))
                .unwrap()
                .next()
                .is_some()
        );
        let message = recovery_flow::cancelled(Ok(warnings));
        assert!(message.starts_with(recovery_flow::ROLLBACK_CANCELLED_MESSAGE));
        assert!(message.contains("복구·검증"));
    }
}

#[test]
fn stopping_the_edl_wait_after_a_rollback_cancel_writes_nothing_and_keeps_the_run() {
    let mut s = Setup::new(Slot::A, RollbackMode::Keep);
    s.rollback = Some(false);
    assert_eq!(s.outcome().unwrap(), Outcome::Cancelled);
    let mut twrp = twrp_session(&s);
    let mut d = restore_disk(s.disk, Slot::A);
    let error = recovery_flow::cancel_to_original(
        &mut d,
        &mut twrp,
        &s.backup,
        &s.firmware.loader,
        &s.store,
        &mut WaitClock::default(),
        &mut Ui {
            cancel: true,
            ..Ui::default()
        },
        &mut vec![],
    )
    .unwrap_err();
    assert!(d.0.inner.writes.is_empty());
    assert!(d.0.inner.resets.is_empty());
    assert!(s.store.load().unwrap().is_some());
    let message = recovery_flow::cancelled(Err(error));
    assert!(message.starts_with(recovery_flow::ROLLBACK_CANCELLED_MESSAGE));
    assert!(message.contains("EDL로 연결해 원본 복구"));
    // The manual restore remains available for the preserved backup.
    assert_eq!(
        recovery_flow::restore::preflight(&s.backup).unwrap(),
        recovery_flow::restore::Assessment::BeforeWrite
    );
}

#[test]
#[ignore = "Pinned TWRP fixture; RAM/disk only. Set LAPISWITCH_TWRP_TEST_IMAGE"]
fn reverse_restore_writes_the_full_original_handles_both_slots_and_blocks_uncertain_retry() {
    use recovery_flow::restore;
    let twrp = PathBuf::from(std::env::var_os("LAPISWITCH_TWRP_TEST_IMAGE").unwrap());
    for (slot, fail) in [(Slot::A, false), (Slot::B, false), (Slot::B, true)] {
        let mut s = Setup::with_recovery_size(slot, RollbackMode::Reset, true);
        let original = s.info.bytes.clone();
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
            deviceinfo::region(tb520fu().layout(), &shared.borrow().0).unwrap(),
            lapiswitch_core::region::Region::Prc
        );
        // The complete original, rollback indices included.
        assert_eq!(shared.borrow().0, original);
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
        let after = s.info.bytes.clone();
        s.transact().unwrap();
        let before = s.info.bytes.clone();
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
            &lapiswitch_core::vendor_boot::patch(
                lapiswitch_core::vendor_boot::RegionMarker::RegionCountry,
                &vendor_image(),
                Region::Row,
            )
            .unwrap(),
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
fn vendor_boot_is_mandatory_and_bound_to_the_backed_up_slot() {
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
        assert!(VerifiedBackup::open(s.backup.path()).is_err());
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
            let mut d = Fake::with_recovery(600, true);
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
            let builder = BackupBuilder::create(dir.path(), Some(tb520fu())).unwrap();
            let folder = builder.partition_dir().to_owned();
            let result = stage1::take_backup(
                &mut d,
                &prepared(slot, lapiswitch_core::prepare::Connection::Adb),
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
fn edl_backups_bind_the_gpt_slot_and_do_not_require_an_adb_identity() {
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
            let builder = BackupBuilder::create(s._dir.path(), Some(tb520fu())).unwrap();
            let backup = stage1::take_backup(
                &mut disk,
                &prepared(slot, lapiswitch_core::prepare::Connection::Edl),
                builder,
                &mut vec![],
            );
            assert_eq!(backup.is_ok(), !changed);
            if let Ok(backup) = backup {
                assert!(backup.manifest().started_in_edl);
                assert_eq!(backup.manifest().slot, slot);
                assert!(
                    backup
                        .file_path(lapiswitch_core::backup::IDENTITY_NAME)
                        .is_err()
                );
                backup.verify().unwrap();
            }
        }
    }
}

#[test]
fn edl_three_minute_timeout_preserves_evidence_and_never_restores_or_flashes() {
    for unstable in [false, true] {
        let mut s = Setup::new(Slot::B, RollbackMode::Reset);
        s.transact().unwrap();
        recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
        let state_before = std::fs::read(s.store.path()).unwrap();
        let mut d = Finish {
            edl: EdlScript {
                ports: if unstable {
                    Default::default()
                } else {
                    std::iter::repeat_n(None, 181).collect()
                },
                opens: std::iter::repeat_n(Ok(false), 181).collect(),
                ..Default::default()
            },
            inner: s.disk,
            flashed: 0,
            active: 0,
            reboots: 0,
            slot: Slot::B,
        };
        let mut clock = WaitClock::default();
        let error = recovery_flow::wait_for_edl(
            &mut d,
            &s.firmware.loader,
            &mut clock,
            &mut Ui::default(),
            &mut vec![],
        )
        .unwrap_err();
        assert!(error.contains("EDL") && error.contains("180초"));
        assert_eq!(clock.0, 180);
        assert_eq!(d.edl.attempts, if unstable { 180 } else { 0 });
        assert!(d.inner.writes.is_empty());
        assert_eq!((d.flashed, d.active, d.reboots), (0, 0, 0));
        assert_eq!(std::fs::read(s.store.path()).unwrap(), state_before);
        s.backup.verify().unwrap();
    }
}

#[test]
fn a_connection_that_returns_after_the_deadline_cannot_hand_off_to_writes() {
    struct LateClock(std::cell::Cell<u32>);
    impl lapiswitch_core::workflow::Clock for LateClock {
        fn seconds(&self) -> u64 {
            let n = self.0.get();
            self.0.set(n + 1);
            if n < 2 { 0 } else { 180 }
        }
        fn sleep_one_second(&mut self) {
            panic!("late connection must stop immediately");
        }
    }
    let s = Setup::new(Slot::A, RollbackMode::Reset);
    let mut d = Finish {
        edl: EdlScript::default(),
        inner: s.disk,
        flashed: 0,
        active: 0,
        reboots: 0,
        slot: Slot::A,
    };
    assert!(
        recovery_flow::wait_for_edl(
            &mut d,
            &s.firmware.loader,
            &mut LateClock(std::cell::Cell::new(0)),
            &mut Ui::default(),
            &mut vec![]
        )
        .unwrap_err()
        .contains("180초")
    );
    assert_eq!(d.edl.attempts, 1);
    assert!(d.inner.writes.is_empty());
    for transition in [Transition::Twrp, Transition::Edl] {
        assert!(recovery_flow::check_connection_timeout(transition, 179).is_ok());
        assert!(recovery_flow::check_connection_timeout(transition, 180).is_err());
    }
}

#[test]
fn s3_system_ext_version_and_custom_rom_consent_are_separate_from_stock_ceiling() {
    for slot in [Slot::A, Slot::B] {
        for (incremental, answer, expected, prompts) in [
            ("ZUI_18.5.10.037_260929_ROW", false, true, 0),
            ("ZUI_18.5.10.038_260929_ROW", true, false, 0),
            ("ZUXOS_1.5.10.187_260929_PRC", true, false, 0),
            ("GSI_TEST", false, false, 1),
            ("GSI_TEST", true, true, 1),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut d = Fake::with_recovery(600, true);
            let name = format!("vbmeta_system{}", slot.suffix());
            d.fingerprints.insert(
                name.clone(),
                "qti/TB520FU/TB520FU:17/CQ2A.260712.001/37:user/release-keys".into(),
            );
            let ext =
                format!("qti/TB520FU/TB520FU:17/CQ2A.260712.001/{incremental}:user/release-keys");
            d.system_ext_fingerprints.insert(name, ext.clone());
            let builder = BackupBuilder::create(dir.path(), Some(tb520fu())).unwrap();
            let folder = builder.partition_dir().to_owned();
            let mut called = 0;
            let result = stage1::take_backup_with_confirmation(
                &mut d,
                &prepared(slot, lapiswitch_core::prepare::Connection::Adb),
                builder,
                &mut || {
                    called += 1;
                    Ok(answer)
                },
                &mut vec![],
            );
            assert_eq!(called, prompts);
            assert_eq!(
                result.is_ok(),
                expected,
                "{incremental} {answer}: {:?}",
                result.as_ref().err()
            );
            assert!(d.writes.is_empty());
            if let Ok(backup) = result {
                assert_eq!(backup.manifest().fingerprint, ext);
            } else {
                assert_eq!(std::fs::read_dir(folder).unwrap().count(), 0);
            }
        }
    }
}

fn edit_request(pairs: &[(&str, &str)], firmware_dir: Option<PathBuf>) -> EditRequest {
    EditRequest {
        edits: pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        firmware_dir,
    }
}

#[test]
fn edit_without_region_change_writes_once_binds_no_firmware_and_never_flashes() {
    for slot in [Slot::A, Slot::B] {
        let mut s = Setup::edit(slot);
        let before = s.info.bytes.clone();
        s.edit = Some(edit_request(&[("rollback_index[5]", "7")], None));
        s.transact().unwrap();
        assert_eq!(s.edits_shown, 1);
        assert_eq!(s.info.writes, 1);
        assert_eq!(deviceinfo::indices(tb520fu().layout(), &s.info.bytes)[5], 7);
        // Only the edited field differs.
        let field = tb520fu().layout().field("rollback_index[5]").unwrap();
        for (n, (a, b)) in before.iter().zip(&s.info.bytes).enumerate() {
            if a != b {
                assert!(field.range().contains(&n), "{n:#x}");
            }
        }
        assert!(s.state.firmware.is_none());
        assert!(s.chosen.is_none());
        assert!(saved_state(&s).firmware.is_none());
        // The re-read after the EDL prompt neither asks nor writes again.
        s.transact().unwrap();
        assert_eq!(s.edits_shown, 1);
        assert_eq!(s.info.writes, 1);
        recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
        let mut d = Finish {
            edl: EdlScript::default(),
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
            None,
            &mut s.state,
            &s.store,
            &mut ui,
            &mut vec![],
        )
        .unwrap();
        // Originals restored, then a plain reboot: no firmware, no wipe, no slot change.
        assert_eq!((d.flashed, d.active, d.reboots), (0, 0, 1));
        assert_eq!(
            d.inner
                .writes
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            [recovery_flow::recovery_name(slot).as_str(), "gpt_main4"]
        );
        assert_eq!(
            ui.phases,
            vec![
                Phase::EdlWaiting,
                Phase::EdlWaiting,
                Phase::Restoring,
                Phase::FinalReboot
            ]
        );
        assert_eq!(s.state.recovery.phase, Phase::Done);
    }
}

#[test]
fn edit_region_change_requires_a_checked_firmware_folder_before_any_write() {
    use recovery_flow::restore::{self, Assessment};
    let missing = std::env::temp_dir().join("lapiswitch-no-such-firmware");
    for firmware_dir in [None, Some(missing)] {
        let mut s = Setup::edit(Slot::A);
        s.edit = Some(edit_request(&[("region", "ROW")], firmware_dir.clone()));
        let error = s.transact().unwrap_err();
        if firmware_dir.is_none() {
            assert!(error.contains("펌웨어 폴더"), "{error}");
        }
        assert_eq!(s.info.writes, 0);
        assert!(!s.info.prepared);
        assert!(s.state.firmware.is_none());
        assert!(s.chosen.is_none());
        assert!(saved_state(&s).recovery.attempt.is_none());
        assert_eq!(restore::assess(&s.backup).unwrap(), Assessment::BeforeWrite);
    }
}

#[test]
fn edit_invalid_values_and_tail_changes_are_rejected_without_writes() {
    for pairs in [
        vec![("region", "XYZ")],
        vec![("rollback_index[0]", "not a number")],
        vec![("region", "PRC_TOO_LONG_VALUE")],
        vec![("no_such_field", "1")],
    ] {
        let mut s = Setup::edit(Slot::B);
        s.edit = Some(edit_request(&pairs, None));
        assert!(s.transact().is_err(), "{pairs:?}");
        assert_eq!(s.info.writes, 0, "{pairs:?}");
        assert!(saved_state(&s).recovery.attempt.is_none());
    }
}

#[test]
fn closing_the_editor_cancels_before_any_plan_or_write() {
    use recovery_flow::restore::{self, Assessment};
    let mut s = Setup::edit(Slot::A);
    assert_eq!(s.outcome().unwrap(), Outcome::Cancelled);
    assert_eq!(s.edits_shown, 1);
    assert_eq!(s.info.writes, 0);
    let saved = saved_state(&s);
    assert_eq!(saved.recovery.phase, Phase::RecoveryWaiting);
    assert!(saved.recovery.attempt.is_none());
    assert_eq!(restore::assess(&s.backup).unwrap(), Assessment::BeforeWrite);
}

#[test]
fn an_edit_interrupted_after_its_plan_resumes_with_the_recorded_values() {
    let mut s = Setup::edit(Slot::B);
    s.edit = Some(edit_request(&[("rollback_index[1]", "0x10")], None));
    s.info.fail_prepare = true;
    assert!(s.transact().is_err());
    assert_eq!(phase(&s), Phase::Planned);
    // A different answer now must not replace the durable plan.
    s.info.fail_prepare = false;
    s.edit = Some(edit_request(&[("rollback_index[1]", "0x20")], None));
    s.transact().unwrap();
    assert_eq!(s.edits_shown, 1);
    assert_eq!(s.info.writes, 1);
    assert_eq!(
        deviceinfo::indices(tb520fu().layout(), &s.info.bytes)[1],
        0x10
    );
}

#[test]
fn reverse_restore_of_an_edit_targets_the_complete_original() {
    use recovery_flow::restore::{self, Assessment};
    let mut s = Setup::edit(Slot::A);
    let original = s.info.bytes.clone();
    s.edit = Some(edit_request(&[("rollback_index[9]", "3")], None));
    s.transact().unwrap();
    assert_eq!(
        restore::preflight(&s.backup).unwrap(),
        Assessment::Verified {
            original: Some(lapiswitch_core::region::Region::Prc),
            current: Some(lapiswitch_core::region::Region::Prc),
            changed: true,
        }
    );
    assert!(restore::preflight(&s.backup).unwrap().needs_twrp());
    let saved = saved_state(&s);
    let record = saved.recovery.attempt.unwrap();
    assert_eq!(
        record.original_region,
        Some(lapiswitch_core::region::Region::Prc)
    );
    let path = s
        .backup
        .path()
        .join("Partition")
        .join(&record.directory)
        .join("before_shared_1.bin");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        record.before_sha256,
        lapiswitch_core::backup::hash_file(&path, None).unwrap().1
    );
}

#[test]
fn a_tb522fu_edit_uses_its_own_map_and_backup() {
    use lapiswitch_core::region::Region;
    let device = tb522fu();
    let layout = device.layout();
    let mut s = Setup::edit_for(device, Slot::B);
    assert_eq!(s.backup.device(), device);
    assert!(s.backup.file_path("oemowninfo.img").is_ok());
    let before = s.info.bytes.clone();
    // A TB520FU-only field does not exist here; the TB522FU record version does.
    s.edit = Some(edit_request(
        &[("oem_reserved_1", &"00".repeat(0x40))],
        None,
    ));
    assert!(s.transact().unwrap_err().contains("oem_reserved_1"));
    assert_eq!(s.info.writes, 0);
    s.edit = Some(edit_request(
        &[("region_info_version", "1"), ("rollback_index[3]", "20")],
        None,
    ));
    s.transact().unwrap();
    assert_eq!(s.info.writes, 1);
    assert_eq!(deviceinfo::indices(layout, &s.info.bytes)[3], 20);
    assert_eq!(
        deviceinfo::region(layout, &s.info.bytes).unwrap(),
        Region::Prc
    );
    for (n, (a, b)) in before.iter().zip(&s.info.bytes).enumerate() {
        if a != b {
            assert!(
                (0xCD0..0xCD4).contains(&n) || (0x898 + 24..0x898 + 32).contains(&n),
                "{n:#x}"
            );
        }
    }
    assert!(s.state.firmware.is_none());
    // Its region field sits at 0xCD4; a change still needs TB522FU firmware.
    let mut s = Setup::edit_for(device, Slot::A);
    s.edit = Some(edit_request(&[("region", "ROW")], None));
    assert!(s.transact().unwrap_err().contains("펌웨어 폴더"));
    assert_eq!(s.info.writes, 0);
}

fn tb522fu_finish_setup(slot: Slot, country: Option<&str>) -> (Setup, Finish) {
    let mut s = Setup::build_for(tb522fu(), slot, Some(RollbackMode::Reset), false);
    s.state.options.country = country.map(str::to_owned);
    s.transact().unwrap();
    recovery_flow::checkpoint(&mut s.state, &s.store, Phase::EdlWaiting).unwrap();
    let inner = std::mem::replace(&mut s.disk, Fake::new(600, true));
    let d = Finish {
        edl: EdlScript::default(),
        inner,
        flashed: 0,
        active: 0,
        reboots: 0,
        slot,
    };
    (s, d)
}

fn finish_tb522fu(s: &mut Setup, d: &mut Finish) -> Result<(), String> {
    recovery_flow::finish(
        d,
        &s.backup,
        Some(s.firmware.clone()),
        &mut s.state,
        &s.store,
        &mut Ui::default(),
        &mut vec![],
    )
}

fn oemowninfo() -> lapiswitch_core::country::oemowninfo::OemOwnInfo {
    match tb522fu().country() {
        lapiswitch_devices::Country::Oemowninfo(field) => field,
        other => panic!("{other:?}"),
    }
}

#[test]
fn tb522fu_country_runs_after_firmware_in_its_own_partition_only() {
    for slot in [Slot::A, Slot::B] {
        for code in [None, Some("CN"), Some("KR"), Some("DE")] {
            let (mut s, mut d) = tb522fu_finish_setup(slot, code);
            let original = d.inner.bytes("oemowninfo").to_vec();
            let persist = d.inner.bytes("persist").to_vec();
            let devinfo = d.inner.bytes("devinfo").to_vec();
            finish_tb522fu(&mut s, &mut d).unwrap();
            let expected = code.map_or_else(
                || original.clone(),
                |c| {
                    lapiswitch_core::country::oemowninfo::plan(&oemowninfo(), &original, c).unwrap()
                },
            );
            assert_eq!(d.inner.bytes("oemowninfo"), expected);
            // The devinfo/persist method is never used for this device.
            assert_eq!(d.inner.bytes("persist"), persist);
            assert_eq!(d.inner.bytes("devinfo"), devinfo);
            assert_eq!(
                std::fs::read(s.backup.file_path("oemowninfo.img").unwrap()).unwrap(),
                original
            );
            assert_eq!(
                d.inner
                    .writes
                    .iter()
                    .filter(|p| p.name == "oemowninfo")
                    .count(),
                usize::from(matches!(code, Some("KR" | "DE")))
            );
            assert_eq!((d.flashed, d.active, d.reboots), (1, 1, 1));
            assert_eq!(s.state.recovery.phase, Phase::Done);
            s.backup.verify().unwrap();
        }
    }
}

#[test]
fn a_lost_country_write_response_is_verified_on_resume_without_a_second_write() {
    let (mut s, mut d) = tb522fu_finish_setup(Slot::B, Some("KR"));
    d.inner.fail_writes.insert(3);
    assert!(
        finish_tb522fu(&mut s, &mut d)
            .unwrap_err()
            .contains("결과 불명")
    );
    assert_eq!((d.active, d.reboots), (0, 0));
    assert_eq!(s.state.recovery.phase, Phase::FirmwareWriting);
    assert!(
        s.backup
            .path()
            .join("Partition/oemowninfo.country.json")
            .is_file()
    );
    d.inner.fail_writes.clear();
    finish_tb522fu(&mut s, &mut d).unwrap();
    assert_eq!(
        d.inner
            .writes
            .iter()
            .filter(|p| p.name == "oemowninfo")
            .count(),
        1
    );
    assert_eq!((d.active, d.reboots), (1, 1));
}

#[test]
fn a_corrupt_country_readback_keeps_evidence_and_never_rewrites() {
    let (mut s, mut d) = tb522fu_finish_setup(Slot::A, Some("KR"));
    d.inner.corrupt_writes.insert(3);
    assert!(
        finish_tb522fu(&mut s, &mut d)
            .unwrap_err()
            .contains("재읽기 검증 실패")
    );
    assert_eq!((d.active, d.reboots), (0, 0));
    let count = d.inner.writes.len();
    d.inner.corrupt_writes.clear();
    assert!(
        finish_tb522fu(&mut s, &mut d)
            .unwrap_err()
            .contains("자동 재쓰기 없이")
    );
    assert_eq!(d.inner.writes.len(), count);
    s.backup.verify().unwrap();
}
