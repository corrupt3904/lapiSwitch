//! Saved-state history, resume decisions and run archiving.
#[path = "support/memory_device.rs"]
mod memory_device;
use lapiswitch_core::{
    backup::{TransactionStage, VerifiedBackup},
    deviceinfo::RollbackMode,
    firmware::FirmwareReport,
    recovery_flow::{self, Phase},
    slot::Slot,
    state::{FlashOptions, ResumeAction, RunState, STATE_VERSION, StateStore},
};
use memory_device::*;
struct Setup {
    _root: tempfile::TempDir,
    device: Fake,
    backup: VerifiedBackup,
    firmware: FirmwareReport,
    state: RunState,
    store: StateStore,
}
fn options() -> FlashOptions {
    FlashOptions::new(None).unwrap()
}
impl Setup {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut device = Fake::with_recovery(600, true);
        let backup = device.backup(root.path(), Slot::A);
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
        let mut state =
            RunState::convert(&firmware, RollbackMode::Reset, options(), backup.path()).unwrap();
        state.bind_backup(&backup).unwrap();
        recovery_flow::checkpoint(&mut state, &store, Phase::RecoveryWaiting).unwrap();
        Self {
            _root: root,
            device,
            backup,
            firmware,
            state,
            store,
        }
    }
    fn at(&mut self, phase: Phase) {
        recovery_flow::checkpoint(&mut self.state, &self.store, phase).unwrap();
    }
}
#[test]
fn missing_nested_payloads_are_pinned_without_requiring_the_directory() {
    let mut setup = Setup::new();
    let xml = &setup.firmware.raw_xmls[0];
    std::fs::write(
        xml,
        "<data><program filename=\"nested/optional.img\" num_partition_sectors=\"1\"/></data>",
    )
    .unwrap();
    setup.state.firmware.as_mut().unwrap().files =
        lapiswitch_core::state::seal_firmware(&setup.firmware).unwrap();
    std::fs::create_dir(setup.firmware.image_dir.join("nested")).unwrap();
    setup
        .state
        .verify_binding(&setup.backup, Some(&setup.firmware))
        .unwrap();
    std::fs::write(
        setup.firmware.image_dir.join("nested/optional.img"),
        b"new payload",
    )
    .unwrap();
    assert!(
        setup
            .state
            .verify_binding(&setup.backup, Some(&setup.firmware))
            .is_err()
    );
    // Between binding and flashing only the identity is compared; the full
    // payload hash runs on resume and immediately before flashing.
    setup
        .state
        .check_binding(&setup.backup, Some(&setup.firmware))
        .unwrap();
    let mut other = setup.firmware.clone();
    other.indices.boot += 1;
    assert!(
        setup
            .state
            .check_binding(&setup.backup, Some(&other))
            .is_err()
    );
    assert!(setup.state.check_binding(&setup.backup, None).is_err());
}

#[test]
fn resume_decisions_combine_the_saved_phase_and_backup_transaction() {
    let mut setup = Setup::new();
    for (phase, transaction, action) in [
        (
            Phase::Preflight,
            TransactionStage::BackupVerified,
            ResumeAction::Restart,
        ),
        (
            Phase::BackedUp,
            TransactionStage::BackupVerified,
            ResumeAction::Restart,
        ),
        (
            Phase::Staging,
            TransactionStage::BackupVerified,
            ResumeAction::Recovery,
        ),
        (Phase::Staged, TransactionStage::S4Done, ResumeAction::Wait),
        (
            Phase::RecoveryWaiting,
            TransactionStage::S4Done,
            ResumeAction::Wait,
        ),
        (Phase::Planned, TransactionStage::S4Done, ResumeAction::Wait),
        (
            Phase::WritePending,
            TransactionStage::S4Done,
            ResumeAction::Wait,
        ),
        (
            Phase::EdlWaiting,
            TransactionStage::S4Done,
            ResumeAction::Wait,
        ),
        (
            Phase::Restoring,
            TransactionStage::S4Done,
            ResumeAction::Flash,
        ),
        (
            Phase::OriginalsRestored,
            TransactionStage::S4Done,
            ResumeAction::Flash,
        ),
        (
            Phase::FirmwareWriting,
            TransactionStage::S4Done,
            ResumeAction::Flash,
        ),
        (
            Phase::FinalReboot,
            TransactionStage::S4Done,
            ResumeAction::Finalize,
        ),
        (Phase::Done, TransactionStage::S4Done, ResumeAction::Restart),
        (
            Phase::RecoveryWaiting,
            TransactionStage::BootWriting,
            ResumeAction::Recovery,
        ),
        (
            Phase::RecoveryWaiting,
            TransactionStage::RecoveryRequired,
            ResumeAction::Recovery,
        ),
        (
            Phase::RecoveryWaiting,
            TransactionStage::BackupVerified,
            ResumeAction::Recovery,
        ),
        (
            Phase::RecoveryWaiting,
            TransactionStage::Restored,
            ResumeAction::Restart,
        ),
    ] {
        setup.at(phase);
        assert_eq!(
            setup.state.resume_action(Some(transaction)),
            action,
            "{phase:?} {transaction:?}"
        );
    }
    setup.at(Phase::RecoveryWaiting);
    assert_eq!(setup.state.resume_action(None), ResumeAction::Recovery);
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
    let setup = Setup::new();
    let original = std::fs::read(setup.store.path()).unwrap();
    let _held = setup.store.lock().unwrap();
    setup.backup.mark(TransactionStage::Restored).unwrap();
    let archived = setup
        .store
        .archive_restored(&setup.backup)
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(archived).unwrap(), original);
    assert!(setup.store.load().unwrap().is_none());
    std::fs::remove_file(setup.backup.path().join("transaction.json")).unwrap();
    std::fs::remove_file(setup.backup.path().join("manifest.json")).unwrap();
    let fresh = RunState::convert(
        &setup.firmware,
        RollbackMode::Reset,
        options(),
        setup.backup.path(),
    )
    .unwrap();
    setup.store.begin(&fresh).unwrap();
    assert_eq!(setup.store.load().unwrap().unwrap().run_id, fresh.run_id);
}

#[test]
fn unfinished_restore_and_unrelated_backup_never_clear_active_state() {
    let mut setup = Setup::new();
    let _held = setup.store.lock().unwrap();
    assert!(setup.store.archive_restored(&setup.backup).is_err());
    assert!(setup.store.load().unwrap().is_some());
    setup.backup.mark(TransactionStage::Restored).unwrap();
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
    let setup = Setup::new();
    let _held = setup.store.lock().unwrap();
    setup.backup.mark(TransactionStage::Restored).unwrap();
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
fn malformed_old_or_unknown_state_never_becomes_a_fresh_run() {
    let setup = Setup::new();
    std::fs::write(setup.store.path(), b"{incomplete").unwrap();
    assert!(setup.store.load().is_err());
    assert!(setup.store.begin(&setup.state).is_err());
    setup.store.save(&setup.state).unwrap();
    let current: serde_json::Value =
        serde_json::from_slice(&std::fs::read(setup.store.path()).unwrap()).unwrap();
    assert_eq!(current["version"], STATE_VERSION);
    let write = |value: &serde_json::Value| {
        std::fs::write(setup.store.path(), serde_json::to_vec(value).unwrap()).unwrap();
    };
    for version in [1, 2, 99] {
        let mut value = current.clone();
        value["version"] = version.into();
        write(&value);
        assert!(setup.store.load().is_err(), "{version}");
        assert!(setup.store.begin(&setup.state).is_err());
    }
    // Records from earlier formats: no recovery progress, keep-data flag,
    // GPT policy marker or implicit rollback mode.
    let mut value = current.clone();
    value["recovery"] = serde_json::Value::Null;
    write(&value);
    assert!(setup.store.load().is_err());
    for (field, json) in [
        ("wipe", serde_json::json!(true)),
        (
            "gpt_policy",
            serde_json::json!("FIXED_VBMETA_A_ACTIVE_PVMFW"),
        ),
    ] {
        let mut value = current.clone();
        if field == "wipe" {
            value["options"][field] = json;
        } else {
            value[field] = json;
        }
        write(&value);
        assert!(setup.store.load().is_err(), "{field}");
    }
    let mut value = current.clone();
    value["options"]["rollback_mode"] = serde_json::json!("RESET");
    write(&value);
    assert!(setup.store.load().is_err());
    let mut value = current.clone();
    value["request"]["CONVERT"]
        .as_object_mut()
        .unwrap()
        .remove("rollback_mode");
    write(&value);
    assert!(setup.store.load().is_err());
    for field in ["target_region", "firmware_dir"] {
        let mut value = current.clone();
        value[field] = serde_json::json!("ROW");
        write(&value);
        assert!(setup.store.load().is_err(), "{field}");
    }
    write(&current);
    assert!(setup.store.load().unwrap().is_some());
}

#[test]
fn unfinished_writes_block_new_runs_but_prewrite_or_restored_runs_allow_restart() {
    let mut setup = Setup::new();
    assert!(setup.store.begin(&setup.state).is_err());
    setup.at(Phase::BackedUp);
    setup.backup.mark(TransactionStage::BackupVerified).unwrap();
    setup.store.begin(&setup.state).unwrap();
    setup.at(Phase::Staging);
    assert!(setup.store.begin(&setup.state).is_err());
    setup.backup.mark(TransactionStage::Restored).unwrap();
    setup.store.begin(&setup.state).unwrap();
}

#[test]
fn country_codes_are_validated_when_options_are_built() {
    assert!(FlashOptions::new(Some("invalid")).is_err());
    assert_eq!(
        FlashOptions::new(Some("kr")).unwrap().country.as_deref(),
        Some("KR")
    );
}

#[test]
fn damaged_payload_or_journal_cannot_hide_a_prior_write_during_restart() {
    for malformed_journal in [false, true] {
        let mut setup = Setup::new();
        setup.at(Phase::BackedUp);
        setup.backup.mark(TransactionStage::GptWriting).unwrap();
        std::fs::write(setup.backup.file_path("devinfo.img").unwrap(), b"damaged").unwrap();
        if malformed_journal {
            std::fs::write(setup.backup.path().join("transaction.json"), b"incomplete").unwrap();
        }
        assert!(setup.store.begin(&setup.state).is_err());
    }
}

#[test]
fn explicit_archive_preserves_exact_state_backups_and_logs_without_claiming_restoration() {
    let setup = Setup::new();
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
    let device_writes = setup.device.writes.clone();
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
    assert_eq!(setup.device.writes, device_writes);
    assert_eq!(
        VerifiedBackup::open(setup.backup.path())
            .unwrap()
            .transaction_stage()
            .unwrap(),
        TransactionStage::S4Done
    );
    // A newly prepared run can now take the active pointer normally.
    let fresh = RunState::convert(
        &setup.firmware,
        RollbackMode::Reset,
        options(),
        setup.backup.path(),
    )
    .unwrap();
    setup.store.begin(&fresh).unwrap();
    assert_eq!(setup.store.load().unwrap().unwrap().run_id, fresh.run_id);
}

#[test]
fn explicit_archive_works_when_the_original_backup_was_moved() {
    let setup = Setup::new();
    let moved = setup._root.path().join("moved-backup");
    std::fs::rename(setup.backup.path(), &moved).unwrap();
    let archived = setup.store.archive_for_new_run(&setup.state).unwrap();
    assert!(archived.join("state.json").is_file());
    assert!(setup.store.load().unwrap().is_none());
    assert!(moved.join("manifest.json").is_file());
}

#[test]
fn explicit_archive_cannot_race_a_running_job_or_archive_a_changed_confirmation() {
    let setup = Setup::new();
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
    let setup = Setup::new();
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
