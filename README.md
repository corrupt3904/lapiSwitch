<p align="center">
  <img src="crates/lapiswitch-gui/assets/branding/lapiswitch.png" alt="lapiSwitch" width="96">
</p>

<h1 align="center">lapiSwitch</h1>

<p align="center">PRC ↔ ROW region conversion for Lenovo TB520FU (<code>lapis</code>)</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-x64-0078D6" alt="Windows x64">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="GPL-3.0-or-later"></a>
</p>

<p align="center">
  English · <a href="README.ko.md">한국어</a> · <a href="https://github.com/corrupt3904/lapiSwitch/releases">Download</a>
</p>

lapiSwitch is an unofficial Windows tool developed for researching and experimenting with **PRC (China) ↔ ROW (global)** region conversion on the **Lenovo TB520FU**, also known as Yoga Tab Plus.

lapiSwitch is an **independent tool based on [LTBox](https://github.com/miner7222/LTBox) by [@miner7222](https://github.com/miner7222)**. It uses the **[TB520FU TWRP build](https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp/releases/tag/v3.7.1_16-20261014) published by [@wnduddld0513](https://github.com/wnduddld0513)**.

It backs up, updates and verifies DeviceInfo in TWRP, restores the original GPT and Recovery, and installs official firmware for the selected region.

> [!WARNING]
> **Read before use**
>
> - Firmware and region changes can cause **boot failure (bricking), device damage or data loss**. Recovery is not guaranteed.
> - **Region conversion always factory-resets user data**, including apps, photos and accounts stored on the tablet.
> - Modifications **may void the manufacturer's warranty or affect repair and support eligibility**, depending on the applicable terms and law.
>
> Back up personal files separately and retain the original backups and logs created by the app. Understand the procedure and risks before starting, and keep USB connected throughout the operation.

The software is provided **“AS IS,” without warranty**, to the extent permitted by applicable law. Developers and contributors disclaim liability for device damage, data loss and other losses arising from its use or inability to use it, except where liability cannot lawfully be excluded. See Sections 15–16 of [LICENSE](LICENSE).

## Features

- **Guided workflow** — target region, firmware checks, conversion options and a final review.
- **Device monitoring** — ADB, Fastboot and EDL status on the Dashboard, with driver checks and supported installation options.
- **Backup and verification** — original partition backups and full 4096-byte DeviceInfo readback verification after writing.
- **Conversion options** — country code selection and rollback index reset (default) or preservation. With preservation, a firmware whose rollback indices are lower than the values stored in DeviceInfo opens a prompt in TWRP: reset them to 0 and continue, or cancel and automatically restore the original configuration.
- **Saved progress** — resume and original-configuration restore, subject to the saved state and verified backup evidence.

Interface languages: 한국어, English, 简体中文, 日本語 and Русский.

## Supported configuration

| Item | Requirement |
|---|---|
| Host OS | **Windows x64 only** |
| Tablet | **Lenovo TB520FU (`lapis`)**; other models are blocked |
| Installed PRC firmware | `ZUXOS_1.5.10.186` or earlier; preceding PRC ZUI builds are also allowed |
| Installed ROW firmware | `ZUI_18.5.10.037` or earlier |
| Installation package | Unpacked official firmware matching the **target region** |
| Connection | Reliable USB cable, adequate battery charge and internet access for required downloads |

The version limits above apply to firmware **already installed on the tablet**. The app checks the selected installation package separately. Identified stock firmware above these limits blocks conversion. If the stock version or region cannot be identified, the app warns about a possible custom ROM, GSI or test build and requires an explicit choice to continue. Connection waits in STEP 5 and STEP 7 allow stopping after 30 seconds and time out after 3 minutes, preserving the saved state. Check the [release notes](https://github.com/corrupt3904/lapiSwitch/releases) for the validation scope and limitations of each prerelease.

## Getting started

1. Download the Windows ZIP and its `.zip.sha256` checksum from [Releases](https://github.com/corrupt3904/lapiSwitch/releases). Verify the checksum, then extract the ZIP.
2. Run `lapiSwitch.exe`. Follow the app's USB driver guidance and install supported drivers if required.
3. Boot the tablet to Android, enable USB debugging and connect it. Confirm its model and connection state on the **Dashboard** and approve the USB debugging prompt. Conversion starts from ADB (or EDL); it cannot start from Fastboot.
4. Select **Region conversion**, then choose the target region, firmware folder and conversion options. Review the final confirmation carefully.
5. Keep USB connected and follow the reboot prompts until the operation finishes.

The workflow is **checks and backups → TWRP / DeviceInfo update and verification → original configuration restore → firmware installation and reboot**.

TWRP is downloaded separately from the pinned [lapis release `v3.7.1_16-20261014`](https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp/releases/tag/v3.7.1_16-20261014). Its size and SHA-256 are checked on every use.

## Backups and recovery

The default data location is `%LOCALAPPDATA%\lapiSwitch`:

| Contents | Location |
|---|---|
| Operation logs | `logs` |
| Original backups | `backup\region_change` |

Change the root folder under **Settings → Logs and backup location**. Keep the complete backup folder and saved records together.

Use **Recovery** on the Dashboard to restore the backed-up GPT, Recovery and vendor_boot configuration and, when applicable, the original DeviceInfo region. **This does not roll back the entire installed firmware.**

Resume and restore depend on the available evidence. If the DeviceInfo write result cannot be established or backup verification fails, the app may refuse automatic recovery.

Backups and saved runs from earlier app versions cannot be opened by this version. Finish or restore any interrupted run with the version that created it before updating.

## Build from source

Install [Rust through rustup](https://rustup.rs/) and Visual Studio Build Tools with **C++ build tools and a Windows SDK**. The repository pins Rust **1.95.0** in [`rust-toolchain.toml`](rust-toolchain.toml).

Run from the repository root:

```powershell
cargo build --release --locked -p lapiswitch-gui
```

The executable is `target\release\lapiSwitch.exe`. Close any running copy before rebuilding.

For a preview without connecting to a device:

```powershell
cargo run --locked -p lapiswitch-gui -- --demo
```

See [`tools/deviceinfo/README.md`](tools/deviceinfo/README.md) for the Android helper and its validation scope.

## Credits

- **[LTBox](https://github.com/miner7222/LTBox) — [@miner7222](https://github.com/miner7222):** the foundation for device communication, EDL flashing, driver and firmware handling, and selected GUI assets. Its vendored `ltbox-core` and `ltbox-device` sources are retained unmodified. AVB parsing uses [avbtool-rs](https://github.com/miner7222/avbtool-rs).
- **[TeamWin / TWRP](https://twrp.me/), TWRP-Test and lapis contributors:** the [TB520FU recovery build](https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp/releases/tag/v3.7.1_16-20261014) is published by [@wnduddld0513](https://github.com/wnduddld0513) under [lenovo-sm8650](https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp).
- **Other components:** [qdlrs](https://github.com/qualcomm/qdlrs), [flag-icons](https://github.com/lipis/flag-icons), [Noto Sans KR](https://github.com/notofonts/noto-cjk), [iced](https://github.com/iced-rs/iced) and the Rust dependencies recorded in [`Cargo.lock`](Cargo.lock).

## License

lapiSwitch is an independent project based on LTBox and licensed under **GPL-3.0-or-later**. See [LICENSE](LICENSE). It is not an official Lenovo product.

Third-party components retain their respective licenses. Open **Settings → Open-source licenses → View licenses** to read the included notices and license texts offline, including the TWRP / TeamWin entry and its source references. GUI asset provenance is documented in [`crates/lapiswitch-gui/assets/README.md`](crates/lapiswitch-gui/assets/README.md).

Product names and trademarks belong to their respective owners.
