//! A device's pinned upstream TWRP Release (`[twrp]` in device.toml). Cache
//! contents are verified on every use.
use crate::device::Device;
use std::path::{Path, PathBuf};

pub fn size(device: Device) -> u64 {
    device.twrp().size
}
pub fn verify(device: Device, path: &Path) -> Result<(), String> {
    let twrp = device.twrp();
    if crate::backup::hash_file(path, None)? != (twrp.size, twrp.sha256.to_owned()) {
        return Err(format!(
            "{device} TWRP Release 크기 또는 SHA-256이 일치하지 않습니다."
        ));
    }
    Ok(())
}
pub fn acquire(device: Device, cache: &Path, log: &mut Vec<String>) -> Result<PathBuf, String> {
    let twrp = device.twrp();
    // Per-device folders: two devices never share or overwrite a release.
    let path = cache.join(device.model()).join(twrp.tag).join(twrp.name);
    if path.exists() {
        verify(device, &path)?;
        return Ok(path);
    }
    ltbox_core::live!(
        log,
        "{device} TWRP {}: 공식 Release를 다운로드합니다.",
        twrp.tag
    );
    ltbox_core::downloader::download_to_file(twrp.url, &path, log).map_err(|e| e.to_string())?;
    verify(device, &path)?;
    Ok(path)
}
