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
    fn recent_deviceinfo_files_are_unique_latest_first_and_at_most_five() {
        let mut p = Preferences::default();
        for n in 0..7 {
            assert!(p.remember_deviceinfo(Path::new(&format!("{n}.bin"))));
        }
        let names = |p: &Preferences| -> Vec<String> {
            p.recent_deviceinfo
                .iter()
                .map(|path| path.display().to_string())
                .collect()
        };
        assert_eq!(names(&p), ["6.bin", "5.bin", "4.bin", "3.bin", "2.bin"]);
        // Again: moved to the front, never twice; first again: no change.
        assert!(p.remember_deviceinfo(Path::new("3.bin")));
        assert!(!p.remember_deviceinfo(Path::new("3.bin")));
        assert_eq!(names(&p), ["3.bin", "6.bin", "5.bin", "4.bin", "2.bin"]);
        assert!(p.forget_deviceinfo(Path::new("5.bin")));
        assert!(!p.forget_deviceinfo(Path::new("5.bin")));
        assert_eq!(names(&p), ["3.bin", "6.bin", "4.bin", "2.bin"]);
    }
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
            // Country names are looked up by code; countries.rs checks them.
            .filter(|key| !key.starts_with("country_"))
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

    /// Every name and help written in `text/en.toml` is written in every
    /// language too, so no screen falls back to English unnoticed.
    #[test]
    fn every_deviceinfo_text_is_written_in_every_language() {
        use lapiswitch_core::deviceinfo::map::TEXT_LANGUAGES;
        let mut checked = 0;
        for layout in lapiswitch_core::device::Device::all().filter_map(|device| device.layout()) {
            let mut entries = vec![
                ("@rollback".to_owned(), layout.rollback_help()),
                ("@persistent".to_owned(), layout.persistent_help()),
            ];
            for field in layout.fields() {
                entries.push((format!("{} name", field.key), field.names));
                entries.push((format!("{} help", field.key), field.helps));
            }
            for (what, texts) in entries {
                if texts[0].is_empty() {
                    continue;
                }
                for (text, code) in texts.iter().zip(TEXT_LANGUAGES) {
                    assert!(
                        !text.trim().is_empty(),
                        "{} text/{code}.toml: {what}",
                        layout.model()
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 40, "{checked}");
        let field = lapiswitch_core::deviceinfo::map::Layout::find("TB520FU")
            .unwrap()
            .field("is_unlocked")
            .unwrap();
        assert_eq!(
            Language::Korean.field_text(field.names),
            "부트로더 Unlock 플래그"
        );
        assert_eq!(
            Language::English.field_text(field.names),
            "Bootloader Unlock flag"
        );
        assert_eq!(
            Language::Korean.field_text(["Only English", "", "", "", ""]),
            "Only English"
        );
        // One shared file: help.<MODEL> gives each tablet its own text.
        let help = |model: &str| {
            let field = lapiswitch_core::deviceinfo::map::Layout::find(model)
                .unwrap()
                .field("fastboot_mode")
                .unwrap();
            Language::Korean.field_text(field.helps)
        };
        assert!(help("TB520FU").starts_with("0 = 허용"));
        assert!(help("TB522FU").starts_with("0 = 차단"));
        // A numbered entry gives one rollback index its own help; the
        // repeated entry covers the rest, with {n} as the index.
        let rollback = |n: usize| {
            let field = lapiswitch_core::deviceinfo::map::Layout::find("TB520FU")
                .unwrap()
                .field(&format!("rollback_index[{n}]"))
                .unwrap();
            Language::Korean.field_text(field.helps)
        };
        assert!(rollback(0).starts_with(
            "Rollback Index Location: 0 - vbmeta
"
        ));
        assert!(rollback(2).starts_with("Rollback Index Location: 2 - vbmeta_system"));
        assert!(rollback(3).starts_with("Rollback Index Location: 3 - boot"));
        assert!(rollback(7).starts_with("Rollback Index Location: 7 - 미사용"));
        assert!(rollback(31).starts_with("Rollback Index Location: 31 - 미사용"));
        // A field only TB522FU has.
        assert!(
            lapiswitch_core::deviceinfo::map::Layout::find("TB520FU")
                .unwrap()
                .field("deviceinfo_format_version")
                .is_err()
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
    /// The name of a country code (`country_<CODE>`), else the code.
    pub fn country_name(self, code: &str) -> &'static str {
        let key = format!("country_{code}");
        self.catalog()
            .get(key.as_str())
            .or_else(|| Self::English.catalog().get(key.as_str()))
            .copied()
            .unwrap_or_else(|| crate::countries::find(code).map_or("", |c| c.code))
    }
    /// A DeviceInfo name or help from `devices/<MODEL>/text/<lang>.toml` in
    /// this language, or the English one when it is not written.
    pub fn field_text(self, texts: [&'static str; 5]) -> &'static str {
        lapiswitch_core::deviceinfo::map::TEXT_LANGUAGES
            .iter()
            .position(|code| *code == self.code())
            .map(|index| texts[index])
            .filter(|text| !text.is_empty())
            .unwrap_or(texts[0])
    }
    /// The run messages of the core, worker and CLI in this language.
    fn run_catalog(self) -> &'static Catalog {
        static CATALOGS: [OnceLock<Catalog>; 5] = [const { OnceLock::new() }; 5];
        let index = Self::ALL.iter().position(|l| *l == self).expect("listed");
        CATALOGS[index].get_or_init(|| {
            leak_catalog(lapiswitch_core::i18n::json(self.code()).expect("bundled run messages"))
        })
    }
    fn code(self) -> &'static str {
        match self {
            Self::Korean => "ko",
            Self::English => "en",
            Self::Chinese => "zh",
            Self::Japanese => "ja",
            Self::Russian => "ru",
        }
    }
    /// Backend crates log through `ltbox_core::i18n::tr`; give them the
    /// screen and run catalogs of this language. A key missing from both
    /// returns itself, and `lapiswitch_core::i18n` then falls back to English.
    pub fn install(self) {
        ltbox_core::i18n::set_translator(move |key| {
            self.catalog()
                .get(key)
                .or_else(|| self.run_catalog().get(key))
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
    /// DeviceInfo files parsed with a determined model, latest first.
    pub recent_deviceinfo: Vec<PathBuf>,
}

/// How many recent DeviceInfo files are kept.
pub const RECENT_MAX: usize = 5;
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::Korean,
            theme: ThemeChoice::System,
            mask_serial: true,
            kernel: ltbox_device::driver::kernel_mode_supported(),
            storage_root: None,
            dump_root: None,
            recent_deviceinfo: Vec::new(),
        }
    }
}
impl Preferences {
    pub fn storage_root(&self, data: &Path) -> PathBuf {
        self.storage_root
            .clone()
            .unwrap_or_else(|| data.to_path_buf())
    }
    /// Puts a parsed DeviceInfo file first among the recent ones, once.
    /// Returns whether the list changed.
    pub fn remember_deviceinfo(&mut self, path: &Path) -> bool {
        let before = self.recent_deviceinfo.clone();
        self.recent_deviceinfo.retain(|p| p != path);
        self.recent_deviceinfo.insert(0, path.to_path_buf());
        self.recent_deviceinfo.truncate(RECENT_MAX);
        self.recent_deviceinfo != before
    }
    /// Removes a file from the recent ones; returns whether it was there.
    pub fn forget_deviceinfo(&mut self, path: &Path) -> bool {
        let count = self.recent_deviceinfo.len();
        self.recent_deviceinfo.retain(|p| p != path);
        self.recent_deviceinfo.len() != count
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
