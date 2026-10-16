//! Original backup integrity and legacy original boot/GPT restoration.
#[path = "support/memory_device.rs"]
mod memory_device;
use lapiswitch_core::{
    backup::{self, BackupBuilder, TransactionStage, VerifiedBackup},
    gpt::SECTOR_SIZE,
    slot::Slot,
    stage1::{self, RecoveryOutcome, ResetMode},
};
use memory_device::*;
#[test]
fn legacy_backup_layout_and_original_restore_remain_supported() {
    let out = tempfile::tempdir().unwrap();
    let mut device = Fake::new(600, true);
    let backup = device.backup(out.path(), Slot::B);
    assert_eq!(backup.manifest().version, 2);
    assert!(backup.path().join("getvar.txt").is_file());
    for file in &backup.manifest().files {
        let path = backup.file_path(&file.filename).unwrap();
        if file.filename != "getvar.txt" {
            assert_eq!(path.parent().unwrap(), backup.path().join("Partition"));
            assert!(!backup.path().join(&file.filename).exists());
        }
    }
    device.disk[4][800 * SECTOR_SIZE] ^= 1;
    assert!(backup.path().join("Partition/gpt_header4.bin").is_file());
    assert!(std::fs::read_dir(backup.path()).unwrap().all(|entry| {
        !matches!(
            entry.unwrap().path().extension().and_then(|e| e.to_str()),
            Some("img" | "bin")
        )
    }));
    stage1::restore(&mut device, &backup, &mut vec![]).unwrap();
    device.assert_original(Slot::B);
}

#[test]
fn legacy_root_backups_restore_and_neither_layout_falls_back_to_other_files() {
    for slot in [Slot::A, Slot::B] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::new(600, true);
        let backup = device.backup(out.path(), slot);
        let manifest_path = backup.path().join("manifest.json");
        let mut manifest = serde_json::to_value(backup.manifest()).unwrap();
        for file in &backup.manifest().files {
            if file.filename != "getvar.txt" {
                std::fs::rename(
                    backup.file_path(&file.filename).unwrap(),
                    backup.path().join(&file.filename),
                )
                .unwrap();
            }
        }
        // A v2 backup must fail instead of silently reading root originals.
        assert!(VerifiedBackup::open(backup.path()).is_err());
        manifest["version"] = 1.into();
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let legacy = VerifiedBackup::open(backup.path()).unwrap();
        device.disk[4][if slot == Slot::A { 100 } else { 800 } * SECTOR_SIZE] ^= 1;
        stage1::restore(&mut device, &legacy, &mut vec![]).unwrap();
        device.assert_original(slot);
        let original = legacy.file_path("persist.img").unwrap();
        std::fs::rename(&original, legacy.path().join("Partition/persist.img")).unwrap();
        assert!(VerifiedBackup::open(legacy.path()).is_err());
    }
}

#[test]
fn manual_restore_reconstructs_original_boot_and_gpt() {
    let out = tempfile::tempdir().unwrap();
    let mut device = Fake::new(600, true);
    let backup = device.backup(out.path(), Slot::A);
    device.disk[4][100 * SECTOR_SIZE] ^= 1;
    let reopened = VerifiedBackup::open(backup.path()).unwrap();
    let report = stage1::restore(&mut device, &reopened, &mut vec![]).unwrap();
    assert!(report.warnings.is_empty());
    device.assert_original(Slot::A);
    assert_eq!(
        reopened.transaction_stage().unwrap(),
        TransactionStage::Restored
    );
    assert_eq!(device.resets, [ResetMode::System]);
}

#[test]
fn manual_restore_rejects_other_device_guid_or_changed_boot_coordinates() {
    for wrong_guid in [false, true] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::new(600, true);
        let backup = device.backup(out.path(), Slot::A);
        if wrong_guid {
            device.disk[4][SECTOR_SIZE + 56] ^= 1;
        } else {
            device.moved = true;
        }
        assert!(stage1::restore(&mut device, &backup, &mut vec![]).is_err());
        assert!(device.writes.is_empty());
    }
}

#[test]
fn manual_restore_failure_attempts_both_originals_and_never_boots_system() {
    for corrupt in [false, true] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::new(600, true);
        let backup = device.backup(out.path(), Slot::A);
        if corrupt {
            device.corrupt_writes.insert(1);
        } else {
            device.fail_writes.insert(1);
        }
        let error = stage1::restore(&mut device, &backup, &mut vec![])
            .err()
            .unwrap();
        assert_eq!(error.recovery, RecoveryOutcome::Failed);
        assert_eq!(
            device
                .writes
                .iter()
                .map(|part| part.name.as_str())
                .collect::<Vec<_>>(),
            ["boot_a", "gpt_main4"]
        );
        assert_eq!(device.resets, [ResetMode::Edl]);
        assert_eq!(
            backup.transaction_stage().unwrap(),
            TransactionStage::RecoveryRequired
        );
    }
}

#[test]
fn verified_manual_restore_keeps_completion_and_warns_on_system_reset_error() {
    let out = tempfile::tempdir().unwrap();
    let mut device = Fake::new(600, true);
    let backup = device.backup(out.path(), Slot::A);
    device.fail_reset = true;
    let mut log = vec![];
    let report = stage1::restore(&mut device, &backup, &mut log).unwrap();
    device.assert_original(Slot::A);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("시스템 부팅 요청 결과를 확인하지 못했습니다"));
    assert_eq!(device.resets, [ResetMode::System]);
    assert_eq!(
        backup.transaction_stage().unwrap(),
        TransactionStage::Restored
    );
    assert!(
        !log.iter()
            .any(|line| line.contains("시스템 부팅 요청을 전송했습니다"))
    );
}

#[test]
fn manual_restore_can_recover_a_damaged_gpt_with_matching_disk_guid() {
    for offset in [SECTOR_SIZE, SECTOR_SIZE + 16, 2 * SECTOR_SIZE] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::new(600, true);
        let backup = device.backup(out.path(), Slot::A);
        device.disk[4][offset] ^= 1;
        stage1::restore(&mut device, &backup, &mut vec![]).unwrap();
        device.assert_original(Slot::A);
    }
}

#[test]
fn manifest_path_traversal_duplicate_and_coordinate_tampering_are_rejected() {
    for changed in ["path", "duplicate", "coords", "schema"] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::new(600, true);
        let backup = device.backup(out.path(), Slot::A);
        let path = backup.path().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        match changed {
            "path" => manifest["files"][0]["filename"] = "../gpt_main4.bin".into(),
            "duplicate" => {
                let entry = manifest["files"][0].clone();
                manifest["files"].as_array_mut().unwrap().push(entry);
            }
            "coords" => manifest["files"][1]["partition"]["start_sector"] = 99.into(),
            _ => manifest["version"] = 255.into(),
        }
        std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(VerifiedBackup::open(backup.path()).is_err(), "{changed}");
        assert!(device.writes.is_empty());
    }
}

#[test]
fn backup_folders_are_unique_and_getvar_cannot_be_overwritten() {
    let out = tempfile::tempdir().unwrap();
    let a = BackupBuilder::create(out.path()).unwrap();
    let b = BackupBuilder::create(out.path()).unwrap();
    assert_ne!(a.path(), b.path());
    a.save_getvar("first").unwrap();
    a.save_getvar("first").unwrap();
    assert!(a.save_getvar("different").is_err());
    assert_eq!(
        std::fs::read(a.path().join("getvar.txt")).unwrap(),
        b"first"
    );
}

#[test]
fn absurd_gpt_header_geometry_is_rejected_without_overflow_or_large_allocations() {
    let device = Fake::new(600, true);
    for bad in ["array", "first", "entry_size"] {
        let mut bytes = device.original_gpt.clone();
        match bad {
            "array" => {
                bytes[SECTOR_SIZE + 72..SECTOR_SIZE + 80].copy_from_slice(&u64::MAX.to_le_bytes())
            }
            "first" => {
                bytes[SECTOR_SIZE + 40..SECTOR_SIZE + 48].copy_from_slice(&u64::MAX.to_le_bytes())
            }
            _ => bytes[SECTOR_SIZE + 84..SECTOR_SIZE + 88].copy_from_slice(&u32::MAX.to_le_bytes()),
        }
        refresh_header(&mut bytes);
        assert!(lapiswitch_core::storage::validate_gpt(&bytes).is_err());
    }
}

#[test]
fn prefix_hashing_accepts_sector_padding_but_not_truncation() {
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("readback");
    std::fs::write(&path, [1, 2, 3, 0, 0]).unwrap();
    let hash = backup::hash_file(&path, Some(3)).unwrap().1;
    std::fs::write(&path, [1, 2, 3]).unwrap();
    assert_eq!(backup::hash_file(&path, None).unwrap().1, hash);
    assert!(backup::hash_file(&path, Some(4)).is_err());
}

#[test]
fn legacy_manifest_can_restore_originals() {
    let out = tempfile::tempdir().unwrap();
    let mut device = Fake::new(600, true);
    let backup = device.backup(out.path(), Slot::B);
    let path = backup.path().join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest.as_object_mut().unwrap().remove("gpt_policy");
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let legacy = VerifiedBackup::open(backup.path()).unwrap();
    assert_eq!(
        legacy.manifest().gpt_policy,
        lapiswitch_core::gpt::GptPolicy::LegacyActiveSlot
    );

    assert!(device.writes.is_empty());
    // Simulate the old slot-b policy's live GPT before restoring it.
    let old_parts: Vec<_> = device
        .snapshot
        .iter()
        .filter(|p| !matches!(p.name.as_str(), "vbmeta_b" | "pvmfw_b"))
        .cloned()
        .collect();
    let old_live = gpt(&old_parts);
    device.disk[4][..old_live.len()].copy_from_slice(&old_live);
    stage1::restore(&mut device, &legacy, &mut vec![]).unwrap();
    device.assert_original(Slot::B);
    assert_eq!(
        legacy.transaction_stage().unwrap(),
        TransactionStage::Restored
    );
}

#[test]
fn previous_fixed_policy_backup_can_restore_originals() {
    for policy in [
        lapiswitch_core::gpt::GptPolicy::FixedVbmetaA,
        lapiswitch_core::gpt::GptPolicy::FixedVbmetaAPvmfwA,
    ] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::new(600, true);
        let backup = device.backup(out.path(), Slot::B);
        let path = backup.path().join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["gpt_policy"] = serde_json::to_value(policy).unwrap();
        std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let previous = VerifiedBackup::open(backup.path()).unwrap();
        assert_eq!(previous.manifest().gpt_policy, policy);

        assert!(device.writes.is_empty());
        let previous_parts: Vec<_> = device
            .snapshot
            .iter()
            .filter(|part| {
                part.name != "vbmeta_a"
                    && !(policy == lapiswitch_core::gpt::GptPolicy::FixedVbmetaAPvmfwA
                        && part.name == "pvmfw_a")
            })
            .cloned()
            .collect();
        let previous_gpt = gpt(&previous_parts);
        device.disk[4][..previous_gpt.len()].copy_from_slice(&previous_gpt);
        stage1::restore(&mut device, &previous, &mut vec![]).unwrap();
        device.assert_original(Slot::B);
        assert_eq!(device.resets, [ResetMode::System]);
    }
}
