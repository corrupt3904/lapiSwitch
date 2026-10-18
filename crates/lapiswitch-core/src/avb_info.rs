//! AVB metadata the S0/S1 checks read from firmware images: build
//! fingerprint, signing-key class and rollback index.
//!
//! Ported from LTBox `ltbox-patch` (`avb::extract_image_avb_info`,
//! `avb::build_fingerprint`, `key_map::classify_pubkey`), on the same
//! `avbtool-rs` tag.

use std::path::Path;

/// AOSP testkeys, by public-key SHA-1 (LTBox `KEY_MAP`).
const TESTKEY_SHA1S: [&str; 2] = [
    "2597c218aae470a130f61162feaae70afd97f011", // testkey_rsa4096
    "cdbb77177f731920bbe0a0f94f84d9038ae0617d", // testkey_rsa2048
];

/// Lenovo production key, by public-key SHA-1 (LTBox `LENOVO_KEY_MAP`).
const LENOVO_KEY_SHA1S: [&str; 1] = ["8fcb864f11f53ed11284615fb67685522085d3a2"];

/// Root of trust an image is signed with. Device key classes are informational;
/// `Unknown` firmware is refused, as in LTBox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyClass {
    Testkey,
    Lenovo,
    Unknown,
}

pub fn classify_pubkey(sha1: Option<&str>) -> KeyClass {
    let Some(sha1) = sha1.map(str::trim).filter(|s| !s.is_empty()) else {
        return KeyClass::Unknown;
    };
    if TESTKEY_SHA1S.iter().any(|k| k.eq_ignore_ascii_case(sha1)) {
        KeyClass::Testkey
    } else if LENOVO_KEY_SHA1S
        .iter()
        .any(|k| k.eq_ignore_ascii_case(sha1))
    {
        KeyClass::Lenovo
    } else {
        KeyClass::Unknown
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{path}: {message}")]
pub struct AvbReadError {
    pub path: String,
    pub message: String,
}

/// The fields of an image's (or a vbmeta image's own) AVB header this tool uses.
#[derive(Debug, Clone)]
pub struct AvbSummary {
    pub rollback_index: u64,
    pub rollback_index_location: u32,
    pub public_key_sha1: Option<String>,
    pub props: Vec<(String, Vec<u8>)>,
}

pub fn read_avb(path: &Path) -> Result<AvbSummary, AvbReadError> {
    let info = avbtool_rs::image::inspect_avb_image(path).map_err(|error| AvbReadError {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    let props = info
        .descriptors
        .iter()
        .filter_map(|descriptor| match descriptor {
            avbtool_rs::info::DescriptorInfo::Property { key, value } => {
                Some((key.clone(), value.clone()))
            }
            _ => None,
        })
        .collect();
    Ok(AvbSummary {
        rollback_index: info.header.rollback_index,
        rollback_index_location: info.header.rollback_index_location,
        public_key_sha1: info.public_key_sha1.clone(),
        props,
    })
}

fn property_text(raw: &[u8]) -> Option<String> {
    std::str::from_utf8(raw)
        .ok()
        .map(|s| s.trim_end_matches('\0').trim().to_string())
        .filter(|s| !s.is_empty())
}

impl AvbSummary {
    pub fn key_class(&self) -> KeyClass {
        classify_pubkey(self.public_key_sha1.as_deref())
    }

    /// Model/display selection prefers system_ext, then the legacy LTBox
    /// `build_fingerprint`: prefer `com.android.build.system.fingerprint`
    /// when it names a known model, else the first `*.fingerprint` that does,
    /// else the system one, else the first one.
    pub fn build_fingerprint(&self) -> Option<String> {
        let known = ltbox_core::model::fingerprint_names_known_model;
        let system_ext = self.property("com.android.build.system_ext.fingerprint");
        if let Some(fp) = &system_ext
            && known(fp)
        {
            return Some(fp.clone());
        }
        let system = self
            .props
            .iter()
            .filter(|(key, _)| key == "com.android.build.system.fingerprint")
            .find_map(|(_, value)| property_text(value));
        if let Some(system) = &system
            && known(system)
        {
            return Some(system.clone());
        }
        let mut first = None;
        for value in self
            .props
            .iter()
            .filter(|(key, _)| key.ends_with(".fingerprint"))
            .filter_map(|(_, value)| property_text(value))
        {
            if known(&value) {
                return Some(value);
            }
            first.get_or_insert(value);
        }
        system_ext.or(system).or(first)
    }

    fn property(&self, name: &str) -> Option<String> {
        self.props
            .iter()
            .filter(|(key, _)| key == name)
            .find_map(|(_, value)| property_text(value))
    }

    /// Admission must inspect system_ext itself, including a custom/GSI
    /// value. Never hide it behind a stock fingerprint from another partition.
    /// Old images without this property retain their system fallback.
    pub fn version_fingerprint(&self) -> Option<String> {
        let key = "com.android.build.system_ext.fingerprint";
        if self.props.iter().any(|(name, _)| name == key) {
            self.property(key)
        } else {
            self.property("com.android.build.system.fingerprint")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_the_keys_seen_in_tb520fu_firmware() {
        // PRC 186: vbmeta_system testkey 2048, vbmeta testkey 4096; ROW: Lenovo.
        assert_eq!(
            classify_pubkey(Some("cdbb77177f731920bbe0a0f94f84d9038ae0617d")),
            KeyClass::Testkey
        );
        assert_eq!(
            classify_pubkey(Some("2597C218AAE470A130F61162FEAAE70AFD97F011")),
            KeyClass::Testkey
        );
        assert_eq!(
            classify_pubkey(Some("8fcb864f11f53ed11284615fb67685522085d3a2")),
            KeyClass::Lenovo
        );
        assert_eq!(classify_pubkey(Some("00")), KeyClass::Unknown);
        assert_eq!(classify_pubkey(Some(" ")), KeyClass::Unknown);
        assert_eq!(classify_pubkey(None), KeyClass::Unknown);
    }

    fn summary(props: &[(&str, &str)]) -> AvbSummary {
        AvbSummary {
            rollback_index: 0,
            rollback_index_location: 0,
            public_key_sha1: None,
            props: props
                .iter()
                .map(|(k, v)| (k.to_string(), format!("{v}\0").into_bytes()))
                .collect(),
        }
    }

    #[test]
    fn prefers_the_system_fingerprint() {
        let fp = "qti/TB520FU/TB520FU:16/BQ2A/ZUXOS_1.5.10.186_260408_PRC:user/release-keys";
        let s = summary(&[
            (
                "com.android.build.product.fingerprint",
                "x/other:16/y:user/release-keys",
            ),
            ("com.android.build.system.fingerprint", fp),
        ]);
        assert_eq!(s.build_fingerprint().as_deref(), Some(fp));
    }

    #[test]
    fn falls_back_to_a_fingerprint_naming_a_known_model() {
        let fp = "Lenovo/TB520FU/TB520FU:14/UKQ1/ZUI_17.5.10.309_260528_ROW:user/release-keys";
        let s = summary(&[
            (
                "com.android.build.system.fingerprint",
                "custom/foo:16/x:user/test-keys",
            ),
            ("com.android.build.vendor.fingerprint", fp),
        ]);
        assert_eq!(s.build_fingerprint().as_deref(), Some(fp));
        assert_eq!(summary(&[]).build_fingerprint(), None);
    }

    #[test]
    fn system_ext_carries_the_current_rom_version_when_system_only_has_a_number() {
        let system = "qti/TB520FU/TB520FU:17/CQ2A.260712.001/37:user/release-keys";
        let ext =
            "qti/TB520FU/TB520FU:17/CQ2A.260712.001/ZUI_18.5.10.037_260929_ROW:user/release-keys";
        let s = summary(&[
            ("com.android.build.system.fingerprint", system),
            ("com.android.build.system_ext.fingerprint", ext),
        ]);
        assert_eq!(s.build_fingerprint().as_deref(), Some(ext));
        assert_eq!(s.version_fingerprint().as_deref(), Some(ext));
        assert_eq!(
            crate::device_version::check(ext).unwrap(),
            "ROW ZUI_18.5.10.037"
        );
    }

    #[test]
    fn custom_system_ext_is_not_replaced_by_a_stock_system_for_admission() {
        let system = "qti/TB520FU/TB520FU:16/BUILD/ZUI_17.5.10.309_260528_ROW:user/release-keys";
        let custom = "generic/gsi/arm64:17/BUILD/custom:userdebug/test-keys";
        let s = summary(&[
            ("com.android.build.system.fingerprint", system),
            ("com.android.build.system_ext.fingerprint", custom),
        ]);
        // Model evidence remains independent from ROM-version admission.
        assert_eq!(s.build_fingerprint().as_deref(), Some(system));
        assert_eq!(s.version_fingerprint().as_deref(), Some(custom));
        assert!(crate::device_version::check(custom).is_err());
        assert_eq!(
            summary(&[("com.android.build.system.fingerprint", system)])
                .version_fingerprint()
                .as_deref(),
            Some(system)
        );
        assert_eq!(
            summary(&[
                ("com.android.build.system.fingerprint", system),
                ("com.android.build.system_ext.fingerprint", "")
            ])
            .version_fingerprint(),
            None
        );
    }
}
