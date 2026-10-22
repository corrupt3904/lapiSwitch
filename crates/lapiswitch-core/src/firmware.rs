//! S0: prepare and validate the firmware folder the user picked.
//! Nothing here touches the device.
//!
//! XML selection is LTBox's own `collect_firmware_xmls_for_flash`; folder
//! redirect, loader lookup and `.x` decryption are ported from LTBox
//! `ltbox-gui` (`loader.rs`, `workers/flash/mod.rs`).

use std::path::{Path, PathBuf};

use crate::avb_info::{self, KeyClass};
use crate::device::Device;
use crate::region::Region;
use crate::rollback::RollbackIndices;
use lapiswitch_devices::RegionMarker;

#[derive(Debug, thiserror::Error)]
pub enum FirmwareError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("no rawprogram*.xml / rawprogram*.x in {0} or its image/ folder")]
    NotFirmwareFolder(String),
    #[error("EDL loader ({0}) not found in {1} or its parent")]
    LoaderMissing(&'static str, String),
    #[error("required image missing: {0}")]
    ImageMissing(String),
    #[error("compressed images (.zst) are not supported: {0}")]
    CompressedImage(String),
    #[error("cannot decrypt {path}: {message}")]
    Decrypt { path: String, message: String },
    #[error(transparent)]
    Avb(#[from] avb_info::AvbReadError),
    #[error("vbmeta_system.img carries no build fingerprint")]
    NoFingerprint,
    #[error("firmware is for another model: {0}")]
    WrongModel(String),
    #[error("vbmeta_system.img is signed with an unknown key ({0})")]
    UnknownKey(String),
    #[error("vendor_boot.img region markers: {0:?}")]
    RegionUndetermined(VendorBootRegion),
    #[error("vendor_boot.img says {marker:?} but the fingerprint says {fingerprint:?}")]
    RegionMismatch { marker: Region, fingerprint: Region },
    #[error("선택한 리전은 {}이지만 펌웨어의 리전은 {}입니다. 선택한 리전과 같은 펌웨어를 선택하세요.", target.token(), firmware.token())]
    WrongRegion { firmware: Region, target: Region },
    #[error("XML selection failed: {0}")]
    XmlSelection(String),
    #[error("excluded XML variant was selected: {0}")]
    ExcludedXmlSelected(String),
    #[error("{xml}: {message}")]
    Xml { xml: String, message: String },
    #[error("split super image missing: {0}")]
    SuperImageMissing(String),
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> FirmwareError + '_ {
    move |source| FirmwareError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// An image a selected rawprogram names but the package does not ship. LTBox
/// skips it with a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingImage {
    pub xml: String,
    pub label: String,
    pub filename: String,
}

/// Everything S0 established about a valid firmware folder.
#[derive(Debug, Clone)]
pub struct FirmwareReport {
    /// The supported device the package's vbmeta_system fingerprint names.
    pub device: Device,
    pub image_dir: PathBuf,
    pub loader: PathBuf,
    pub region: Region,
    pub fingerprint: String,
    pub key_class: KeyClass,
    pub indices: RollbackIndices,
    pub raw_xmls: Vec<PathBuf>,
    pub patch_xmls: Vec<PathBuf>,
    /// `.x` files decrypted in place this run (LTBox behaviour, §12 v1.1).
    pub decrypted: usize,
    pub missing_images: Vec<MissingImage>,
    /// The package's LUN 4 PrimaryGPT entry names a file, so the S6 flash
    /// rewrites the GPT itself (§4.2).
    pub lun4_gpt_in_package: bool,
}

/// Validate `folder` as firmware for `target`. Decrypts `.x` rawprograms in
/// place first, exactly as LTBox does; nothing else is written.
pub fn check_firmware(folder: &Path, target: Region) -> Result<FirmwareReport, FirmwareError> {
    let dir = resolve_image_dir(folder)?;
    if let Some(zst) = find_zst(&dir, 0)? {
        return Err(FirmwareError::CompressedImage(zst.display().to_string()));
    }
    if !dir.join("vbmeta_system.img").is_file() {
        return Err(FirmwareError::ImageMissing("vbmeta_system.img".into()));
    }
    // The package names its device; that device's rules apply from here.
    let vbs = avb_info::read_avb(&dir.join("vbmeta_system.img"))?;
    let fingerprint = vbs
        .build_fingerprint()
        .ok_or(FirmwareError::NoFingerprint)?;
    let device = Device::identify(&fingerprint)
        .ok_or_else(|| FirmwareError::WrongModel(fingerprint.clone()))?;
    let loader = find_loader(&dir, device.loader())
        .ok_or_else(|| FirmwareError::LoaderMissing(device.loader(), dir.display().to_string()))?;
    for name in device.required_images() {
        if !dir.join(name).is_file() {
            return Err(FirmwareError::ImageMissing(name.to_string()));
        }
    }
    let key_class = vbs.key_class();
    if key_class == KeyClass::Unknown {
        return Err(FirmwareError::UnknownKey(
            vbs.public_key_sha1.clone().unwrap_or_default(),
        ));
    }
    let boot = avb_info::read_avb(&dir.join("boot.img"))?;
    let indices = RollbackIndices {
        boot: boot.rollback_index,
        vbmeta_system: vbs.rollback_index,
    };

    let vendor_boot = std::fs::read(dir.join("vendor_boot.img")).map_err(io_error(&dir))?;
    let firmware_region = match device.region_marker() {
        RegionMarker::RegionCountry => match detect_vendor_boot_region(&vendor_boot) {
            VendorBootRegion::Prc => Region::Prc,
            VendorBootRegion::Row => Region::Row,
            other => return Err(FirmwareError::RegionUndetermined(other)),
        },
        marker => crate::vendor_boot::region(marker, &vendor_boot)
            .map_err(|_| FirmwareError::RegionUndetermined(VendorBootRegion::Unknown))?,
    };
    if let Some(fingerprint_region) =
        crate::device_version::fingerprint_region(device, &fingerprint)
        && fingerprint_region != firmware_region
    {
        return Err(FirmwareError::RegionMismatch {
            marker: firmware_region,
            fingerprint: fingerprint_region,
        });
    }
    if firmware_region != target {
        return Err(FirmwareError::WrongRegion {
            firmware: firmware_region,
            target,
        });
    }

    let decrypted = decrypt_x_files(&dir)?;
    let (raw_xmls, patch_xmls) = ltbox_device::edl::collect_firmware_xmls_for_flash(&dir, false)
        .map_err(|error| FirmwareError::XmlSelection(error.to_string()))?;
    if raw_xmls.is_empty() {
        return Err(FirmwareError::NotFirmwareFolder(dir.display().to_string()));
    }
    if let Some(excluded) = raw_xmls.iter().find(|path| is_excluded_variant(path)) {
        return Err(FirmwareError::ExcludedXmlSelected(
            excluded.display().to_string(),
        ));
    }
    let scan = scan_rawprograms(&raw_xmls)?;
    if let Some(super_image) = scan.missing.iter().find(|m| is_split_super(&m.filename)) {
        return Err(FirmwareError::SuperImageMissing(
            super_image.filename.clone(),
        ));
    }

    Ok(FirmwareReport {
        device,
        image_dir: dir,
        loader,
        region: firmware_region,
        fingerprint,
        key_class,
        indices,
        raw_xmls,
        patch_xmls,
        decrypted,
        missing_images: scan.missing,
        lun4_gpt_in_package: scan.lun4_gpt_in_package,
    })
}

/// LTBox `redirect_to_image_subdir`: use `folder/image` when `folder` itself
/// has no rawprogram pack but that child does.
pub fn resolve_image_dir(folder: &Path) -> Result<PathBuf, FirmwareError> {
    if has_rawprogram_pack(folder)? {
        return Ok(folder.to_path_buf());
    }
    for entry in std::fs::read_dir(folder)
        .map_err(io_error(folder))?
        .flatten()
    {
        let path = entry.path();
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("image")
            && path.is_dir()
            && has_rawprogram_pack(&path)?
        {
            return Ok(path);
        }
    }
    Err(FirmwareError::NotFirmwareFolder(
        folder.display().to_string(),
    ))
}

fn has_rawprogram_pack(dir: &Path) -> Result<bool, FirmwareError> {
    Ok(std::fs::read_dir(dir)
        .map_err(io_error(dir))?
        .flatten()
        .any(|entry| {
            let path = entry.path();
            let stem_ok = path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.to_ascii_lowercase().starts_with("rawprogram"));
            let ext_ok = path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("x") || x.eq_ignore_ascii_case("xml"));
            stem_ok && ext_ok
        }))
}

/// LTBox `find_firmware_loader`: the Sahara manifest wins over the single
/// `.melf`; look in the image folder, then its parent.
pub fn find_loader(dir: &Path, name: &str) -> Option<PathBuf> {
    find_loader_in(dir, name).or_else(|| dir.parent().and_then(|p| find_loader_in(p, name)))
}

fn find_loader_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    let manifest = entries
        .iter()
        .find(|p| ltbox_core::sahara_xml::is_manifest_filename(p))
        .or_else(|| {
            entries
                .iter()
                .find(|p| ltbox_core::sahara_xml::is_encrypted_manifest_filename(p))
        });
    if let Some(manifest) = manifest {
        return Some(manifest.clone());
    }
    entries.into_iter().find(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
    })
}

fn find_zst(dir: &Path, depth: u32) -> Result<Option<PathBuf>, FirmwareError> {
    if depth > 8 {
        return Ok(None);
    }
    for path in std::fs::read_dir(dir)
        .map_err(io_error(dir))?
        .flatten()
        .map(|e| e.path())
    {
        if path.is_dir() {
            if let Some(found) = find_zst(&path, depth + 1)? {
                return Ok(Some(found));
            }
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case("zst"))
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// LTBox `decrypt_rawprogram_x_files`: every `.x` except the encrypted Sahara
/// manifest becomes `<stem>.xml` next to it, overwriting an existing one.
pub fn decrypt_x_files(dir: &Path) -> Result<usize, FirmwareError> {
    let mut sources: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(io_error(dir))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|x| x.eq_ignore_ascii_case("x"))
                && !ltbox_core::sahara_xml::is_encrypted_manifest_filename(p)
        })
        .collect();
    sources.sort();
    for source in &sources {
        let output = dir
            .join(source.file_stem().unwrap_or_default())
            .with_extension("xml");
        ltbox_core::crypto::decrypt_file(source, &output).map_err(|error| {
            FirmwareError::Decrypt {
                path: source.display().to_string(),
                message: error.to_string(),
            }
        })?;
    }
    Ok(sources.len())
}

/// Region markers in `vendor_boot.img` (LTBox `RegionPatternSet` patterns).
/// TB520FU firmware carries four `IPRC` or four `IROW` (§4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorBootRegion {
    Prc,
    Row,
    Mixed,
    Unknown,
}

pub fn detect_vendor_boot_region(data: &[u8]) -> VendorBootRegion {
    let count = |needle: &[u8]| data.windows(needle.len()).filter(|w| *w == needle).count();
    let prc = count(b".PRC") + count(b"IPRC");
    let row = count(b".ROW") + count(b"IROW");
    match (prc > 0, row > 0) {
        (true, false) => VendorBootRegion::Prc,
        (false, true) => VendorBootRegion::Row,
        (true, true) => VendorBootRegion::Mixed,
        (false, false) => VendorBootRegion::Unknown,
    }
}

/// LTBox drops `*WIPE_PARTITIONS*` / `*BLANK_GPT*` case-sensitively; re-check
/// its selection ignoring case.
pub fn is_excluded_variant(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.contains("wipe_partitions") || name.contains("blank_gpt")
}

fn is_split_super(filename: &str) -> bool {
    Path::new(filename).file_name().is_some_and(|name| {
        let name = name.to_string_lossy().to_ascii_lowercase();
        name.starts_with("super_") && name.ends_with(".img")
    })
}

#[derive(Debug, Default)]
struct RawprogramScan {
    missing: Vec<MissingImage>,
    lun4_gpt_in_package: bool,
}

/// Images the selected rawprograms reference but the folder lacks, and
/// whether the LUN 4 PrimaryGPT entry carries a file.
fn scan_rawprograms(xmls: &[PathBuf]) -> Result<RawprogramScan, FirmwareError> {
    let mut scan = RawprogramScan::default();
    for xml_path in xmls {
        let xml_name = xml_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let xml_error = |message: String| FirmwareError::Xml {
            xml: xml_name.clone(),
            message,
        };
        let text = ltbox_core::xml::read(xml_path).map_err(|e| xml_error(e.to_string()))?;
        let doc = ltbox_core::xml::parse(&text).map_err(|e| xml_error(e.to_string()))?;
        let dir = xml_path.parent().unwrap_or(Path::new("."));
        for node in doc
            .descendants()
            .filter(|n| n.tag_name().name().eq_ignore_ascii_case("program"))
        {
            let label = node.attribute("label").unwrap_or("").trim();
            let filename = node.attribute("filename").unwrap_or("").trim();
            let sectors: u64 = node
                .attribute("num_partition_sectors")
                .unwrap_or("0")
                .trim()
                .parse()
                .unwrap_or(0);
            if filename.is_empty() || sectors == 0 {
                continue;
            }
            if label.eq_ignore_ascii_case("PrimaryGPT")
                && node.attribute("physical_partition_number").map(str::trim) == Some("4")
            {
                scan.lun4_gpt_in_package = true;
            }
            if !dir.join(filename).exists() {
                scan.missing.push(MissingImage {
                    xml: xml_name.clone(),
                    label: label.to_string(),
                    filename: filename.to_string(),
                });
            }
        }
    }
    Ok(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn vendor_boot_markers_decide_the_region() {
        // As in the real TB520FU DTB: a 4-byte FDT property value.
        let prc = [&[0, 0, 0, 3, 0, 0, 0, 4][..], b"IPRC\0\0\0\0"].concat();
        assert_eq!(detect_vendor_boot_region(&prc), VendorBootRegion::Prc);
        assert_eq!(
            detect_vendor_boot_region(b"..IROW.."),
            VendorBootRegion::Row
        );
        assert_eq!(detect_vendor_boot_region(b"x.PRCx"), VendorBootRegion::Prc);
        assert_eq!(
            detect_vendor_boot_region(b"IPRC IROW"),
            VendorBootRegion::Mixed
        );
        assert_eq!(
            detect_vendor_boot_region(b"PRC ROW"),
            VendorBootRegion::Unknown
        );
    }

    #[test]
    fn excluded_variants_are_caught_in_any_case() {
        for name in [
            "rawprogram4_BLANK_GPT.xml",
            "rawprogram4_blank_gpt.xml",
            "rawprogram0_Wipe_Partitions.xml",
        ] {
            assert!(is_excluded_variant(Path::new(name)), "{name}");
        }
        assert!(!is_excluded_variant(Path::new("rawprogram4.xml")));
    }

    #[test]
    fn redirects_a_package_root_to_its_image_folder() {
        let root = temp_dir();
        let image = root.path().join("image");
        std::fs::create_dir(&image).unwrap();
        std::fs::write(image.join("rawprogram4.x"), b"").unwrap();
        assert_eq!(resolve_image_dir(root.path()).unwrap(), image);
        assert_eq!(resolve_image_dir(&image).unwrap(), image);
        let empty = temp_dir();
        assert!(matches!(
            resolve_image_dir(empty.path()),
            Err(FirmwareError::NotFirmwareFolder(_))
        ));
    }

    #[test]
    fn loader_is_found_beside_or_above_the_image_folder() {
        let root = temp_dir();
        let image = root.path().join("image");
        std::fs::create_dir(&image).unwrap();
        let name = crate::device::tb520fu().loader();
        assert_eq!(find_loader(&image, name), None);
        std::fs::write(root.path().join("XBL_S_DEVPRG_NS.melf"), b"").unwrap();
        assert!(find_loader(&image, name).is_some());
        std::fs::write(image.join(name), b"").unwrap();
        assert_eq!(find_loader(&image, name), Some(image.join(name)));
    }

    #[test]
    fn scan_reports_missing_images_and_the_lun4_gpt_entry() {
        let dir = temp_dir();
        std::fs::write(dir.path().join("gpt_main4.bin"), b"").unwrap();
        std::fs::write(dir.path().join("tz.mbn"), b"").unwrap();
        let xml = dir.path().join("rawprogram4.xml");
        std::fs::write(
            &xml,
            r#"<data>
              <program label="PrimaryGPT" filename="gpt_main4.bin" num_partition_sectors="6" physical_partition_number="4" start_sector="0"/>
              <program label="tz_a" filename="tz.mbn" num_partition_sectors="1024" physical_partition_number="4" start_sector="1542"/>
              <program label="devinfo" filename="" num_partition_sectors="1" physical_partition_number="4" start_sector="9"/>
              <program label="vm-persist" filename="vm-persist.img" num_partition_sectors="8" physical_partition_number="4" start_sector="20"/>
            </data>"#,
        )
        .unwrap();
        let scan = scan_rawprograms(&[xml]).unwrap();
        assert!(scan.lun4_gpt_in_package);
        assert_eq!(
            scan.missing,
            vec![MissingImage {
                xml: "rawprogram4.xml".into(),
                label: "vm-persist".into(),
                filename: "vm-persist.img".into(),
            }]
        );
    }

    #[test]
    fn placeholder_gpt_entry_is_not_counted() {
        let dir = temp_dir();
        let xml = dir.path().join("rawprogram4.xml");
        std::fs::write(
            &xml,
            r#"<data><program label="PrimaryGPT" filename="" num_partition_sectors="6" physical_partition_number="4" start_sector="0"/></data>"#,
        )
        .unwrap();
        assert!(!scan_rawprograms(&[xml]).unwrap().lun4_gpt_in_package);
    }

    #[test]
    fn split_super_names() {
        assert!(is_split_super("super_3.img"));
        assert!(is_split_super("SUPER_1.IMG"));
        assert!(!is_split_super("super.img"));
    }
}
