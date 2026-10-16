//! Rust-only development commands; no device access.
mod gpt_reference;
mod icons;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn value(args: &mut impl Iterator<Item = OsString>, option: &str) -> Result<PathBuf> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("Missing {option} value").into())
}
fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref().and_then(|s| s.to_str()) {
        Some("build-helper") => {
            let mut ndk = std::env::var_os("ANDROID_NDK_HOME").map(PathBuf::from);
            while let Some(arg) = args.next() {
                if arg == "--ndk" { ndk = Some(value(&mut args, "--ndk")?); }
                else { return Err(format!("Unknown option: {}", arg.to_string_lossy()).into()); }
            }
            build_helper(&ndk.ok_or("Supply --ndk PATH or ANDROID_NDK_HOME")?)?;
        }
        Some("icons") => {
            let mut source = root().join("crates/lapiswitch-gui/assets/branding/lapiswitch-source.png");
            let mut output = source.parent().unwrap().to_owned();
            while let Some(arg) = args.next() {
                if arg == "--source" { source = value(&mut args, "--source")?; }
                else if arg == "--output" { output = value(&mut args, "--output")?; }
                else { return Err(format!("Unknown option: {}", arg.to_string_lossy()).into()); }
            }
            icons::generate(&source, &output)?;
            println!("Icons written to {}", output.display());
        }
        Some("gpt-reference") => {
            let input = value(&mut args, "input")?;
            let (mut slot, mut output) = (None, None);
            while let Some(arg) = args.next() {
                if arg == "--slot" { slot = Some(args.next().ok_or("Missing --slot value")?); }
                else if arg == "-o" || arg == "--output" { output = Some(value(&mut args, "--output")?); }
                else { return Err(format!("Unknown option: {}", arg.to_string_lossy()).into()); }
            }
            let slot = slot.as_deref().and_then(|s| s.to_str()).ok_or("Supply --slot a|b")?;
            let output = output.ok_or("Supply --output PATH")?;
            if input == output { return Err("The original dump must not be overwritten".into()); }
            let modified = gpt_reference::modify(&fs::read(&input)?, slot)?;
            use std::io::Write;
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&output)?;
            file.write_all(&modified)?; file.sync_all()?;
            println!("Written {} bytes: {}", modified.len(), output.display());
        }
        _ => return Err("Commands: build-helper --ndk PATH | icons [--source PNG] [--output DIR] | gpt-reference INPUT --slot a|b --output FILE".into()),
    }
    Ok(())
}
fn build_helper(ndk: &Path) -> Result<()> {
    let host = if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    };
    let clang = ndk
        .join("toolchains/llvm/prebuilt")
        .join(host)
        .join("bin")
        .join(if cfg!(windows) { "clang.exe" } else { "clang" });
    if !clang.is_file() {
        return Err(format!("NDK clang not found: {}", clang.display()).into());
    }
    let ndk_version = fs::read_to_string(ndk.join("source.properties"))?;
    if !ndk_version
        .lines()
        .any(|line| line.trim().starts_with("Pkg.Revision = 27.3."))
    {
        return Err("Use the validated Android NDK r27d (27.3)".into());
    }
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root()).args(["build", "--locked", "--release", "--target", "aarch64-linux-android", "-p", "lapiswitch-deviceinfo"])
        .env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", &clang)
        .env("CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS", "-C link-arg=--target=aarch64-linux-android30 -C link-arg=-Wl,-z,relro,-z,now -C link-arg=-Wl,-z,max-page-size=16384")
        .env("CARGO_PROFILE_RELEASE_PANIC", "abort").status()?;
    if !status.success() {
        return Err("Android helper build failed; install rustup target add aarch64-linux-android if needed".into());
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"));
    let bytes = fs::read(target.join("aarch64-linux-android/release/lapiswitch_deviceinfo"))?;
    validate_elf(&bytes)?;
    let sha = sha256(&bytes);
    let native_path = root().join("crates/lapiswitch-core/src/recovery_flow/native.rs");
    let native = fs::read_to_string(&native_path)?;
    let marker = "const HELPER_SHA: &str = \"";
    let (prefix, rest) = native
        .split_once(marker)
        .ok_or("Embedded helper hash declaration not found")?;
    let (old_sha, suffix) = rest
        .split_once('"')
        .ok_or("Invalid helper hash declaration")?;
    if old_sha.len() != 64 {
        return Err("Unexpected helper hash length".into());
    }
    let asset = root().join("assets/lapiswitch_deviceinfo");
    let old_bytes = fs::read(&asset)?;
    if sha256(&old_bytes) != old_sha {
        return Err("Existing helper and its source hash disagree".into());
    }
    let updated = format!("{prefix}{marker}{sha}\"{suffix}");
    fs::write(&asset, &bytes)?;
    if let Err(error) = fs::write(&native_path, updated) {
        fs::write(&asset, old_bytes)?;
        return Err(error.into());
    }
    println!(
        "Android ARM64 PIE helper: {} bytes, SHA-256 {sha}",
        bytes.len()
    );
    Ok(())
}
fn validate_elf(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 64
        || &bytes[..6] != b"\x7fELF\x02\x01"
        || u16::from_le_bytes(bytes[16..18].try_into()?) != 3
        || u16::from_le_bytes(bytes[18..20].try_into()?) != 183
    {
        return Err("Helper is not an ARM64 little-endian ELF PIE".into());
    }
    let offset = usize::try_from(u64::from_le_bytes(bytes[32..40].try_into()?))?;
    let entry_size = u16::from_le_bytes(bytes[54..56].try_into()?) as usize;
    let count = u16::from_le_bytes(bytes[56..58].try_into()?) as usize;
    if entry_size < 56 {
        return Err("Invalid ELF program headers".into());
    }
    for i in 0..count {
        let start = i
            .checked_mul(entry_size)
            .and_then(|n| offset.checked_add(n))
            .ok_or("ELF overflow")?;
        let header = bytes
            .get(start..start.checked_add(56).ok_or("ELF overflow")?)
            .ok_or("Truncated ELF headers")?;
        if u32::from_le_bytes(header[..4].try_into()?) == 3 {
            let begin = usize::try_from(u64::from_le_bytes(header[8..16].try_into()?))?;
            let size = usize::try_from(u64::from_le_bytes(header[32..40].try_into()?))?;
            if bytes.get(begin..begin.checked_add(size).ok_or("ELF overflow")?)
                == Some(b"/system/bin/linker64\0")
            {
                return Ok(());
            }
        }
    }
    Err("Helper does not use the Android 64-bit dynamic linker".into())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn elf_gate_rejects_a_host_executable_and_truncated_header() {
        assert!(validate_elf(b"MZ Windows").is_err());
        let mut elf = vec![0; 64];
        elf[..6].copy_from_slice(b"\x7fELF\x02\x01");
        elf[16..18].copy_from_slice(&3_u16.to_le_bytes());
        elf[18..20].copy_from_slice(&183_u16.to_le_bytes());
        assert!(validate_elf(&elf).is_err());
    }
}
