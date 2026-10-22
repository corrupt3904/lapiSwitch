//! A pinned upstream Release (`[twrp]` in device.toml). Cache contents are
//! verified on every use.
use std::path::{Path, PathBuf};

fn twrp() -> &'static lapiswitch_devices::Twrp {
    &crate::device::flow().twrp
}
pub fn size() -> u64 {
    twrp().size
}
pub fn verify(path: &Path) -> Result<(), String> {
    if crate::backup::hash_file(path, None)? != (size(), twrp().sha256.to_owned()) {
        return Err("TWRP Release 크기 또는 SHA-256이 일치하지 않습니다.".into());
    }
    Ok(())
}
pub fn acquire(cache: &Path, log: &mut Vec<String>) -> Result<PathBuf, String> {
    let twrp = twrp();
    let path = cache.join(twrp.tag).join(twrp.name);
    if path.exists() {
        verify(&path)?;
        return Ok(path);
    }
    ltbox_core::live!(log, "TWRP {}: 공식 Release를 다운로드합니다.", twrp.tag);
    ltbox_core::downloader::download_to_file(twrp.url, &path, log).map_err(|e| e.to_string())?;
    verify(&path)?;
    Ok(path)
}
