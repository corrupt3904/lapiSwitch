//! Text of the run messages: logs, statuses and errors of the core, the GUI
//! worker and the CLI. Every language lives in `lang/<code>.json`, keyed by
//! identifiers such as `core_dump_saved`. The GUI installs the chosen
//! language through `ltbox_core::i18n`; without it, or for a key it lacks,
//! the English text is used, then the key itself.
use std::collections::HashMap;
use std::sync::LazyLock;

/// The bundled catalog of a language code (`ko`, `en`, `zh`, `ja`, `ru`).
pub fn json(code: &str) -> Option<&'static str> {
    Some(match code {
        "ko" => include_str!("../lang/ko.json"),
        "en" => include_str!("../lang/en.json"),
        "zh" => include_str!("../lang/zh.json"),
        "ja" => include_str!("../lang/ja.json"),
        "ru" => include_str!("../lang/ru.json"),
        _ => return None,
    })
}

static ENGLISH: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../lang/en.json")).expect("bundled English messages")
});

/// The text of `key` in the installed language, else English, else the key.
pub fn text(key: &str) -> String {
    let translated = ltbox_core::i18n::tr(key);
    if translated != key {
        return translated;
    }
    ENGLISH.get(key).cloned().unwrap_or(translated)
}

/// `text(key)` with its `{name}` placeholders filled in.
pub fn format(key: &str, values: &[(&str, String)]) -> String {
    ltbox_core::i18n::format_template(&text(key), values)
}

/// The placeholder values of `line` if it is the message `key` in the
/// installed language: the inverse of `format`, for lines read back from a
/// run log. Each value is taken up to the literal text that follows it.
pub fn parse(key: &str, line: &str) -> Option<Vec<(String, String)>> {
    let template = text(key);
    let mut rest = template.as_str();
    let mut line = line;
    let mut values = Vec::new();
    loop {
        let Some(start) = rest.find('{') else {
            return (line == rest).then_some(values);
        };
        line = line.strip_prefix(&rest[..start])?;
        let end = rest[start..].find('}')? + start;
        let name = &rest[start + 1..end];
        rest = &rest[end + 1..];
        let next = rest.find('{').unwrap_or(rest.len());
        let value_end = if rest[..next].is_empty() {
            line.len()
        } else {
            line.find(&rest[..next])?
        };
        values.push((name.to_string(), line[..value_end].to_string()));
        line = &line[value_end..];
    }
}

/// A run message by key, with optional `name = value` placeholders:
/// `msg!("core_dump_saved", folder = folder.display())`.
#[macro_export]
macro_rules! msg {
    ($key:literal) => {
        $crate::i18n::text($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::format($key, &[$((stringify!($name), format!("{}", $value))),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    const CODES: [&str; 5] = ["en", "ko", "zh", "ja", "ru"];

    fn catalog(code: &str) -> HashMap<String, String> {
        serde_json::from_str(json(code).unwrap()).unwrap()
    }
    fn placeholders(text: &str) -> BTreeSet<&str> {
        text.split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name))
            .collect()
    }
    fn sources(dir: &Path, out: &mut String) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                // This file names keys only in examples and tests.
                && !path.ends_with("i18n.rs")
            {
                out.push_str(&std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    /// Keys named as `msg!("key"` or `i18n::parse("key"` in a source text.
    fn used_keys(source: &str) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for marker in ["msg!(", "i18n::parse("] {
            for part in source.split(marker).skip(1) {
                let part = part.trim_start();
                if let Some(rest) = part.strip_prefix('"')
                    && let Some((key, _)) = rest.split_once('"')
                {
                    keys.insert(key.to_string());
                }
            }
        }
        keys
    }

    #[test]
    fn every_language_has_the_same_keys_and_placeholders() {
        let english = catalog("en");
        for code in CODES {
            let other = catalog(code);
            let mut a: Vec<_> = english.keys().collect();
            let mut b: Vec<_> = other.keys().collect();
            a.sort();
            b.sort();
            assert_eq!(a, b, "{code}");
            for (key, text) in &english {
                assert_eq!(
                    placeholders(text),
                    placeholders(&other[key]),
                    "{code} {key}"
                );
            }
        }
    }
    #[test]
    fn every_used_key_exists_and_every_key_is_used() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut source = String::new();
        for dir in ["src", "../lapiswitch-gui/src", "../lapiswitch-cli/src"] {
            sources(&root.join(dir), &mut source);
        }
        let used = used_keys(&source);
        let english: BTreeSet<String> = catalog("en").into_keys().collect();
        let missing: Vec<_> = used.difference(&english).collect();
        let unused: Vec<_> = english.difference(&used).collect();
        assert!(missing.is_empty(), "missing {missing:?}");
        assert!(unused.is_empty(), "unused {unused:?}");
    }
    #[test]
    fn untranslated_text_is_english_and_a_line_parses_back() {
        let line = format(
            "core_rollback_compare",
            &[
                ("partition", "boot".into()),
                ("image", "5".into()),
                ("floor", "7".into()),
            ],
        );
        assert_eq!(line, "Rollback comparison boot: firmware 5, device 7");
        let values = parse("core_rollback_compare", &line).unwrap();
        assert_eq!(
            values,
            [("partition", "boot"), ("image", "5"), ("floor", "7")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
        );
        assert_eq!(parse("core_rollback_compare", "Rollback"), None);
        assert_eq!(text("Missing-key"), "Missing-key");
    }
}
