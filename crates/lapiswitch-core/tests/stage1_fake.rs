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
        (7, "recovery_deviceinfo_v5"),
        (backup::MANIFEST_VERSION, "recovery_deviceinfo_v5"),
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
    for field in [
        "gpt_policy",
        "slot_estimated",
        "device_floors",
        "target_region",
        "firmware_indices",
    ] {
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
    let a = BackupBuilder::create(out.path(), Some(tb520fu())).unwrap();
    let b = BackupBuilder::create(out.path(), Some(tb520fu())).unwrap();
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

fn tb522fu_fake(oemowninfo_sectors: u64, build: &str) -> Fake {
    let mut device = Fake::with_recovery(600, true);
    device.disk[0].resize(400 * SECTOR_SIZE, 0x5A);
    device
        .snapshot
        .push(part("oemowninfo", 0, 100, oemowninfo_sectors));
    for slot in ["a", "b"] {
        device.fingerprints.insert(
            format!("vbmeta_system_{slot}"),
            format!("qti/TB522FU/TB522FU:17/CQ2A.260712.001/{build}:user/release-keys"),
        );
    }
    device
}

fn tb522fu_prepared(slot: Slot) -> lapiswitch_core::prepare::PrepareReport {
    let device = lapiswitch_core::device::Device::find("TB522FU").unwrap();
    lapiswitch_core::prepare::PrepareReport {
        device,
        started_in: lapiswitch_core::prepare::Connection::Adb,
        slot,
        device_key: None,
        identity: Some(lapiswitch_core::prepare::identity_record(
            device, slot, "FAKE0002",
        )),
    }
}

#[test]
fn a_tb522fu_backup_uses_its_own_rules_and_keeps_the_country_partition() {
    let tb522fu = lapiswitch_core::device::Device::find("TB522FU").unwrap();
    for slot in [Slot::A, Slot::B] {
        let out = tempfile::tempdir().unwrap();
        let mut device = tb522fu_fake(256, "10.028.260924C");
        let builder = BackupBuilder::create(out.path(), Some(tb522fu)).unwrap();
        assert!(
            builder
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("TB522FU_")
        );
        let backup = lapiswitch_core::stage1::take_backup(
            &mut device,
            &tb522fu_prepared(slot),
            builder,
            &mut vec![],
        )
        .unwrap();
        assert_eq!(backup.manifest().model, "TB522FU");
        assert_eq!(backup.device(), tb522fu);
        assert_eq!(
            std::fs::read(backup.file_path("oemowninfo.img").unwrap())
                .unwrap()
                .len(),
            0x100000
        );
        assert!(device.writes.is_empty());
    }
    // The TB520FU fixture keeps no country partition.
    let out = tempfile::tempdir().unwrap();
    let backup = Fake::with_recovery(600, true).backup(out.path(), Slot::A);
    assert!(backup.file_path("oemowninfo.img").is_err());
}

#[test]
fn tb522fu_country_partition_size_and_build_date_ceiling_are_enforced_before_backup() {
    let tb522fu = lapiswitch_core::device::Device::find("TB522FU").unwrap();
    for (sectors, build, expected) in [
        (255, "10.028.260924C", "oemowninfo"),
        (256, "10.028.260925C", "2026-09-24"),
        (256, "11.055.260905W", "2026-09-04"),
    ] {
        let out = tempfile::tempdir().unwrap();
        let mut device = tb522fu_fake(sectors, build);
        let builder = BackupBuilder::create(out.path(), Some(tb522fu)).unwrap();
        let error = lapiswitch_core::stage1::take_backup(
            &mut device,
            &tb522fu_prepared(Slot::A),
            builder,
            &mut vec![],
        )
        .err()
        .unwrap();
        assert!(error.contains(expected), "{build}: {error}");
        assert!(device.writes.is_empty());
    }
    // A TB520FU fingerprint is not the TB522FU the run prepared.
    let out = tempfile::tempdir().unwrap();
    let mut device = Fake::with_recovery(600, true);
    let builder = BackupBuilder::create(out.path(), Some(tb522fu)).unwrap();
    assert!(
        lapiswitch_core::stage1::take_backup(
            &mut device,
            &tb522fu_prepared(Slot::A),
            builder,
            &mut vec![]
        )
        .is_err()
    );
}
