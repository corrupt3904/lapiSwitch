//! A supported device: a folder in `devices/` with `flow = true`. Model
//! names, firmware rules, the pinned TWRP and the processing methods come
//! from its `device.toml`; code never branches on a model name.
use crate::deviceinfo::map::Layout;
use crate::region::Region;
use lapiswitch_devices::{Country, Flow, RegionCode, RegionMarker, Twrp, VersionScheme};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy)]
pub struct Device(&'static lapiswitch_devices::Device);

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.model() == other.model()
    }
}
impl Eq for Device {}
impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.model())
    }
}
impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.model())
    }
}

/// Saved runs and backups record the model name.
impl Serialize for Device {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.model())
    }
}
impl<'de> Deserialize<'de> for Device {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let model = String::deserialize(deserializer)?;
        Self::find(&model)
            .ok_or_else(|| serde::de::Error::custom(format!("지원하지 않는 기기: {model}")))
    }
}

impl Device {
    /// Every supported device.
    pub fn all() -> impl Iterator<Item = Self> {
        lapiswitch_devices::flow_devices().map(Self)
    }
    /// A supported device by its exact model name.
    pub fn find(model: &str) -> Option<Self> {
        Self::all().find(|device| device.model().eq_ignore_ascii_case(model.trim()))
    }
    /// The one supported device named by an ADB model, codename or build
    /// fingerprint; `None` when none or several match.
    pub fn identify(text: &str) -> Option<Self> {
        let mut found = Self::all().filter(|device| device.matches(text));
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }
    /// The model or codename appears as a whole token of `text`.
    pub fn matches(self, text: &str) -> bool {
        let text = text.trim();
        ltbox_core::model::fingerprint_model_match(&text.to_ascii_uppercase(), self.model())
            || text
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|token| token.eq_ignore_ascii_case(self.codename()))
    }
    pub fn model(self) -> &'static str {
        self.0.model
    }
    /// TWRP's `ro.product.device`; the build requires one for flow devices.
    pub fn codename(self) -> &'static str {
        self.0.codename.expect("flow devices have a codename")
    }
    pub fn image(self) -> Option<&'static [u8]> {
        self.0.image
    }
    pub fn layout(self) -> Layout {
        Layout::of(self.0)
    }
    fn flow(self) -> &'static Flow {
        self.0.flow.as_ref().expect("only flow devices are listed")
    }
    pub fn loader(self) -> &'static str {
        self.flow().firmware.loader
    }
    pub fn required_images(self) -> &'static [&'static str] {
        self.flow().firmware.required_images
    }
    pub fn region_marker(self) -> RegionMarker {
        self.flow().firmware.region_marker
    }
    pub fn version_scheme(self) -> VersionScheme {
        self.flow().version_scheme
    }
    pub fn installed_firmware(self) -> &'static [lapiswitch_devices::InstalledFirmware] {
        self.flow().installed_firmware
    }
    pub fn twrp(self) -> &'static Twrp {
        &self.flow().twrp
    }
    pub fn country(self) -> Country {
        self.flow().country
    }
}

pub fn region(code: RegionCode) -> Region {
    match code {
        RegionCode::Prc => Region::Prc,
        RegionCode::Row => Region::Row,
    }
}

#[cfg(test)]
pub(crate) fn tb520fu() -> Device {
    Device::find("TB520FU").unwrap()
}
#[cfg(test)]
pub(crate) fn tb522fu() -> Device {
    Device::find("TB522FU").unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_are_whole_tokens_and_ambiguous_text_names_no_device() {
        for (device, codename) in [(tb520fu(), "lapis"), (tb522fu(), "mojito")] {
            assert_eq!(Device::identify(device.model()), Some(device));
            assert_eq!(Device::identify(codename), Some(device));
            assert_eq!(
                Device::identify(&format!(
                    "qti/{0}/{0}:17/id/build:user/release-keys",
                    device.model()
                )),
                Some(device)
            );
            assert_eq!(Device::identify(&format!("{}X", device.model())), None);
        }
        assert_eq!(Device::identify("TB520FU/TB522FU"), None);
        assert_eq!(Device::identify("TB321FU"), None);
        let json = serde_json::to_string(&tb522fu()).unwrap();
        assert_eq!(json, "\"TB522FU\"");
        assert_eq!(serde_json::from_str::<Device>(&json).unwrap(), tb522fu());
        assert!(serde_json::from_str::<Device>("\"TB999FU\"").is_err());
    }
}
