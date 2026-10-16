//! S0 on real TB520FU firmware packages, like LTBox's
//! `LTBOX_REAL_FIRMWARE_DIR` tests. Set either or both:
//!
//! - `TB520_FW_PRC` — a PRC package folder (or its `image/`)
//! - `TB520_FW_ROW` — a ROW package folder (or its `image/`)
//!
//! `check_firmware` decrypts `.x` rawprograms in place, as LTBox does.
//! Without the variables the test passes after printing that it was skipped.

use std::path::PathBuf;

use lapiswitch_core::firmware::{FirmwareError, check_firmware};
use lapiswitch_core::region::Region;

fn package(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

#[test]
fn real_packages_pass_s0_for_their_own_region_only() {
    let mut ran = false;
    for (var, region, other) in [
        ("TB520_FW_PRC", Region::Prc, Region::Row),
        ("TB520_FW_ROW", Region::Row, Region::Prc),
    ] {
        let Some(folder) = package(var) else {
            continue;
        };
        ran = true;
        let report = check_firmware(&folder, region).unwrap_or_else(|e| panic!("{var}: {e}"));
        assert_eq!(report.region, region);
        assert!(report.fingerprint.ends_with(":user/release-keys"));
        assert!(report.loader.ends_with("xbl_s_devprg_ns.melf"));
        assert!(report.lun4_gpt_in_package);
        // S6 reads these GPTs directly after flashing, bypassing stale caches.
        for (name, required) in [("gpt_main0.bin", "persist"), ("gpt_main4.bin", "devinfo")] {
            let bytes = std::fs::read(report.image_dir.join(name)).unwrap();
            // LUN0's package userdata end is a template value corrected by
            // patch0.xml on DISK. Validate its CRC/array here; S6 validates
            // full live geometry after LTBox applies the disk patches.
            let table = lapiswitch_core::gpt::PrimaryGpt::parse(&bytes).unwrap();
            if name == "gpt_main4.bin" {
                lapiswitch_core::storage::validate_gpt(&bytes).unwrap();
            }
            assert!(table.find(required).is_some(), "{var}: {required}");
        }
        let names: Vec<String> = report
            .raw_xmls
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"rawprogram4.xml".to_string()));
        assert!(names.contains(&"rawprogram_save_persist_unsparse0.xml".to_string()));
        assert!(
            report
                .missing_images
                .iter()
                .all(|m| m.filename.starts_with("vm-")),
            "{var}: unexpected missing images {:?}",
            report.missing_images
        );
        assert!(matches!(
            check_firmware(&folder, other),
            Err(FirmwareError::WrongRegion { .. })
        ));
    }
    if !ran {
        eprintln!("skipped: set TB520_FW_PRC / TB520_FW_ROW to run");
    }
}
