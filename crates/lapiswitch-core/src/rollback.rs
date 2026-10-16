//! Anti-rollback check: refuse firmware whose
//! `boot` or `vbmeta_system` rollback index is below the device floor. A
//! downgrade is blocked in Keep mode and legacy workflows. The new recovery
//! workflow may explicitly reset DeviceInfo indices before flashing (user policy
//! 2026-10-16); this pure comparison function never changes a device.
//!
//! Floor classification is ported from LTBox `ltbox-patch/src/rollback.rs`
//! (`classify_fastboot_rollback_floors`, `compute_device_rollback_index`) and
//! the EDL estimate from `workers/flash/mod.rs` (`rollback_floors`).

use std::collections::HashMap;

/// Rollback indices of the two AVB locations the TB520FU commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RollbackIndices {
    pub boot: u64,
    pub vbmeta_system: u64,
}

/// Device floor as far as it could be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeviceFloors {
    /// Exactly two meaningful fastboot locations (TB520FU: 2 = vbmeta_system,
    /// 3 = boot), or the per-location EDL estimate.
    PerLocation(RollbackIndices),
    /// Any other fastboot layout: LTBox's conservative `max(value > 1)`,
    /// applied to both images.
    Aggregate(u64),
    /// Every location is stock 0/1 — nothing committed.
    None,
}

/// Classify fastboot `stored_rollback_index:N` values. Location 1 (recovery)
/// and values 0/1 are ignored; of the rest, the lower location is
/// `vbmeta_system` and the higher is `boot`.
pub fn floors_from_fastboot(stored: &HashMap<u32, u64>) -> DeviceFloors {
    let mut candidates: Vec<(u32, u64)> = stored
        .iter()
        .filter(|&(&location, &index)| location != 1 && index > 1)
        .map(|(&location, &index)| (location, index))
        .collect();
    candidates.sort_unstable();
    if let [(_, vbmeta_system), (_, boot)] = candidates.as_slice() {
        return DeviceFloors::PerLocation(RollbackIndices {
            boot: *boot,
            vbmeta_system: *vbmeta_system,
        });
    }
    // Like LTBox's aggregate, this one counts location 1 as well.
    match stored.values().copied().filter(|&index| index > 1).max() {
        Some(max) => DeviceFloors::Aggregate(max),
        None => DeviceFloors::None,
    }
}

/// EDL estimate (device started in EDL): each location's floor is the max of
/// the AVB index installed on slot a and slot b. `None` when a location parsed
/// on neither slot — the caller must abort.
pub fn floors_from_edl(
    boot: [Option<u64>; 2],
    vbmeta_system: [Option<u64>; 2],
) -> Option<RollbackIndices> {
    Some(RollbackIndices {
        boot: boot.into_iter().flatten().max()?,
        vbmeta_system: vbmeta_system.into_iter().flatten().max()?,
    })
}

/// Which image is below the device floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Downgrade {
    pub partition: &'static str,
    pub firmware: u64,
    pub floor: u64,
}

/// R-check. `Err` lists every image below its floor.
pub fn check(firmware: RollbackIndices, floors: DeviceFloors) -> Result<(), Vec<Downgrade>> {
    let floor = match floors {
        DeviceFloors::None => return Ok(()),
        DeviceFloors::PerLocation(floor) => floor,
        DeviceFloors::Aggregate(max) => RollbackIndices {
            boot: max,
            vbmeta_system: max,
        },
    };
    let downgrades: Vec<Downgrade> = [
        ("boot", firmware.boot, floor.boot),
        ("vbmeta_system", firmware.vbmeta_system, floor.vbmeta_system),
    ]
    .into_iter()
    .filter(|&(_, firmware, floor)| firmware < floor)
    .map(|(partition, firmware, floor)| Downgrade {
        partition,
        firmware,
        floor,
    })
    .collect();
    if downgrades.is_empty() {
        Ok(())
    } else {
        Err(downgrades)
    }
}

/// TB520FU rollback indices are Unix build timestamps; show them as a UTC date
/// (LTBox `format_unix_date_utc`). Values outside the calendar print as-is.
pub fn index_date(index: u64) -> String {
    i64::try_from(index)
        .ok()
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| index.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const APR_05: u64 = 0x69D1_A600; // 2026-04-05: PRC 186, ROW 309
    const JUL_05: u64 = 1_783_209_600; // 2026-07-05: ROW 362

    fn stored(values: &[(u32, u64)]) -> HashMap<u32, u64> {
        values.iter().copied().collect()
    }

    #[test]
    fn classifies_the_real_tb520fu_getvar_dump() {
        // LTBox fixture: 32 locations, 1 = recovery `1`, 2 and 3 committed.
        let mut values: HashMap<u32, u64> = (0..=31).map(|loc| (loc, 0)).collect();
        values.insert(1, 1);
        values.insert(2, APR_05);
        values.insert(3, APR_05);
        assert_eq!(
            floors_from_fastboot(&values),
            DeviceFloors::PerLocation(RollbackIndices {
                boot: APR_05,
                vbmeta_system: APR_05,
            })
        );
    }

    #[test]
    fn lower_location_is_vbmeta_system() {
        assert_eq!(
            floors_from_fastboot(&stored(&[(0, 0), (1, 1), (2, 10), (3, 11)])),
            DeviceFloors::PerLocation(RollbackIndices {
                boot: 11,
                vbmeta_system: 10,
            })
        );
        assert_eq!(
            floors_from_fastboot(&stored(&[(1, 999), (8, 10), (9, 11)])),
            DeviceFloors::PerLocation(RollbackIndices {
                boot: 11,
                vbmeta_system: 10,
            })
        );
    }

    #[test]
    fn other_layouts_fall_back_to_the_aggregate_or_none() {
        assert_eq!(
            floors_from_fastboot(&stored(&[(2, 10), (3, 11), (4, 12)])),
            DeviceFloors::Aggregate(12)
        );
        assert_eq!(
            floors_from_fastboot(&stored(&[(2, 10)])),
            DeviceFloors::Aggregate(10)
        );
        assert_eq!(
            floors_from_fastboot(&stored(&[(0, 0), (1, 1), (4, 1)])),
            DeviceFloors::None
        );
        assert_eq!(floors_from_fastboot(&stored(&[])), DeviceFloors::None);
        // Only recovery committed: no pair to classify, but LTBox's aggregate
        // still counts it.
        assert_eq!(
            floors_from_fastboot(&stored(&[(1, 5)])),
            DeviceFloors::Aggregate(5)
        );
    }

    #[test]
    fn edl_estimate_takes_the_max_per_location() {
        assert_eq!(
            floors_from_edl([Some(5), Some(7)], [None, Some(3)]),
            Some(RollbackIndices {
                boot: 7,
                vbmeta_system: 3,
            })
        );
        assert_eq!(floors_from_edl([Some(5), None], [None, None]), None);
    }

    #[test]
    fn same_or_newer_firmware_passes() {
        let floor = DeviceFloors::PerLocation(RollbackIndices {
            boot: APR_05,
            vbmeta_system: APR_05,
        });
        let same = RollbackIndices {
            boot: APR_05,
            vbmeta_system: APR_05,
        };
        assert_eq!(check(same, floor), Ok(()));
        let newer = RollbackIndices {
            boot: JUL_05,
            vbmeta_system: JUL_05,
        };
        assert_eq!(check(newer, floor), Ok(()));
        assert_eq!(check(same, DeviceFloors::None), Ok(()));
    }

    #[test]
    fn row_362_to_prc_186_is_blocked() {
        let floor = DeviceFloors::PerLocation(RollbackIndices {
            boot: JUL_05,
            vbmeta_system: JUL_05,
        });
        let prc_186 = RollbackIndices {
            boot: APR_05,
            vbmeta_system: APR_05,
        };
        let downgrades = check(prc_186, floor).unwrap_err();
        assert_eq!(downgrades.len(), 2);
        assert_eq!(downgrades[0].partition, "boot");
        assert_eq!(downgrades[1].partition, "vbmeta_system");
    }

    #[test]
    fn one_location_below_is_enough_to_block() {
        let floor = DeviceFloors::PerLocation(RollbackIndices {
            boot: 10,
            vbmeta_system: 20,
        });
        let fw = RollbackIndices {
            boot: 30,
            vbmeta_system: 19,
        };
        assert_eq!(
            check(fw, floor),
            Err(vec![Downgrade {
                partition: "vbmeta_system",
                firmware: 19,
                floor: 20,
            }])
        );
        // The aggregate floor applies to both images.
        assert!(check(fw, DeviceFloors::Aggregate(25)).is_err());
    }

    #[test]
    fn indices_read_as_build_dates() {
        assert_eq!(index_date(APR_05), "2026-04-05");
        assert_eq!(index_date(JUL_05), "2026-07-05");
        assert_eq!(index_date(u64::MAX), u64::MAX.to_string());
    }
}
