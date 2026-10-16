# lapiSwitch DeviceInfo helper

ARM64 Android API 30 / NDK r27d. The transport comes from the user's successful
`TB520FU_QSEECOM_READONLY_KM_SHARED_0x1000_V4.c`; the product wrapper is
`lapiswitch_deviceinfo.c`. No private dumps or TA binaries belong here.

Build from this directory (the NDK path is an example):

```powershell
.\build.ps1 -NdkRoot 'D:\Android\android-ndk-r27d'
```

The committed `assets/lapiswitch_deviceinfo` is embedded in the GUI. Both PC and
remote SHA-256 must match the constant in `recovery_flow/native.rs`. Source
changes require rebuilding it and deliberately updating the embedded hash.

The helper supports `--read --out-prefix PATH` and a dry run with `--expected`,
`--planned`, `--out-prefix`. Only explicit `--apply-once` sends 0x203. Optional
`--reset-rollback` allows the additional 0x898..0x997 range and requires all of
that range to be zero. All other bytes except the three region characters must
remain identical. Region may already equal the target.

`--self-test` performs CPU-only keep/reset/region and protected-byte tests, then
exits before loading QSEECom or opening the TA. Production runs this after each
helper upload. Normal reads use 0x200/0x202; writes require version 5.3, app
5.3000. The host explicitly selects keymaster_a or keymaster_b from the
confirmed active slot for every read, dry run and write. It does not restrict
the TA to a firmware-specific partition hash. There is no
automatic alternate-slot or unknown-TA fallback.

The helper holds an exclusive process lock, saves a unique write intent before
0x203, syncs CPU DMA access, and re-reads all 4096 bytes through a fresh buffer
and a new TA session. Exit 40 means an attempted write needs reconciliation;
the PC must not retry it automatically. PC-side backup/state and independent
process readback remain mandatory. JSONL phase events have protocol version 1.

The upstream read-only reference retains its original command-line behavior;
the product never invokes its hexdump-producing main function. Product read
mode saves bytes to files and emits phase/command status only.
