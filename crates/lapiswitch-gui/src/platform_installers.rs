//! LTBox's privileged udev entry point, invoked only by its installer.
#[cfg(target_os = "linux")]
const UDEV_RULES_PATH: &str = "/etc/udev/rules.d/51-ltbox-qcom.rules";
#[cfg(target_os = "linux")]
const UDEV_RULES_CONTENT: &str = include_str!("../../../misc/udev/51-ltbox-qcom.rules");
#[cfg(target_os = "linux")]
pub fn install_udev_rules() -> ! {
    eprintln!("[tb520] Installing udev rules → {UDEV_RULES_PATH}");
    if let Err(e) = std::fs::write(UDEV_RULES_PATH, UDEV_RULES_CONTENT) {
        eprintln!("[tb520] write {UDEV_RULES_PATH}: {e}");
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            let exe = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "ltbox".into());
            eprintln!();
            eprintln!("[tb520] Permission denied — needs root. Re-run as:");
            eprintln!("  sudo {exe} --install-udev");
            eprintln!("  pkexec {exe} --install-udev");
        }
        std::process::exit(1);
    }
    // Force the rules world-readable. A hardened root umask (0077) would
    // otherwise leave the file 0600, which udev can't apply and which the
    // GUI's read-back verification (run as the normal user) would read as
    // `UdevRulesNoPermission` — making reinstall appear to fail and loop.
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) =
        std::fs::set_permissions(UDEV_RULES_PATH, std::fs::Permissions::from_mode(0o644))
    {
        eprintln!("[tb520] WARNING: chmod 0644 {UDEV_RULES_PATH}: {e}");
    }
    let reload_ok = std::process::Command::new("udevadm")
        .args(["control", "--reload"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !reload_ok {
        eprintln!(
            "[tb520] WARNING: `udevadm control --reload` failed (rules still on disk; reboot will pick them up)"
        );
    }
    let trigger_ok = std::process::Command::new("udevadm")
        .arg("trigger")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !trigger_ok {
        eprintln!(
            "[tb520] WARNING: `udevadm trigger` failed (replug device manually to apply rules)"
        );
    }
    eprintln!();
    eprintln!(
        "[tb520] Done. Replug a connected Qualcomm 9008 / Lenovo USB device for the new ACL grants to take effect."
    );
    std::process::exit(0);
}
#[cfg(not(target_os = "linux"))]
pub fn install_udev_rules() -> ! {
    eprintln!("udev rules installation is only supported on Linux.");
    std::process::exit(1);
}
