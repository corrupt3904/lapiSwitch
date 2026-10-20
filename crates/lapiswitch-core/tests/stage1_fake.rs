//! Original backup integrity for the single supported backup format.
#[path = "support/memory_device.rs"]
mod memory_device;
use lapiswitch_core::{
    backup::{self, BackupBuilder, VerifiedBackup},
    gpt::SECTOR_SIZE,
    slot::Slot,
};
use memory_device::*;

fn manifest(backup: &VerifiedBackup) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(backup.path().join("manifest.json")).unwrap()).unwrap()
}

fn rewrite(backup: &VerifiedBackup, value: &serde_json::Value) {
    std::fs::write(
        backup.path().join("manifest.json"),
        serde_json::to_vec(value).unwrap(),
    )
    .unwrap();
}

#[test]
fn backup_layout_records_only_the_current_format() {
    for slot in [Slot::A, Slot::B] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::with_recovery(600, true);
        let backup = device.backup(out.path(), slot);
        assert_eq!(backup.manifest().version, backup::MANIFEST_VERSION);
        assert_eq!(backup.manifest().operation, backup::MANIFEST_OPERATION);
        assert!(backup.path().join(backup::IDENTITY_NAME).is_file());
        let mut names: Vec<_> = backup
            .manifest()
            .files
            .iter()
            .map(|file| file.filename.clone())
            .collect();
        names.sort();
        let mut expected = vec![
            backup::IDENTITY_NAME.to_owned(),
            "devinfo.img".into(),
            "gpt_main4.bin".into(),
            "misc.img".into(),
            "persist.img".into(),
            "proinfo.img".into(),
            format!("recovery{}.img", slot.suffix()),
            format!("vendor_boot{}.img", slot.suffix()),
        ];
        expected.sort();
        assert_eq!(names, expected);
        for file in &backup.manifest().files {
            let path = backup.file_path(&file.filename).unwrap();
            if file.filename != backup::IDENTITY_NAME {
                assert_eq!(path.parent().unwrap(), backup.path().join("Partition"));
                assert!(!backup.path().join(&file.filename).exists());
            }
        }
        // Boot images are never dumped by the TWRP/DeviceInfo workflow.
        assert!(!backup.path().join("Partition/boot_a.img").exists());
        assert!(!backup.path().join("Partition/boot_b.img").exists());
        assert!(std::fs::read_dir(backup.path()).unwrap().all(|entry| {
            !matches!(
                entry.unwrap().path().extension().and_then(|e| e.to_str()),
                Some("img" | "bin")
            )
        }));
        assert!(device.writes.is_empty());
    }
}

#[test]
fn backups_from_other_format_versions_are_rejected() {
    let out = tempfile::tempdir().unwrap();
    let mut device = Fake::with_recovery(600, true);
    let backup = device.backup(out.path(), Slot::A);
    let current = manifest(&backup);
    for (version, operation) in [
        (1, "region_change"),
        (2, "region_change"),
        (3, "recovery_deviceinfo_v1"),
        (4, "recovery_deviceinfo_v2"),
        (5, "recovery_deviceinfo_v3"),
        (6, "recovery_deviceinfo_v4"),
        (backup::MANIFEST_VERSION, "recovery_deviceinfo_v3"),
    ] {
        let mut old = current.clone();
        old["version"] = version.into();
        old["operation"] = operation.into();
        rewrite(&backup, &old);
        assert!(
            VerifiedBackup::open(backup.path())
                .err()
                .unwrap()
                .contains("이 버전에서 만든 백업만"),
            "{version} {operation}"
        );
    }
    // Earlier manifests also carried fields this version no longer reads.
    for field in ["gpt_policy", "slot_estimated", "device_floors"] {
        let mut old = current.clone();
        old[field] = serde_json::Value::Bool(false);
        rewrite(&backup, &old);
        assert!(VerifiedBackup::open(backup.path()).is_err(), "{field}");
    }
    rewrite(&backup, &current);
    VerifiedBackup::open(backup.path()).unwrap();
}

#[test]
fn every_recovery_original_is_mandatory() {
    for missing in ["recovery_a.img", "misc.img", "vendor_boot_a.img"] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::with_recovery(600, true);
        let backup = device.backup(out.path(), Slot::A);
        let mut value = manifest(&backup);
        value["files"]
            .as_array_mut()
            .unwrap()
            .retain(|file| file["filename"] != missing);
        rewrite(&backup, &value);
        assert!(VerifiedBackup::open(backup.path()).is_err(), "{missing}");
    }
}

#[test]
fn manifest_path_traversal_duplicate_and_coordinate_tampering_are_rejected() {
    for changed in ["path", "duplicate", "coords", "schema"] {
        let out = tempfile::tempdir().unwrap();
        let mut device = Fake::with_recovery(600, true);
        let backup = device.backup(out.path(), Slot::A);
        let mut value = manifest(&backup);
        match changed {
            "path" => value["files"][0]["filename"] = "../gpt_main4.bin".into(),
            "duplicate" => {
                let entry = value["files"][0].clone();
                value["files"].as_array_mut().unwrap().push(entry);
            }
            "coords" => value["files"][1]["partition"]["start_sector"] = 99.into(),
            _ => value["version"] = 255.into(),
        }
        rewrite(&backup, &value);
        assert!(VerifiedBackup::open(backup.path()).is_err(), "{changed}");
        assert!(device.writes.is_empty());
    }
}

#[test]
fn backup_folders_are_unique_and_the_adb_identity_cannot_be_overwritten() {
    let out = tempfile::tempdir().unwrap();
    let a = BackupBuilder::create(out.path()).unwrap();
    let b = BackupBuilder::create(out.path()).unwrap();
    assert_ne!(a.path(), b.path());
    a.save_identity("first").unwrap();
    a.save_identity("first").unwrap();
    assert!(a.save_identity("different").is_err());
    assert_eq!(
        std::fs::read(a.path().join(backup::IDENTITY_NAME)).unwrap(),
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
