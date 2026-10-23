//! Anti-rollback comparison of the firmware's `boot` and `vbmeta_system`
//! rollback indices with the values stored in the device's DeviceInfo. The
//! comparison runs in TWRP for Keep mode only; Reset mode writes zeros. This
//! pure function never changes a device.

/// Rollback indices of the two AVB locations the TB520FU commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RollbackIndices {
    pub boot: u64,
    pub vbmeta_system: u64,
}

/// Which image is below the device's stored index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Downgrade {
    pub partition: &'static str,
    pub firmware: u64,
    pub floor: u64,
}

/// `Err` lists every image whose firmware index is below the device's.
pub fn check(firmware: RollbackIndices, device: RollbackIndices) -> Result<(), Vec<Downgrade>> {
    let downgrades: Vec<Downgrade> = [
        ("boot", firmware.boot, device.boot),
        (
            "vbmeta_system",
            firmware.vbmeta_system,
            device.vbmeta_system,
        ),
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

/// `boot: 펌웨어 N < 기기 M / vbmeta_system: …` with exact decimal indices.
pub fn describe(downgrades: &[Downgrade]) -> String {
    downgrades
        .iter()
        .map(|image| {
            crate::msg!(
                "core_rollback_downgrade",
                partition = image.partition,
                firmware = image.firmware,
                floor = image.floor
            )
        })
        .collect::<Vec<_>>()
        .join(" / ")
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

    fn both(index: u64) -> RollbackIndices {
        RollbackIndices {
            boot: index,
            vbmeta_system: index,
        }
    }

    #[test]
    fn same_or_newer_firmware_passes() {
        assert_eq!(check(both(APR_05), both(APR_05)), Ok(()));
        assert_eq!(check(both(JUL_05), both(APR_05)), Ok(()));
        // A never-committed device stores zeros.
        assert_eq!(check(both(APR_05), both(0)), Ok(()));
    }

    #[test]
    fn row_362_to_prc_186_is_reported_for_both_images() {
        let downgrades = check(both(APR_05), both(JUL_05)).unwrap_err();
        assert_eq!(downgrades.len(), 2);
        assert_eq!(downgrades[0].partition, "boot");
        assert_eq!(downgrades[1].partition, "vbmeta_system");
        assert_eq!(
            describe(&downgrades),
            format!(
                "boot: firmware {APR_05} < device {JUL_05} / vbmeta_system: firmware {APR_05} < device {JUL_05}"
            )
        );
    }

    #[test]
    fn one_location_below_is_enough() {
        let device = RollbackIndices {
            boot: 10,
            vbmeta_system: 20,
        };
        let fw = RollbackIndices {
            boot: 30,
            vbmeta_system: 19,
        };
        assert_eq!(
            check(fw, device),
            Err(vec![Downgrade {
                partition: "vbmeta_system",
                firmware: 19,
                floor: 20,
            }])
        );
    }

    #[test]
    fn indices_read_as_build_dates() {
        assert_eq!(index_date(APR_05), "2026-04-05");
        assert_eq!(index_date(JUL_05), "2026-07-05");
        assert_eq!(index_date(u64::MAX), u64::MAX.to_string());
    }
}
