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
    /// "PRC" or "ROW", NUL padded, 16 bytes.
    Region,
    /// Printable ASCII, NUL padded.
    Text,
    /// Printable ASCII; the last byte stays NUL.
    TextNul,
    /// Raw bytes, entered as hex.
    Bytes,
}

/// Editing tier of a field. Every tier is editable.
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

/// One accepted family of the firmware installed on the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstalledFirmware {
    pub region: RegionCode,
    pub family: &'static str,
    /// Highest accepted version and its display label; `None` accepts all.
    pub max: Option<([u32; 4], &'static str)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwarePackage {
    pub loader: &'static str,
    pub required_images: &'static [&'static str],
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

/// What the backup/TWRP/DeviceInfo/restore flow needs for a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flow {
    pub firmware: FirmwarePackage,
    pub installed_firmware: &'static [InstalledFirmware],
    pub twrp: Twrp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Device {
    pub model: &'static str,
    pub codename: Option<&'static str>,
    /// `None`: only the DeviceInfo map is defined.
    pub flow: Option<Flow>,
    pub deviceinfo: DeviceInfoMap,
}

impl Device {
    /// The flow data; only devices returned by [`flow_device`] are used here.
    pub fn flow(&self) -> &Flow {
        self.flow
            .as_ref()
            .expect("the build guarantees flow data for the flow device")
    }
}

include!(concat!(env!("OUT_DIR"), "/devices.rs"));

/// A device by model name, case-insensitively.
pub fn find(model: &str) -> Option<&'static Device> {
    DEVICES
        .iter()
        .find(|device| device.model.eq_ignore_ascii_case(model.trim()))
}

/// The device the workflow runs for. The build allows exactly one.
pub fn flow_device() -> &'static Device {
    DEVICES
        .iter()
        .find(|device| device.flow.is_some())
        .expect("the build guarantees one flow device")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_device_map_covers_its_logical_size_and_has_the_required_fields() {
        assert!(!DEVICES.is_empty());
        for device in DEVICES {
            let map = &device.deviceinfo;
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
    fn exactly_the_flow_device_has_workflow_data() {
        assert_eq!(DEVICES.iter().filter(|d| d.flow.is_some()).count(), 1);
        let device = flow_device();
        assert_eq!(device.model, "TB520FU");
        assert_eq!(find("tb520fu"), Some(device));
        assert_eq!(device.flow().twrp.sha256.len(), 64);
        assert!(find("TB522FU").is_some_and(|d| d.flow.is_none()));
        assert!(find("TB999FU").is_none());
    }
}
