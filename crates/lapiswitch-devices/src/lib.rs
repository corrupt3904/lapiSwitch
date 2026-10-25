//! Supported devices. Each folder in `devices/` becomes one [`Device`]; the
//! build script reads and validates the files and generates [`DEVICES`].
//! A malformed device file fails the build, so everything here is checked.

/// How a DeviceInfo field's bytes are shown and entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One byte, 0 or 1.
    Flag,
    /// Little-endian unsigned integer of 1, 2, 4 or 8 bytes.
    Uint,
    /// "PRC" or "ROW", 3 bytes.
    Region,
    /// Printable ASCII, NUL padded.
    Text,
    /// Printable ASCII; the last byte stays NUL.
    TextNul,
    /// Raw bytes, entered as hex.
    Bytes,
}

/// Editing tier of a field: how much care a change needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Basic,
    Advanced,
    /// Structure markers and security state.
    Protected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub key: &'static str,
    pub offset: usize,
    pub size: usize,
    pub kind: Kind,
    pub class: Class,
    pub description: &'static str,
    /// `false` keeps the field as read; the app shows it without an input.
    pub editable: bool,
    /// The only values that may be written, as shown (decimal for numbers).
    /// Empty accepts any value of the kind.
    pub choices: &'static [&'static str],
    /// A `uint` shown and entered in decimal only.
    pub decimal: bool,
}

/// The logical DeviceInfo structure: fields in offset order, without gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceInfoMap {
    pub logical_size: usize,
    pub fields: &'static [Field],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionCode {
    Prc,
    Row,
}

/// How the installed firmware's version and region are read from the
/// incremental of its build fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionScheme {
    /// `ZUI_18.5.10.037_260408_ROW`: family, four-part version, region token.
    ZuiVersion,
    /// `10.028.260924C`: build date `YYMMDD` and a region suffix letter.
    BuildDate,
}

/// One accepted kind of installed firmware and its optional ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledFirmware {
    Version {
        region: RegionCode,
        family: &'static str,
        /// Highest accepted version and its label; `None` accepts all.
        max: Option<([u32; 4], &'static str)>,
    },
    BuildDate {
        region: RegionCode,
        /// Last letter of the incremental, e.g. `C` (PRC) or `W` (ROW).
        suffix: char,
        /// Latest accepted build date (`YYYYMMDD`) and its label.
        max: Option<(u32, &'static str)>,
    },
}

impl InstalledFirmware {
    pub fn region(&self) -> RegionCode {
        match self {
            Self::Version { region, .. } | Self::BuildDate { region, .. } => *region,
        }
    }
}

/// Where the firmware package's vendor_boot DTB names its region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionMarker {
    /// `region_country` property of the root node.
    RegionCountry,
    /// `build_region` property of the `product_region` node.
    ProductRegionBuildRegion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwarePackage {
    pub loader: &'static str,
    pub required_images: &'static [&'static str],
    pub region_marker: RegionMarker,
}

/// A pinned TWRP release, verified by size and SHA-256 on every use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Twrp {
    pub tag: &'static str,
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub url: &'static str,
}

/// A fixed country-code field inside a dedicated partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OemOwnInfo {
    pub partition: &'static str,
    pub lun: u8,
    pub size: usize,
    pub offset: usize,
    pub field_size: usize,
}

/// How the country code is changed after firmware installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Country {
    /// devinfo and persist patching (LTBox).
    DevinfoPersist,
    /// One field of a partition, which is also backed up before any write.
    Oemowninfo(OemOwnInfo),
}

/// What the backup/TWRP/DeviceInfo/restore flow needs for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flow {
    pub firmware: FirmwarePackage,
    pub version_scheme: VersionScheme,
    pub installed_firmware: &'static [InstalledFirmware],
    pub twrp: Twrp,
    pub country: Country,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Device {
    pub model: &'static str,
    pub codename: Option<&'static str>,
    /// DeviceInfo can be read and dumped; the backup, TWRP and restore flow
    /// runs for the device.
    pub deviceinfo_read: bool,
    /// DeviceInfo can be written: firmware flashing and DeviceInfo editing.
    /// The device is supported only with this flag.
    pub deviceinfo_write: bool,
    /// Present exactly when `deviceinfo_read` is set.
    pub flow: Option<Flow>,
    /// `devices/<MODEL>/image.png`, shown on the Dashboard.
    pub image: Option<&'static [u8]>,
    /// `devices/<MODEL>/deviceinfo.toml`, required to write.
    pub deviceinfo: Option<DeviceInfoMap>,
}

include!(concat!(env!("OUT_DIR"), "/devices.rs"));

/// A device by model name, case-insensitively.
pub fn find(model: &str) -> Option<&'static Device> {
    DEVICES
        .iter()
        .find(|device| device.model.eq_ignore_ascii_case(model.trim()))
}

/// Devices the workflow runs for (`deviceinfo_read = true`).
pub fn flow_devices() -> impl Iterator<Item = &'static Device> {
    DEVICES.iter().filter(|device| device.flow.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_device_map_covers_its_logical_size_and_has_the_required_fields() {
        assert!(!DEVICES.is_empty());
        for device in DEVICES {
            assert_eq!(device.flow.is_some(), device.deviceinfo_read);
            assert!(!device.deviceinfo_write || device.deviceinfo_read);
            let Some(map) = &device.deviceinfo else {
                assert!(!device.deviceinfo_write, "{}", device.model);
                continue;
            };
            let mut next = 0;
            for field in map.fields {
                assert_eq!(field.offset, next, "{} {}", device.model, field.key);
                next += field.size;
            }
            assert_eq!(next, map.logical_size, "{}", device.model);
            let field = |key: &str| map.fields.iter().find(|f| f.key == key).unwrap();
            assert_eq!(field("magic").offset, 0);
            assert_eq!(field("region").kind, Kind::Region);
            for n in 0..32 {
                assert_eq!(field(&format!("rollback_index[{n}]")).size, 8);
            }
        }
    }

    #[test]
    fn both_tablets_run_the_workflow_with_their_own_methods() {
        let models: Vec<_> = flow_devices().map(|d| d.model).collect();
        assert_eq!(models, ["TB520FU", "TB522FU"]);
        let lapis = find("tb520fu").unwrap().flow.unwrap();
        let mojito = find("TB522FU").unwrap().flow.unwrap();
        assert_eq!(lapis.version_scheme, VersionScheme::ZuiVersion);
        assert_eq!(mojito.version_scheme, VersionScheme::BuildDate);
        assert_eq!(lapis.firmware.region_marker, RegionMarker::RegionCountry);
        assert_eq!(
            mojito.firmware.region_marker,
            RegionMarker::ProductRegionBuildRegion
        );
        assert_eq!(lapis.country, Country::DevinfoPersist);
        assert!(matches!(mojito.country, Country::Oemowninfo(o) if o.offset == 0xE4));
        assert_ne!(lapis.twrp.sha256, mojito.twrp.sha256);
        assert!(find("TB520FU").unwrap().image.is_some());
        assert!(find("TB522FU").unwrap().image.is_some());
        // TB522FU is dumped only until its DeviceInfo writes are validated.
        assert!(find("TB520FU").unwrap().deviceinfo_write);
        assert!(!find("TB522FU").unwrap().deviceinfo_write);
        assert!(find("TB522FU").unwrap().deviceinfo_read);
        assert!(find("TB999FU").is_none());
    }
}
