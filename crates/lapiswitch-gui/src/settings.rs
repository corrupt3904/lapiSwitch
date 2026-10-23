use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Every language's text lives in `lang/<code>.json`, keyed by identifiers
/// such as `view_final_check`. A key missing in a language falls back to
/// English, then to the key itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    Korean,
    English,
    Chinese,
    Japanese,
    Russian,
}

type Catalog = HashMap<&'static str, &'static str>;

/// Parse once per process; the leaked strings live as long as the app.
fn leak_catalog(json: &'static str) -> Catalog {
    let parsed: HashMap<String, String> = serde_json::from_str(json).expect("bundled translations");
    parsed
        .into_iter()
        .map(|(k, v)| {
            let k: &'static str = Box::leak(k.into_boxed_str());
            let v: &'static str = Box::leak(v.into_boxed_str());
            (k, v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn rust_sources(dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(std::fs::read_to_string(path).unwrap());
            }
        }
    }

    fn placeholders(s: &str) -> Vec<String> {
        let rx = regex::Regex::new(r"\{[A-Za-z_]*\}").unwrap();
        let mut found: Vec<String> = rx.find_iter(s).map(|m| m.as_str().to_owned()).collect();
        found.sort();
        found
    }

    fn catalog(language: Language) -> HashMap<String, String> {
        serde_json::from_str(language.json()).unwrap()
    }

    /// Every key the GUI asks for exists in English, every language has
    /// exactly the English keys with the same placeholders, and no key is
    /// left that neither the GUI nor the vendored LTBox crates use.
    #[test]
    fn every_language_has_every_key_the_sources_use() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut gui = vec![];
        rust_sources(&manifest.join("src"), &mut gui);
        let mut vendor = vec![];
        for dir in [
            "../../vendor/ltbox-core/src",
            "../../vendor/ltbox-device/src",
        ] {
            rust_sources(&manifest.join(dir), &mut vendor);
        }
        let call = regex::Regex::new(
            r#"(?s)(?:\btr|title)\(\s*(?:app\s*,\s*)?"([a-z0-9_]+)"(?:\s*,\s*"([a-z0-9_]+)")?"#,
        )
        .unwrap();
        let mut used = BTreeSet::new();
        for file in &gui {
            for c in call.captures_iter(file) {
                used.insert(c[1].to_owned());
                if let Some(second) = c.get(2) {
                    used.insert(second.as_str().to_owned());
                }
            }
        }
        assert!(used.len() > 200, "source scan found too few keys");
        let english = catalog(Language::English);
        let keys: BTreeSet<String> = english.keys().cloned().collect();
        let missing: Vec<_> = used.difference(&keys).collect();
        assert!(missing.is_empty(), "missing in en.json: {missing:#?}");
        let unused: Vec<_> = keys
            .iter()
            .filter(|key| !used.contains(*key))
            .filter(|key| {
                let quoted = format!("\"{key}\"");
                !vendor.iter().any(|file| file.contains(&quoted))
            })
            .collect();
        assert!(unused.is_empty(), "unused keys: {unused:#?}");
        for language in Language::ALL {
            let other = catalog(language);
            let other_keys: BTreeSet<String> = other.keys().cloned().collect();
            assert_eq!(other_keys, keys, "{language:?}");
            for (key, value) in &english {
                assert!(!other[key].trim().is_empty(), "{language:?}: {key}");
                assert_eq!(
                    placeholders(value),
                    placeholders(&other[key]),
                    "{language:?}: {key}"
                );
            }
        }
        assert_eq!(Language::Korean.tr("view_final_check"), "최종 확인");
        assert_eq!(Language::Russian.tr("Missing-key"), "Missing-key");
    }

    #[test]
    fn every_deviceinfo_field_description_is_translated() {
        let english: BTreeSet<String> = lapiswitch_core::device::Device::all()
            .filter_map(|device| device.layout())
            .flat_map(|layout| layout.fields())
            .map(|field| field.description.to_owned())
            .collect();
        assert!(english.len() > 20, "{english:?}");
        for language in [
            Language::Korean,
            Language::Chinese,
            Language::Japanese,
            Language::Russian,
        ] {
            let catalog = language.field_catalog().unwrap();
            let keys: BTreeSet<String> = catalog.keys().map(|k| (*k).to_owned()).collect();
            let missing: Vec<_> = english.difference(&keys).collect();
            let stale: Vec<_> = keys.difference(&english).collect();
            assert!(missing.is_empty(), "{language:?} missing: {missing:#?}");
            assert!(stale.is_empty(), "{language:?} unused: {stale:#?}");
            assert!(catalog.values().all(|v| !v.trim().is_empty()));
        }
        assert_eq!(
            Language::Korean.field_description("Bootloader unlock state"),
            "부트로더 잠금 해제 상태"
        );
        assert_eq!(
            Language::English.field_description("Bootloader unlock state"),
            "Bootloader unlock state"
        );
        assert_eq!(
            Language::Korean.field_description("Not mapped"),
            "Not mapped"
        );
    }

    #[test]
    fn saved_language_names_round_trip_and_show_their_own_names() {
        for language in Language::ALL {
            let json = serde_json::to_string(&language).unwrap();
            assert_eq!(serde_json::from_str::<Language>(&json).unwrap(), language);
        }
        assert_eq!(Language::Chinese.to_string(), "简体中文");
        assert_eq!(Language::Japanese.to_string(), "日本語");
        assert_eq!(Language::Russian.to_string(), "Русский");
    }

    #[test]
    fn preferences_without_a_storage_root_use_the_default_and_custom_paths_roundtrip() {
        let preferences: Preferences =
            serde_json::from_str(r#"{"kernel":false,"mask_serial":false}"#).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("lapiSwitch");
        assert_eq!(preferences.storage_root(&data), data);
        let mut preferences = preferences;
        let custom = dir.path().join("custom 한글");
        preferences.storage_root = Some(custom.clone());
        let file = dir.path().join("settings.json");
        preferences.save(&file).unwrap();
        let loaded = Preferences::load(&file);
        assert_eq!(loaded.storage_root(&data), custom);
        assert!(!loaded.kernel);
        assert!(!loaded.mask_serial);
    }
    #[test]
    fn driver_default_matches_ltbox_but_saved_userspace_choice_is_preserved() {
        assert_eq!(
            Preferences::default().kernel,
            ltbox_device::driver::kernel_mode_supported()
        );
        let saved: Preferences = serde_json::from_str(r#"{"kernel":false}"#).unwrap();
        assert!(!saved.kernel);
    }
}
impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Each name is written in its own language so anyone can find it.
        f.write_str(match self {
            Self::Korean => "한국어",
            Self::English => "English",
            Self::Chinese => "简体中文",
            Self::Japanese => "日本語",
            Self::Russian => "Русский",
        })
    }
}
impl Language {
    pub const ALL: [Self; 5] = [
        Self::Korean,
        Self::English,
        Self::Chinese,
        Self::Japanese,
        Self::Russian,
    ];
    fn json(self) -> &'static str {
        match self {
            Self::Korean => include_str!("../lang/ko.json"),
            Self::English => include_str!("../lang/en.json"),
            Self::Chinese => include_str!("../lang/zh.json"),
            Self::Japanese => include_str!("../lang/ja.json"),
            Self::Russian => include_str!("../lang/ru.json"),
        }
    }
    fn catalog(self) -> &'static Catalog {
        static CATALOGS: [OnceLock<Catalog>; 5] = [const { OnceLock::new() }; 5];
        let index = Self::ALL.iter().position(|l| *l == self).expect("listed");
        CATALOGS[index].get_or_init(|| leak_catalog(self.json()))
    }
    /// The text of `key`: this language, else English, else the key.
    pub fn tr(self, key: &'static str) -> &'static str {
        self.catalog()
            .get(key)
            .or_else(|| Self::English.catalog().get(key))
            .copied()
            .unwrap_or(key)
    }
    /// Translations of the DeviceInfo field descriptions, which the device
    /// files write in English.
    fn field_catalog(self) -> Option<&'static Catalog> {
        static KO: OnceLock<Catalog> = OnceLock::new();
        static ZH: OnceLock<Catalog> = OnceLock::new();
        static JA: OnceLock<Catalog> = OnceLock::new();
        static RU: OnceLock<Catalog> = OnceLock::new();
        let (cell, json) = match self {
            Self::English => return None,
            Self::Korean => (&KO, include_str!("../lang/deviceinfo/ko.json")),
            Self::Chinese => (&ZH, include_str!("../lang/deviceinfo/zh.json")),
            Self::Japanese => (&JA, include_str!("../lang/deviceinfo/ja.json")),
            Self::Russian => (&RU, include_str!("../lang/deviceinfo/ru.json")),
        };
        Some(cell.get_or_init(|| leak_catalog(json)))
    }
    /// A DeviceInfo field description in this language, or the English one.
    pub fn field_description(self, english: &'static str) -> &'static str {
        self.field_catalog()
            .and_then(|catalog| catalog.get(english).copied())
            .unwrap_or(english)
    }
    /// Backend crates log through `ltbox_core::i18n::tr`; give them the
    /// same catalog and fallback.
    pub fn install(self) {
        ltbox_core::i18n::set_translator(move |key| {
            self.catalog()
                .get(key)
                .or_else(|| Self::English.catalog().get(key))
                .map_or_else(|| key.to_owned(), |text| (*text).to_owned())
        });
    }
}
/// Light or dark appearance; `System` follows Windows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub language: Language,
    pub theme: ThemeChoice,
    pub mask_serial: bool,
    pub kernel: bool,
    pub storage_root: Option<PathBuf>,
    /// Where DeviceInfo dumps are saved, apart from logs and backups.
    pub dump_root: Option<PathBuf>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::Korean,
            theme: ThemeChoice::System,
            mask_serial: true,
            kernel: ltbox_device::driver::kernel_mode_supported(),
            storage_root: None,
            dump_root: None,
        }
    }
}
impl Preferences {
    pub fn storage_root(&self, data: &Path) -> PathBuf {
        self.storage_root
            .clone()
            .unwrap_or_else(|| data.to_path_buf())
    }
    pub fn load(path: &Path) -> Self {
        std::fs::metadata(path)
            .ok()
            .filter(|m| m.is_file() && m.len() < 16384)
            .and_then(|_| std::fs::read(path).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        std::fs::create_dir_all(path.parent().ok_or("설정 폴더 없음")?)
            .map_err(|e| e.to_string())?;
        std::fs::write(
            path,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}
