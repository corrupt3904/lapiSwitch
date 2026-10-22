use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Korean and English are written beside each other in the code. Chinese,
/// Japanese and Russian look up the English text in `lang/ui/<code>.json`
/// and fall back to English for anything missing.
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

    const LITERAL: &str = r#""((?:[^"\\]|\\.)*)""#;

    fn unescape(s: &str) -> String {
        s.replace("\\\"", "\"")
            .replace("\\n", "\n")
            .replace("\\\\", "\\")
    }

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

    /// English text of every literal pair passed to `t`, `text` and the
    /// view helper `title(app, ko, en, sub_ko, sub_en)` in the GUI sources.
    fn english_sources() -> BTreeSet<String> {
        let pair = regex::Regex::new(&format!(
            r"(?s)\.(?:t|text)\(\s*{LITERAL}\s*,\s*{LITERAL}\s*,?\s*\)"
        ))
        .unwrap();
        let title = regex::Regex::new(&format!(
            r"(?s)title\(\s*app\s*,\s*{LITERAL}\s*,\s*{LITERAL}\s*,\s*{LITERAL}\s*,\s*{LITERAL}"
        ))
        .unwrap();
        let mut files = vec![];
        rust_sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut english = BTreeSet::new();
        for file in &files {
            for c in pair.captures_iter(file) {
                english.insert(unescape(&c[2]));
            }
            for c in title.captures_iter(file) {
                english.insert(unescape(&c[2]));
                english.insert(unescape(&c[4]));
            }
        }
        english
    }

    fn placeholders(s: &str) -> Vec<String> {
        let rx = regex::Regex::new(r"\{[A-Za-z_]*\}").unwrap();
        let mut found: Vec<String> = rx.find_iter(s).map(|m| m.as_str().to_owned()).collect();
        found.sort();
        found
    }

    #[test]
    fn every_english_ui_text_is_translated_in_each_added_language() {
        let english = english_sources();
        assert!(english.len() > 200, "source scan found too little text");
        for language in [Language::Chinese, Language::Japanese, Language::Russian] {
            let catalog = language.ui_catalog().unwrap();
            let keys: BTreeSet<String> = catalog.keys().map(|k| (*k).to_owned()).collect();
            let missing: Vec<_> = english.difference(&keys).collect();
            let stale: Vec<_> = keys.difference(&english).collect();
            assert!(missing.is_empty(), "{language:?} missing: {missing:#?}");
            assert!(stale.is_empty(), "{language:?} unused: {stale:#?}");
            for (en, translated) in catalog {
                assert!(!translated.trim().is_empty(), "{language:?}: {en}");
                assert_eq!(
                    placeholders(en),
                    placeholders(translated),
                    "{language:?}: {en}"
                );
                assert_eq!(language.text("한국어", en), *translated);
            }
            let absent = "Not in any catalog";
            assert_eq!(language.text("한국어", absent), absent);
        }
    }

    #[test]
    fn log_translations_cover_the_same_keys_and_placeholders() {
        let english: HashMap<String, String> =
            serde_json::from_str(include_str!("../lang/en.json")).unwrap();
        for json in [
            include_str!("../lang/ko.json"),
            include_str!("../lang/zh.json"),
            include_str!("../lang/ja.json"),
            include_str!("../lang/ru.json"),
        ] {
            let other: HashMap<String, String> = serde_json::from_str(json).unwrap();
            assert_eq!(
                other.keys().collect::<BTreeSet<_>>(),
                english.keys().collect::<BTreeSet<_>>()
            );
            for (key, value) in &english {
                assert_eq!(placeholders(value), placeholders(&other[key]), "{key}");
            }
        }
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
    fn ui_catalog(self) -> Option<&'static Catalog> {
        static ZH: OnceLock<Catalog> = OnceLock::new();
        static JA: OnceLock<Catalog> = OnceLock::new();
        static RU: OnceLock<Catalog> = OnceLock::new();
        let (cell, json) = match self {
            Self::Korean | Self::English => return None,
            Self::Chinese => (&ZH, include_str!("../lang/ui/zh.json")),
            Self::Japanese => (&JA, include_str!("../lang/ui/ja.json")),
            Self::Russian => (&RU, include_str!("../lang/ui/ru.json")),
        };
        Some(cell.get_or_init(|| leak_catalog(json)))
    }
    pub fn text(self, ko: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Korean => ko,
            Self::English => en,
            _ => self
                .ui_catalog()
                .and_then(|catalog| catalog.get(en).copied())
                .unwrap_or(en),
        }
    }
    pub fn install(self) {
        let json = match self {
            Self::Korean => include_str!("../lang/ko.json"),
            Self::English => include_str!("../lang/en.json"),
            Self::Chinese => include_str!("../lang/zh.json"),
            Self::Japanese => include_str!("../lang/ja.json"),
            Self::Russian => include_str!("../lang/ru.json"),
        };
        let catalog: HashMap<String, String> =
            serde_json::from_str(json).expect("bundled translations");
        ltbox_core::i18n::set_translator(move |key| {
            catalog.get(key).cloned().unwrap_or_else(|| key.to_owned())
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
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: Language::Korean,
            theme: ThemeChoice::System,
            mask_serial: true,
            kernel: ltbox_device::driver::kernel_mode_supported(),
            storage_root: None,
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
