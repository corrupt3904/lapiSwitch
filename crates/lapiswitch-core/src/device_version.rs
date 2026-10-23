//! Device firmware admission at S3, from the active slot's AVB fingerprint.
//! This does not restrict the selected installation package or original restore.
use crate::device::Device;
use crate::region::Region;
use lapiswitch_devices::{InstalledFirmware, VersionScheme};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VersionError {
    #[error("{}", crate::msg!("core_version_unrecognized"))]
    Unrecognized,
    #[error("{}", crate::msg!("core_version_above_limit", display = .display, limit = .limit))]
    AboveLimit {
        display: String,
        limit: &'static str,
    },
}

/// Admission of the installed firmware: parse the incremental with the
/// device's `[installed].scheme` and compare with its `[[installed.allow]]`.
pub fn check(device: Device, fingerprint: &str) -> Result<String, VersionError> {
    match device.version_scheme() {
        VersionScheme::ZuiVersion => check_zui_version(device, fingerprint),
        VersionScheme::BuildDate => check_build_date(device, fingerprint),
    }
}

/// The region a firmware package's fingerprint states, read with the
/// device's scheme; `None` when it states none or both.
pub fn fingerprint_region(device: Device, fingerprint: &str) -> Option<Region> {
    match device.version_scheme() {
        VersionScheme::ZuiVersion => {
            let tokens: Vec<&str> = fingerprint
                .split(|c: char| !c.is_ascii_alphanumeric())
                .collect();
            match (tokens.contains(&"PRC"), tokens.contains(&"ROW")) {
                (true, false) => Some(Region::Prc),
                (false, true) => Some(Region::Row),
                _ => None,
            }
        }
        VersionScheme::BuildDate => {
            let (_, suffix, _) = build_date(fingerprint)?;
            device
                .installed_firmware()
                .iter()
                .find_map(|accepted| match accepted {
                    InstalledFirmware::BuildDate {
                        suffix: s, region, ..
                    } if *s == suffix => Some(crate::device::region(*region)),
                    _ => None,
                })
        }
    }
}

/// Android fingerprints: brand/product/device:release/build/incremental:type/tags.
/// Never mistake the Android release, build ID or build date for ROM version.
fn incremental(fingerprint: &str) -> Option<&str> {
    let mut fields = fingerprint.split(':');
    fields.next()?;
    let mut build = fields.next()?.split('/');
    let _android = build.next()?;
    let _id = build.next()?;
    let incremental = build.next()?;
    build.next().is_none().then_some(incremental)
}

fn check_zui_version(device: Device, fingerprint: &str) -> Result<String, VersionError> {
    let unknown = || VersionError::Unrecognized;
    let incremental = incremental(fingerprint).ok_or_else(unknown)?;
    let (prefix, region) = incremental.rsplit_once('_').ok_or_else(unknown)?;
    let region = match region {
        "PRC" => Region::Prc,
        "ROW" => Region::Row,
        _ => return Err(unknown()),
    };
    let mut tokens = prefix.split('_');
    let family = tokens.next().ok_or_else(unknown)?;
    let raw = tokens.next().ok_or_else(unknown)?;
    if let Some(date) = tokens.next()
        && (!matches!(date.len(), 6 | 8) || !date.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(unknown());
    }
    if tokens.next().is_some() {
        return Err(unknown());
    }
    let numbers: Vec<u32> = raw
        .split('.')
        .map(|p| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err(unknown());
            }
            p.parse().map_err(|_| unknown())
        })
        .collect::<Result<_, _>>()?;
    let version: [u32; 4] = numbers.try_into().map_err(|_| unknown())?;
    let limit = device
        .installed_firmware()
        .iter()
        .find_map(|accepted| match accepted {
            InstalledFirmware::Version {
                region: r,
                family: f,
                max,
            } if crate::device::region(*r) == region && *f == family => Some(*max),
            _ => None,
        })
        .ok_or_else(unknown)?;
    let display = format!("{} {family}_{raw}", region.token());
    if let Some((ceiling, label)) = limit
        && version > ceiling
    {
        return Err(VersionError::AboveLimit {
            display,
            limit: label,
        });
    }
    Ok(display)
}

/// `major.minor.YYMMDD<suffix>`, e.g. `10.028.260924C`: the incremental, the
/// suffix letter and the date as `YYYYMMDD`.
fn build_date(fingerprint: &str) -> Option<(&str, char, u32)> {
    let incremental = incremental(fingerprint)?;
    let parts: Vec<_> = incremental.split('.').collect();
    if parts.len() != 3
        || parts[..2]
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let tail = parts[2].as_bytes();
    if tail.len() != 7 || !tail[..6].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let date =
        chrono::NaiveDate::parse_from_str(&format!("20{}", &parts[2][..6]), "%Y%m%d").ok()?;
    let date: u32 = date.format("%Y%m%d").to_string().parse().ok()?;
    Some((incremental, char::from(tail[6]), date))
}

fn check_build_date(device: Device, fingerprint: &str) -> Result<String, VersionError> {
    let unknown = || VersionError::Unrecognized;
    let (incremental, suffix, date) = build_date(fingerprint).ok_or_else(unknown)?;
    let (region, limit) = device
        .installed_firmware()
        .iter()
        .find_map(|accepted| match accepted {
            InstalledFirmware::BuildDate {
                region,
                suffix: s,
                max,
            } if *s == suffix => Some((crate::device::region(*region), *max)),
            _ => None,
        })
        .ok_or_else(unknown)?;
    let display = format!(
        "{} {incremental} ({}-{:02}-{:02})",
        region.token(),
        date / 10000,
        date / 100 % 100,
        date % 100
    );
    if let Some((ceiling, label)) = limit
        && date > ceiling
    {
        return Err(VersionError::AboveLimit {
            display,
            limit: label,
        });
    }
    Ok(display)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fp(version: &str) -> String {
        format!(
            "qti/TB520FU/TB520FU:16/BQ2A.250610.001-BP2A.250605.031.A3/{version}:user/release-keys"
        )
    }
    #[test]
    fn ceilings_are_numeric_inclusive_and_apply_to_device_region() {
        for v in [
            "ZUXOS_1.5.10.185_260408_PRC",
            "ZUXOS_1.5.10.186_260408_PRC",
            "ZUXOS_1.5.9.999_260408_PRC",
            "ZUI_18.5.10.036_260408_ROW",
            "ZUI_18.5.10.037_260408_ROW",
            "ZUI_17.5.10.309_260528_ROW",
            "ZUI_16.1.10.027_241129_ROW",
            "ZUI_16.1.10.027_241129_PRC",
            "ZUI_17.5.10.309_260528_PRC",
        ] {
            assert!(check(crate::device::tb520fu(), &fp(v)).is_ok(), "{v}");
        }
        for v in [
            "ZUXOS_1.5.10.187_260408_PRC",
            "ZUXOS_1.6.0.0_260408_PRC",
            "ZUI_18.5.10.038_260408_ROW",
            "ZUI_19.1.0.0_260408_ROW",
        ] {
            assert!(
                matches!(
                    check(crate::device::tb520fu(), &fp(v)),
                    Err(VersionError::AboveLimit { .. })
                ),
                "{v}"
            );
        }
    }
    #[test]
    fn unidentified_or_conflicting_versions_never_pass() {
        for v in [
            "ROW",
            "build_ROW",
            "ZUXOS_1.5.10.186_260408_ROW",
            "ZUI_18.5.10_260408_ROW",
            "ZUI_18.5.x.037_260408_ROW",
            "ZUI_-18.5.10.037_260408_ROW",
            "ZUI_18.5.10.037_evil_ROW",
            "ZUI_18.5.10.037_260408_PRC_ROW",
        ] {
            assert!(check(crate::device::tb520fu(), &fp(v)).is_err(), "{v}");
        }
        assert!(
            check(
                crate::device::tb520fu(),
                "qti/TB520FU/TB520FU:16/ZUI_18.5.10.037_ROW:user/release-keys"
            )
            .is_err()
        );
    }
    fn mojito(value: &str) -> String {
        format!("qti/TB522FU/TB522FU:17/CQ2A.260712.001/{value}:user/release-keys")
    }
    #[test]
    fn build_date_caps_are_inclusive_per_region_and_not_version_order() {
        let device = crate::device::tb522fu();
        for s in [
            "15.026.260825C",
            "10.028.260924C",
            "99.999.260924C",
            "11.055.260904W",
            "10.999.260903W",
        ] {
            assert!(check(device, &mojito(s)).is_ok(), "{s}");
        }
        for s in ["1.001.260925C", "11.055.260905W", "1.000.270101C"] {
            assert!(
                matches!(
                    check(device, &mojito(s)),
                    Err(VersionError::AboveLimit { .. })
                ),
                "{s}"
            );
        }
        for s in [
            "10.028.260231C",
            "10.028.260924X",
            "10.028.260924C_EXTRA",
            "10.028.26092C",
            "abc.028.260924C",
            "ZUI_18.5.10.037_260408_ROW",
        ] {
            assert_eq!(
                check(device, &mojito(s)),
                Err(VersionError::Unrecognized),
                "{s}"
            );
        }
        assert_eq!(
            fingerprint_region(device, &mojito("10.028.260924C")),
            Some(Region::Prc)
        );
        assert_eq!(
            fingerprint_region(device, &mojito("11.055.260904W")),
            Some(Region::Row)
        );
        assert_eq!(fingerprint_region(device, &mojito("11.055.260904X")), None);
        // Each device reads only its own scheme.
        assert!(check(crate::device::tb520fu(), &mojito("10.028.260924C")).is_err());
    }
    #[test]
    fn version_fingerprint_region_is_a_whole_token() {
        let device = crate::device::tb520fu();
        let prc = "qti/TB520FU/TB520FU:16/BQ2A/ZUXOS_1.5.10.186_260408_PRC:user/release-keys";
        let row = "qti/TB520FU/TB520FU:16/BQ2A/ZUI_17.5.10.309_260528_ROW:user/release-keys";
        assert_eq!(fingerprint_region(device, prc), Some(Region::Prc));
        assert_eq!(fingerprint_region(device, row), Some(Region::Row));
        assert_eq!(fingerprint_region(device, "a/PRCX/ROWS:user"), None);
        assert_eq!(fingerprint_region(device, "a_PRC_ROW"), None);
    }
}
