//! Saved-state history and legacy original restoration compatibility.
#[path = "support/memory_device.rs"]
mod memory_device;
use lapiswitch_core::{
    backup::{TransactionStage, VerifiedBackup},
    firmware::FirmwareReport,
    slot::Slot,
    stage1,
    state::{FlashOptions, ResumeAction, RunState, Stage, StateStore},
};
use memory_device::*;
struct Device {
    inner: Fake,
}
struct Setup {
    _root: tempfile::TempDir,
    device: Device,
    backup: VerifiedBackup,
    firmware: FirmwareReport,
    state: RunState,
    store: StateStore,
}
impl Setup {
    fn new(options: FlashOptions) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut inner = Fake::new(600, true);
        let backup = inner.backup(root.path(), Slot::A);
        backup.mark(TransactionStage::S4Done).unwrap();
        let fwdir = root.path().join("firmware");
        std::fs::create_dir(&fwdir).unwrap();
        for name in [
            "boot.img",
            "vbmeta.img",
            "vbmeta_system.img",
            "vendor_boot.img",
            "loader.melf",
        ] {
            std::fs::write(fwdir.join(name), name.as_bytes()).unwrap();
        }
        let xml = fwdir.join("rawprogram4.xml");
        std::fs::write(&xml, "<data><program filename=\"boot.img\" num_partition_sectors=\"600\"/><program filename=\"optional.img\" num_partition_sectors=\"1\"/></data>").unwrap();
        let mut firmware = firmware();
        firmware.image_dir = fwdir.clone();
        firmware.loader = fwdir.join("loader.melf");
        firmware.raw_xmls = vec![xml];
        let store = StateStore::new(root.path().join("state.json"));
        let mut state = RunState::new(&firmware, options, backup.path()).unwrap();
        state.bind_backup(&backup).unwrap();
        state.stage = Stage::S4Done;
        store.save(&state).unwrap();
        Self {
            _root: root,
            device: Device { inner },
            backup,
            firmware,
            state,
            store,
        }
    }
}
#[test]
fn missing_nested_payloads_are_pinned_without_requiring_the_directory() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    let xml = &setup.firmware.raw_xmls[0];
    std::fs::write(
        xml,
        "<data><program filename=\"nested/optional.img\" num_partition_sectors=\"1\"/></data>",
    )
    .unwrap();
    setup.state.files = lapiswitch_core::state::seal_firmware(&setup.firmware).unwrap();
    std::fs::create_dir(setup.firmware.image_dir.join("nested")).unwrap();
    setup
        .state
        .verify_binding(&setup.backup, &setup.firmware)
        .unwrap();
    std::fs::write(
        setup.firmware.image_dir.join("nested/optional.img"),
        b"new payload",
    )
    .unwrap();
    assert!(
        setup
            .state
            .verify_binding(&setup.backup, &setup.firmware)
            .is_err()
    );
}

#[test]
fn resume_decisions_combine_state_and_backup_transaction() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    for (stage, action) in [
        (Stage::S1, ResumeAction::Recovery),
        (Stage::S3, ResumeAction::Recovery),
        (Stage::S4, ResumeAction::ResetThenWait),
        (Stage::S4Done, ResumeAction::ResetThenWait),
        (Stage::S5Resetting, ResumeAction::ResetUncertain),
        (Stage::S5, ResumeAction::Wait),
        (Stage::S5Done, ResumeAction::Flash),
        (Stage::S6, ResumeAction::Flash),
        (Stage::FinalReboot, ResumeAction::Finalize),
        (Stage::Done, ResumeAction::Restart),
    ] {
        setup.state.stage = stage;
        assert_eq!(
            setup.state.resume_action(Some(TransactionStage::S4Done)),
            action
        );
    }
    setup.state.stage = Stage::S6;
    assert_eq!(
        setup
            .state
            .resume_action(Some(TransactionStage::BootWriting)),
        ResumeAction::Recovery
    );
    assert_eq!(setup.state.resume_action(None), ResumeAction::Recovery);
    assert_eq!(
        setup.state.resume_action(Some(TransactionStage::Restored)),
        ResumeAction::Restart
    );
}

#[test]
fn state_lock_is_exclusive_and_released_after_drop() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::new(root.path().join("state.json"));
    let held = store.lock().unwrap();
    assert!(store.lock().is_err());
    drop(held);
    let _again = store.lock().unwrap();
}

#[test]
fn completed_restore_archives_active_state_and_backup_removal_does_not_block_new_run() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    setup.state.stage = Stage::S5;
    setup.store.save(&setup.state).unwrap();
    let original = std::fs::read(setup.store.path()).unwrap();
    let _held = setup.store.lock().unwrap();
    stage1::restore(&mut setup.device.inner, &setup.backup, &mut vec![]).unwrap();
    let archived = setup
        .store
        .archive_restored(&setup.backup)
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(archived).unwrap(), original);
    assert!(setup.store.load().unwrap().is_none());
    std::fs::remove_file(setup.backup.path().join("transaction.json")).unwrap();
    std::fs::remove_file(setup.backup.path().join("manifest.json")).unwrap();
    let fresh = RunState::new(
        &setup.firmware,
        FlashOptions::new(true, None).unwrap(),
        setup.backup.path(),
    )
    .unwrap();
    setup.store.begin(&fresh).unwrap();
    assert_eq!(setup.store.load().unwrap().unwrap().run_id, fresh.run_id);
}

#[test]
fn unfinished_restore_and_unrelated_backup_never_clear_active_state() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    let _held = setup.store.lock().unwrap();
    assert!(setup.store.archive_restored(&setup.backup).is_err());
    assert!(setup.store.load().unwrap().is_some());
    stage1::restore(&mut setup.device.inner, &setup.backup, &mut vec![]).unwrap();
    let unrelated = setup._root.path().join("unrelated-backup");
    std::fs::create_dir(&unrelated).unwrap();
    setup.state.backup_dir = unrelated;
    setup.store.save(&setup.state).unwrap();
    let original = std::fs::read(setup.store.path()).unwrap();
    assert!(
        setup
            .store
            .archive_restored(&setup.backup)
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read(setup.store.path()).unwrap(), original);
}

#[test]
fn archive_failure_preserves_active_record_after_verified_restore() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    let _held = setup.store.lock().unwrap();
    stage1::restore(&mut setup.device.inner, &setup.backup, &mut vec![]).unwrap();
    let original = std::fs::read(setup.store.path()).unwrap();
    std::fs::write(
        setup.store.path().parent().unwrap().join("history"),
        b"occupied",
    )
    .unwrap();
    assert!(setup.store.archive_restored(&setup.backup).is_err());
    assert_eq!(std::fs::read(setup.store.path()).unwrap(), original);
    assert_eq!(
        setup.backup.transaction_stage().unwrap(),
        TransactionStage::Restored
    );
}

#[test]
fn malformed_or_unknown_state_never_becomes_a_fresh_run() {
    let setup = Setup::new(FlashOptions::new(true, None).unwrap());
    std::fs::write(setup.store.path(), b"{incomplete").unwrap();
    assert!(setup.store.load().is_err());
    assert!(setup.store.begin(&setup.state).is_err());
    setup.store.save(&setup.state).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(setup.store.path()).unwrap()).unwrap();
    value["version"] = 99.into();
    std::fs::write(setup.store.path(), serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(setup.store.load().is_err());
}

#[test]
fn unfinished_writes_block_new_runs_but_prewrite_or_restored_runs_allow_restart() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    assert!(setup.store.begin(&setup.state).is_err());
    setup.state.stage = Stage::S3;
    setup.store.save(&setup.state).unwrap();
    setup.backup.mark(TransactionStage::BackupVerified).unwrap();
    setup.store.begin(&setup.state).unwrap();
    setup.state.stage = Stage::S4;
    setup.store.save(&setup.state).unwrap();
    setup.backup.mark(TransactionStage::Restored).unwrap();
    setup.store.begin(&setup.state).unwrap();
}

#[test]
fn keep_data_cannot_silently_request_a_country_write() {
    assert!(FlashOptions::new(false, Some("KR")).is_err());
    assert!(FlashOptions::new(true, Some("invalid")).is_err());
}

#[test]
fn damaged_payload_or_journal_cannot_hide_a_prior_write_during_restart() {
    for malformed_journal in [false, true] {
        let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
        setup.state.stage = Stage::S3;
        setup.store.save(&setup.state).unwrap();
        setup.backup.mark(TransactionStage::GptWriting).unwrap();
        std::fs::write(setup.backup.file_path("boot_a.img").unwrap(), b"damaged").unwrap();
        if malformed_journal {
            std::fs::write(setup.backup.path().join("transaction.json"), b"incomplete").unwrap();
        }
        assert!(setup.store.begin(&setup.state).is_err());
    }
}

#[test]
fn explicit_archive_preserves_exact_state_backups_and_logs_without_claiming_restoration() {
    for recovery in [false, true] {
        let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
        setup.state.stage = Stage::S5;
        if recovery {
            setup.state.version = 2;
            setup.state.recovery = Some(lapiswitch_core::recovery_flow::Progress {
                phase: lapiswitch_core::recovery_flow::Phase::RecoveryWaiting,
                attempt: None,
            });
        }
        setup.store.save(&setup.state).unwrap();
        // Preserve the original file bytes, including its formatting.
        let original = format!(
            "\n{}\n",
            std::fs::read_to_string(setup.store.path()).unwrap()
        );
        std::fs::write(setup.store.path(), original.as_bytes()).unwrap();
        let journal_path = setup.backup.path().join("transaction.json");
        let journal = std::fs::read(&journal_path).unwrap();
        let log_path = setup._root.path().join("previous.log");
        std::fs::write(&log_path, b"previous logs").unwrap();
        let device_writes = setup.device.inner.writes.clone();
        assert!(setup.store.begin(&setup.state).is_err());
        let archive = setup.store.archive_for_new_run(&setup.state).unwrap();
        assert_eq!(
            std::fs::read(archive.join("state.json")).unwrap(),
            original.as_bytes()
        );
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(archive.join("archive.json")).unwrap()).unwrap();
        assert_eq!(metadata["reason"], "USER_REQUESTED_NEW_RUN");
        assert_eq!(metadata["device_restoration_verified"], false);
        assert_eq!(
            metadata["state_sha256"],
            lapiswitch_core::backup::hash_file(&archive.join("state.json"), None)
                .unwrap()
                .1
        );
        assert!(setup.store.load().unwrap().is_none());
        assert_eq!(std::fs::read(&journal_path).unwrap(), journal);
        assert_eq!(std::fs::read(&log_path).unwrap(), b"previous logs");
        assert_eq!(setup.device.inner.writes, device_writes);
        assert_eq!(
            VerifiedBackup::open(setup.backup.path())
                .unwrap()
                .transaction_stage()
                .unwrap(),
            TransactionStage::S4Done
        );
        // A newly prepared run can now take the active pointer normally.
        let fresh = RunState::new(
            &setup.firmware,
            FlashOptions::new(true, None).unwrap(),
            setup.backup.path(),
        )
        .unwrap();
        setup.store.begin(&fresh).unwrap();
        assert_eq!(setup.store.load().unwrap().unwrap().run_id, fresh.run_id);
    }
}

#[test]
fn explicit_archive_works_when_the_original_backup_was_moved() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    setup.state.stage = Stage::S5;
    setup.store.save(&setup.state).unwrap();
    let moved = setup._root.path().join("moved-backup");
    std::fs::rename(setup.backup.path(), &moved).unwrap();
    let archived = setup.store.archive_for_new_run(&setup.state).unwrap();
    assert!(archived.join("state.json").is_file());
    assert!(setup.store.load().unwrap().is_none());
    assert!(moved.join("manifest.json").is_file());
}

#[test]
fn explicit_archive_cannot_race_a_running_job_or_archive_a_changed_confirmation() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    setup.state.stage = Stage::S5;
    setup.store.save(&setup.state).unwrap();
    let original = std::fs::read(setup.store.path()).unwrap();
    let held = setup.store.lock().unwrap();
    assert!(setup.store.archive_for_new_run(&setup.state).is_err());
    assert_eq!(std::fs::read(setup.store.path()).unwrap(), original);
    drop(held);
    let mut changed = setup.state.clone();
    changed
        .warnings
        .push("record changed while confirmation was open".into());
    setup.store.save(&changed).unwrap();
    let updated = std::fs::read(setup.store.path()).unwrap();
    assert!(
        setup
            .store
            .archive_for_new_run(&setup.state)
            .unwrap_err()
            .contains("변경")
    );
    assert_eq!(std::fs::read(setup.store.path()).unwrap(), updated);
    assert!(
        !setup
            .store
            .path()
            .parent()
            .unwrap()
            .join("history")
            .exists()
    );
}

#[test]
fn explicit_archive_failure_keeps_the_active_guard_and_retry_can_succeed() {
    let mut setup = Setup::new(FlashOptions::new(true, None).unwrap());
    setup.state.stage = Stage::S5;
    setup.store.save(&setup.state).unwrap();
    let original = std::fs::read(setup.store.path()).unwrap();
    let history = setup.store.path().parent().unwrap().join("history");
    std::fs::write(&history, b"occupied").unwrap();
    assert!(setup.store.archive_for_new_run(&setup.state).is_err());
    assert_eq!(std::fs::read(setup.store.path()).unwrap(), original);
    assert!(setup.store.begin(&setup.state).is_err());
    std::fs::remove_file(&history).unwrap();
    let archived = setup.store.archive_for_new_run(&setup.state).unwrap();
    assert_eq!(
        std::fs::read(archived.join("state.json")).unwrap(),
        original
    );
    assert!(setup.store.archive_for_new_run(&setup.state).is_err());
    assert!(setup.store.load().unwrap().is_none());
}

#[test]
fn recovery_only_entry_point_rejects_legacy_boot_backups_before_any_device_work() {
    let setup = Setup::new(FlashOptions::new(true, None).unwrap());
    assert!(
        VerifiedBackup::open_recovery(setup.backup.path())
            .err()
            .unwrap()
            .contains("Recovery")
    );
    // Legacy backup inspection and original restoration remain available.
    assert!(VerifiedBackup::open(setup.backup.path()).is_ok());
}
