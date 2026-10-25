//! A saved DeviceInfo file read back for viewing: which device's field map
//! fits it. Nothing is written anywhere.
use super::SHARED_SIZE;
use super::map::{Kind, Layout};
use std::path::{Path, PathBuf};

/// How the field map of a file was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The `info.json` saved next to it by a DeviceInfo dump, whose SHA-256
    /// matches the file.
    Record,
    /// The only field map whose region and region markers fit the bytes.
    Detected,
    /// Picked by the user.
    Chosen,
}

#[derive(Debug, Clone)]
pub struct Opened {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub sha256: String,
    /// `None` when no single device could be determined.
    pub layout: Option<(Layout, Source)>,
}

/// Every device that has a field map.
pub fn layouts() -> Vec<Layout> {
    lapiswitch_devices::DEVICES
        .iter()
        .filter_map(Layout::of)
        .collect()
}

/// Whether `layout` fits `bytes`: a region at its region field and readable
/// text in its read-only text fields (the magic and the region markers).
pub fn fits(layout: Layout, bytes: &[u8]) -> bool {
    layout.region(bytes).is_some()
        && layout
            .fields()
            .iter()
            .filter(|field| !field.editable && matches!(field.kind, Kind::Text { .. }))
            .all(|field| !super::map::display(field, bytes).starts_with("hex:"))
}

/// Reads a DeviceInfo file and picks its field map: the dump's record
/// first, then the only map that fits.
pub fn open(path: &Path) -> Result<Opened, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    from_bytes(path, bytes)
}

/// `open` for bytes already read from `path`.
pub fn from_bytes(path: &Path, bytes: Vec<u8>) -> Result<Opened, String> {
    if bytes.len() != SHARED_SIZE || !bytes.starts_with(b"ANDROID-BOOT!") {
        return Err(crate::msg!(
            "core_parse_not_deviceinfo",
            path = path.display()
        ));
    }
    let sha256 = crate::backup::digest_bytes(&bytes);
    let layout = recorded(path, &sha256)
        .map(|layout| (layout, Source::Record))
        .or_else(|| {
            let fitting: Vec<Layout> = layouts()
                .into_iter()
                .filter(|layout| fits(*layout, &bytes))
                .collect();
            match fitting.as_slice() {
                [only] => Some((*only, Source::Detected)),
                _ => None,
            }
        });
    Ok(Opened {
        path: path.to_owned(),
        bytes,
        sha256,
        layout,
    })
}

/// The model named by a matching `info.json` in the same folder.
fn recorded(path: &Path, sha256: &str) -> Option<Layout> {
    let raw = std::fs::read(path.parent()?.join("info.json")).ok()?;
    let info: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    if info.get("sha256")?.as_str()? != sha256 {
        return None;
    }
    Layout::find(info.get("model")?.as_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deviceinfo::map::tests::sample;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lapiswitch-parse-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn each_sample_is_detected_as_its_own_device() {
        for layout in layouts() {
            let dir = temp(layout.model());
            let path = dir.join("deviceinfo.bin");
            std::fs::write(&path, sample(layout)).unwrap();
            let opened = open(&path).unwrap();
            assert_eq!(opened.layout, Some((layout, Source::Detected)));
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_matching_dump_record_names_the_device_and_a_stale_one_is_ignored() {
        let layout = Layout::find("TB520FU").unwrap();
        let dir = temp("record");
        let path = dir.join("deviceinfo.bin");
        let bytes = sample(layout);
        std::fs::write(&path, &bytes).unwrap();
        let record = |sha: &str| {
            std::fs::write(
                dir.join("info.json"),
                serde_json::json!({ "model": "TB520FU", "sha256": sha }).to_string(),
            )
            .unwrap();
        };
        record(&crate::backup::digest_bytes(&bytes));
        assert_eq!(open(&path).unwrap().layout, Some((layout, Source::Record)));
        record("0");
        assert_eq!(
            open(&path).unwrap().layout,
            Some((layout, Source::Detected))
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn other_files_are_refused_and_unknown_shapes_need_a_choice() {
        let dir = temp("other");
        let path = dir.join("x.bin");
        std::fs::write(&path, [0u8; 16]).unwrap();
        assert!(open(&path).is_err());
        let mut bytes = vec![0u8; SHARED_SIZE];
        bytes[..13].copy_from_slice(b"ANDROID-BOOT!");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(open(&path).unwrap().layout, None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
