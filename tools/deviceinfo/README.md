# Rust DeviceInfo helper

Android ARM64 / API 30 / Android NDK r27d. The helper is implemented in Rust.
Its QSEECom message lengths, packed pointer offset and DMA-heap ioctls preserve
the device-validated transport. The vendor's system QSEECom library is loaded
through its existing C ABI; no C source is built in this project.

From the repository root:

```text
rustup target add aarch64-linux-android
cargo run -p lapiswitch-tools -- build-helper --ndk D:/Android/android-ndk-r27d
```

Alternatively set `ANDROID_NDK_HOME`. The build command checks the ARM64 PIE
ELF and Android interpreter, then updates both `assets/lapiswitch_deviceinfo`
and the embedded SHA-256 in `recovery_flow/native.rs`.

The GUI uploads this helper and checks its SHA-256 locally and remotely.
`--self-test` exercises both regions, keep/reset and every protected byte;
it never opens QSEECom and also works in a desktop build:

```text
cargo run -p lapiswitch-deviceinfo -- --self-test
cargo test -p lapiswitch-deviceinfo
```

Device commands require an explicit `--profile TB520FU|TB522FU` and `--ta`
selection of the confirmed active slot's `keymaster_a` or `keymaster_b`; there
is no default or alternate-slot fallback. Read uses `--read --out-prefix PATH`.
A dry run uses `--expected`, `--planned`, `--out-prefix`; only explicit
`--apply-once` issues command 0x203. The read state must carry the
`ANDROID-BOOT!` magic. Every byte of the profile's logical DeviceInfo
(TB520FU `0x000..0xEA7`, TB522FU `0x000..0xD1F`) may be planned; the field
values are chosen and verified on the PC against the structure maps. The
shared-buffer tail after it must be preserved. DeviceInfo and plan files are
exactly 4096 bytes.

The exclusive `/tmp/lapiswitch-deviceinfo.lock` covers reads and writes.
Evidence files are created once, synchronized to disk and never overwritten.
The marker is saved after DMA preparation and before the one write request.
Successful apply requires exact 4096-byte readbacks in the same TA session
and a newly opened session. The desktop app performs its own independent
process readback and keeps its existing backup/journal policy.

Exit codes: 0 success; 1 failure before a write attempt; 2 invalid arguments;
3 lock unavailable; 40 an attempted write requiring reconciliation. A lost
exit status remains unknown. No automatic retry is performed. Phase JSONL
events retain protocol version 1. No private dumps or TA binaries belong here.

## Verification scope

The Rust port passed host policy/fault-injection tests and Android ARM64
compilation. On TB520FU TWRP 3.7.1_16, active slot a, two independent Rust
reads matched the former C helper across all 4096 bytes; the on-device
self-test and dry run passed, and a final baseline read was unchanged.
That initial migration check was read-only and issued no 0x203 write.

The project maintainer subsequently reported hardware validation of the
prerelease on Windows x64 with a real TB520FU, covering the main region
conversion, DeviceInfo update/readback verification, firmware installation
and original-configuration restore workflows. This report applies to the
scenarios actually exercised; it does not assert exhaustive coverage of
every slot, firmware version or interruption/failure combination.
