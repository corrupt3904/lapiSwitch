//! Reproduce LTBox's named-but-unused last_parti scan row without USB/GUIDs.
#[path = "support/memory_device.rs"]
mod memory_device;
use lapiswitch_core::gpt::SECTOR_SIZE;
use lapiswitch_core::slot::Slot;
use lapiswitch_core::storage;
use memory_device::*;

fn with_marker(name: &str, start: u64, sectors: u64, duplicate: bool) -> Fake {
    marked(Fake::new(600, true), name, start, sectors, duplicate)
}

fn marked(mut device: Fake, name: &str, start: u64, sectors: u64, duplicate: bool) -> Fake {
    device.snapshot.push(part(name, 4, start, sectors));
    if duplicate {
        device.snapshot.push(part(name, 4, start, sectors));
    }
    let mut raw = gpt(&device.snapshot);
    for (index, row) in device.snapshot.iter().filter(|p| p.lun == 4).enumerate() {
        if row.name == name {
            raw[2 * SECTOR_SIZE + index * 128..2 * SECTOR_SIZE + index * 128 + 16].fill(0);
        }
    }
    let crc = crc32fast::hash(&raw[2 * SECTOR_SIZE..5 * SECTOR_SIZE]);
    raw[SECTOR_SIZE + 88..SECTOR_SIZE + 92].copy_from_slice(&crc.to_le_bytes());
    refresh_header(&mut raw);
    device.disk[4][..raw.len()].copy_from_slice(&raw);
    device.original_gpt = raw;
    device
}

#[test]
fn unused_end_marker_matches_scan_but_is_not_a_partition() {
    let device = with_marker("last_parti", 1600, 400, false);
    let table = storage::validate_gpt(&device.original_gpt).unwrap();
    assert!(table.find("last_parti").is_none());
    assert!(
        !table
            .entries()
            .iter()
            .any(|entry| entry.name == "last_parti")
    );
    let marker = device.snapshot.last().unwrap();
    storage::match_gpt_partition(&table, marker).unwrap();
    let mut changed = marker.clone();
    changed.start_sector += 1;
    assert!(storage::match_gpt_partition(&table, &changed).is_err());
    changed = marker.clone();
    changed.num_sectors -= 1;
    assert!(storage::match_gpt_partition(&table, &changed).is_err());
    changed = marker.clone();
    changed.lun = 0;
    assert!(storage::match_gpt_partition(&table, &changed).is_err());
    // Unused markers still follow the existing Python-compatible packing rule.
    let modified = table
        .remove_region_vbmeta(lapiswitch_core::slot::Slot::A)
        .unwrap();
    assert!(
        storage::validate_gpt(&modified)
            .unwrap()
            .find("last_parti")
            .is_none()
    );
}

#[test]
fn unknown_overlapping_out_of_bounds_and_duplicate_unused_rows_are_rejected() {
    for (name, start, sectors, duplicate) in [
        ("unexpected_marker", 1600, 400, false),
        ("last_parti", 1500, 500, false), // overlaps devinfo
        ("last_parti", 1600, 399, false), // not the end of usable storage
        ("last_parti", 1600, 401, false), // exceeds usable storage
        ("last_parti", 1600, 400, true),
    ] {
        let device = with_marker(name, start, sectors, duplicate);
        let table = storage::validate_gpt(&device.original_gpt).unwrap();
        assert!(storage::match_gpt_partition(&table, device.snapshot.last().unwrap()).is_err());
    }
    let device = Fake::new(600, true);
    let table = storage::validate_gpt(&device.original_gpt).unwrap();
    assert!(storage::match_gpt_partition(&table, &part("last_parti", 4, 1600, 400)).is_err());
}

#[test]
fn backup_and_gpt_modification_work_with_the_scanners_unused_end_marker() {
    let root = tempfile::tempdir().unwrap();
    // vendor_boot_b ends at 1713; the marker spans the free tail to 1999.
    let mut device = marked(
        Fake::with_recovery(600, true),
        "last_parti",
        1714,
        286,
        false,
    );
    let backup = device.backup(root.path(), Slot::A);
    backup.verify().unwrap();
    assert!(device.writes.is_empty());
    assert!(!backup.path().join("Partition/last_parti.img").exists());
    let table = storage::validate_gpt(&device.disk[4][..6 * SECTOR_SIZE]).unwrap();
    let modified = table.remove_region_vbmeta(Slot::A).unwrap();
    let table = storage::validate_gpt(&modified).unwrap();
    assert!(table.find("vbmeta_a").is_none());
    assert!(table.find("pvmfw_a").is_none());
    for name in ["vbmeta_b", "pvmfw_b"] {
        assert!(table.find(name).is_some());
    }
    backup.verify().unwrap();
}
