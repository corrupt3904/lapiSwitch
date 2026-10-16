//! Offline license inventory embedded in both the ZIP and standalone executable.
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Catalog {
    pub components: Vec<Component>,
    pub texts: Vec<String>,
}

#[derive(Deserialize)]
pub struct Component {
    pub name: String,
    pub version: String,
    pub license: String,
    pub source: String,
    pub notices: Vec<Notice>,
}

#[derive(Deserialize)]
pub struct Notice {
    pub title: String,
    pub text: usize,
}

impl Component {
    pub fn matches(&self, query: &str) -> bool {
        let searchable = format!("{} {} {}", self.name, self.version, self.license).to_lowercase();
        query
            .split_whitespace()
            .all(|word| searchable.contains(&word.to_lowercase()))
    }
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/licenses.json"))
            .expect("bundled license inventory")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_component_has_complete_readable_notices() {
        let catalog = catalog();
        assert!(catalog.components.len() >= 384);
        let mut referenced = std::collections::BTreeSet::new();
        for component in &catalog.components {
            assert!(!component.name.trim().is_empty());
            assert!(!component.license.trim().is_empty(), "{}", component.name);
            assert!(!component.notices.is_empty(), "{}", component.name);
            assert!(
                component
                    .notices
                    .iter()
                    .any(|n| catalog.texts[n.text].trim().len() > 20)
            );
            for notice in &component.notices {
                assert!(!notice.title.trim().is_empty());
                assert!(!catalog.texts[notice.text].trim().is_empty());
                referenced.insert(notice.text);
            }
        }
        assert_eq!(referenced.len(), catalog.texts.len());
        for (name, original) in [
            ("lapiSwitch", include_str!("../../../LICENSE")),
            ("flag-icons", include_str!("../assets/flags/LICENSE")),
            ("Noto Sans KR", include_str!("../fonts/LICENSE-OFL.txt")),
            ("qdl", include_str!("../../../vendor/qdl/LICENSE")),
        ] {
            let component = catalog.components.iter().find(|c| c.name == name).unwrap();
            assert!(
                component
                    .notices
                    .iter()
                    .any(|n| catalog.texts[n.text] == original)
            );
        }
    }
}
