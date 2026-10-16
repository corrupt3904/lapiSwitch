//! A pinned upstream Release. Cache contents are verified on every use.
use std::path::{Path, PathBuf};
pub const TAG: &str = "v3.7.1_16-20261014";
pub const NAME: &str = "TWRP-3.7.1_16-lapis-20261014-landscape.img";
pub const SIZE: u64 = 104857600;
pub const SHA256: &str = "310ff2ecfc4c52060f9b6a0ad6f6e466a63a36d8ae87bc39f0c5408d2dcf24a4";
pub const URL: &str = "https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp/releases/download/v3.7.1_16-20261014/TWRP-3.7.1_16-lapis-20261014-landscape.img";
pub fn verify(path: &Path) -> Result<(), String> {
    if crate::backup::hash_file(path, None)? != (SIZE, SHA256.to_owned()) {
        return Err("TWRP Release 크기 또는 SHA-256이 일치하지 않습니다.".into());
    }
    Ok(())
}
pub fn acquire(cache: &Path, log: &mut Vec<String>) -> Result<PathBuf, String> {
    let path = cache.join(TAG).join(NAME);
    if path.exists() {
        verify(&path)?;
        return Ok(path);
    }
    ltbox_core::live!(log, "TWRP {TAG}: 공식 Release를 다운로드합니다.");
    ltbox_core::downloader::download_to_file(URL, &path, log).map_err(|e| e.to_string())?;
    verify(&path)?;
    Ok(path)
}
