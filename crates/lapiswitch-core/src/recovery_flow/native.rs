//! Direct USB ADB with incremental stdout; no external adb.exe or shell-v2
//! assumption. USB shell in adb_client 3.2.3 returns no exit status, so every
//! command uses an explicit exit trailer and EOF alone can never mean success.
use super::{DeviceInfoIo, Ui};
use crate::{
    backup::{self, VerifiedBackup},
    deviceinfo::{self, map},
    slot::Slot,
};
use adb_client::{ADBDeviceExt, usb::ADBUSBDevice};
use std::io::Write;
use std::path::{Path, PathBuf};
const HELPER: &[u8] = include_bytes!("../../../../assets/lapiswitch_deviceinfo");
const HELPER_SHA: &str = "73151d164adf34bf2ebe94fbd88e2fb56f85678213621885e9561e33d21c2a88";
const EXIT: &str = "__LAPISWITCH_EXIT__:";

struct Output {
    all: Vec<u8>,
    line: Vec<u8>,
    stream: bool,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.all.len() + bytes.len() > 256 * 1024 {
            return Err(std::io::Error::other("ADB 응답 허용 크기 초과"));
        }
        self.all.extend_from_slice(bytes);
        if self.stream {
            for &b in bytes {
                if b == b'\n' {
                    let text = String::from_utf8_lossy(&self.line).trim().to_owned();
                    if !text.is_empty() && !text.starts_with(EXIT) {
                        ltbox_core::live_sink::emit(ltbox_core::live_sink::Entry::info(format!(
                            "[DeviceInfo] {text}"
                        )));
                    }
                    self.line.clear();
                } else {
                    self.line.push(b);
                }
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn command_status(output: &[u8]) -> Result<(String, String), String> {
    let text = String::from_utf8(output.to_vec()).map_err(|_| "ADB 응답이 UTF-8이 아닙니다.")?;
    let (body, trailer) = text
        .rsplit_once(EXIT)
        .ok_or("ADB 명령 종료 상태를 확인할 수 없습니다.")?;
    Ok((body.trim().into(), trailer.trim().into()))
}
pub(crate) fn command_result(output: &[u8]) -> Result<String, String> {
    let (body, code) = command_status(output)?;
    if code != "0" {
        return Err(format!("Recovery 명령 실패 (exit {code})"));
    }
    Ok(body)
}
/// The helper exits 1 (failure before the request), 2 (arguments) or 3 (lock
/// held) without sending 0x203. 40, a signal or a missing status is unknown.
/// The caller still requires two original reads before leaving WritePending.
fn write_outcome(output: &[u8]) -> Result<(), String> {
    let (_, code) =
        command_status(output).map_err(|error| format!("DeviceInfo 쓰기 결과 불명: {error}"))?;
    match code.as_str() {
        "0" => Ok(()),
        "1" | "2" | "3" => Err(format!(
            "DeviceInfo helper가 쓰기 요청 전에 종료했습니다 (exit {code})."
        )),
        _ => Err(format!(
            "DeviceInfo 쓰기 결과 불명 (exit {code}). 자동 재쓰기 없이 다시 읽어 확인합니다."
        )),
    }
}
fn saved_serial(raw: &str) -> Option<&str> {
    raw.lines().find_map(|line| {
        line.trim()
            .strip_prefix("serialno:")
            .map(str::trim)
            .filter(|s| !s.is_empty())
    })
}
/// The current TWRP slot must still match the slot backed up before staging.
/// Never fall back to the other Keymaster partition after a read/write error.
fn active_keymaster_path(expected: Slot, raw: &str) -> Result<&'static str, String> {
    let active = Slot::parse(raw).ok_or("TWRP 활성 슬롯을 읽지 못했습니다.")?;
    if active != expected {
        return Err("TWRP 활성 슬롯이 백업 슬롯과 다릅니다.".into());
    }
    Ok(deviceinfo::keymaster_path(active))
}
/// The plan whose transfers and dry-run succeeded; consumed by one write.
struct Prepared {
    tag: u32,
    before: String,
    planned: String,
}
pub struct Recovery {
    usb: ADBUSBDevice,
    remote: String,
    serial: String,
    slot: Slot,
    reads: u32,
    prepared: Option<Prepared>,
}
impl Recovery {
    pub fn wait(
        backup: &VerifiedBackup,
        ui: &mut impl Ui,
        log: &mut Vec<String>,
    ) -> Result<Self, String> {
        ui.status(
            "TWRP ADB 연결을 기다립니다. PC와 태블릿의 USB 연결을 유지해 주세요.",
            true,
        );
        let started = std::time::Instant::now();
        loop {
            super::stopped(ui)?;
            super::check_connection_timeout(super::Transition::Twrp, started.elapsed().as_secs())?;
            ltbox_device::selection::ensure_single_usb_target().map_err(|e| e.to_string())?;
            let key = ltbox_core::app_paths::adb_key_path();
            if !key.is_file() {
                let mut manager = ltbox_device::adb::AdbManager::new();
                let _ = manager.check_device();
            }
            if let Ok(usb) = ADBUSBDevice::autodetect_with_custom_private_key(key) {
                let remote = format!(
                    "/tmp/lapiswitch-{}-{}",
                    std::process::id(),
                    chrono::Utc::now()
                        .timestamp_nanos_opt()
                        .ok_or("시각 없음")?
                );
                let mut recovery = Self {
                    usb,
                    remote,
                    serial: String::new(),
                    slot: backup.manifest().slot,
                    reads: 0,
                    prepared: None,
                };
                // An authorized ADB connection is not necessarily recovery.
                let twrp = recovery.shell("getprop ro.twrp.version", false)?;
                if !twrp.is_empty() {
                    super::stopped(ui)?;
                    super::check_connection_timeout(
                        super::Transition::Twrp,
                        started.elapsed().as_secs(),
                    )?;
                    // Atomically retire the timed stop before identity/helper
                    // preparation and DeviceInfo work can start.
                    if !ui.finish_wait() {
                        return Err(
                            "연결 대기를 중지했습니다. 백업과 작업 기록을 보존했습니다.".into()
                        );
                    }
                    recovery.verify_identity(backup)?;
                    recovery.serial = recovery.shell("getprop ro.serialno", false)?;
                    if recovery.serial.is_empty() {
                        return Err("TWRP serial을 읽지 못했습니다.".into());
                    }
                    recovery.prepare(log)?;
                    ui.status("TWRP 연결을 확인했습니다. DeviceInfo를 처리합니다.", false);
                    return Ok(recovery);
                }
            }
            super::check_connection_timeout(super::Transition::Twrp, started.elapsed().as_secs())?;
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    fn shell_output(&mut self, command: &str, stream: bool) -> Result<Vec<u8>, String> {
        ltbox_device::selection::ensure_single_usb_target().map_err(|e| e.to_string())?;
        let wrapped = format!("{command}; ls_status=$?; printf '\\n{EXIT}%s\\n' \"$ls_status\"");
        let mut output = Output {
            all: vec![],
            line: vec![],
            stream,
        };
        self.usb
            .shell_command(&wrapped, Some(&mut output), None)
            .map_err(|e| e.to_string())?;
        Ok(output.all)
    }
    fn shell(&mut self, command: &str, stream: bool) -> Result<String, String> {
        command_result(&self.shell_output(command, stream)?)
    }
    /// The helper only enforces the model's shared tail; the PC verified the
    /// field plan before this command is built.
    fn helper_command(&self, ta: &str, tag: u32) -> String {
        format!(
            "{remote}/deviceinfo --profile {model} --ta {ta} --expected {remote}/expected{tag}.bin --planned {remote}/planned{tag}.bin",
            remote = self.remote,
            model = crate::firmware::MODEL,
        )
    }
    fn pull(&mut self, remote: &str, out: &Path, expected: Option<u64>) -> Result<Vec<u8>, String> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out)
            .map_err(|e| e.to_string())?;
        self.usb.pull(&remote, &mut f).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        let len = std::fs::metadata(out).map_err(|e| e.to_string())?.len();
        if expected.is_some_and(|e| e != len) || len > 64 * 1024 * 1024 {
            return Err("Recovery 파일 길이가 잘못되었습니다.".into());
        }
        std::fs::read(out).map_err(|e| e.to_string())
    }
    fn push_checked(
        &mut self,
        bytes: &[u8],
        remote: &str,
        expected_hash: &str,
    ) -> Result<(), String> {
        self.usb
            .push(&mut std::io::Cursor::new(bytes), &remote)
            .map_err(|e| e.to_string())?;
        let actual = self.shell(&format!("sha256sum {remote}"), false)?;
        if actual.split_whitespace().next() != Some(expected_hash) {
            return Err("Recovery 전송 파일 SHA-256 불일치".into());
        }
        Ok(())
    }
    pub fn verify_identity(&mut self, backup: &VerifiedBackup) -> Result<(), String> {
        if self.shell("id -u", false)? != "0"
            || self.shell("getprop ro.twrp.version", false)?.is_empty()
        {
            return Err("root TWRP 환경이 아닙니다.".into());
        }
        let model = self.shell("getprop ro.product.model", false)?;
        let product = self.shell("getprop ro.product.device", false)?;
        if !crate::prepare::supported_model(&model) || !product.eq_ignore_ascii_case("lapis") {
            return Err("TWRP 기기가 TB520FU/lapis로 확인되지 않았습니다.".into());
        }
        if self.shell("getprop ro.product.cpu.abi", false)? != "arm64-v8a" {
            return Err("ARM64 Recovery가 아닙니다.".into());
        }
        self.active_keymaster()?;
        if !self.serial.is_empty() && self.shell("getprop ro.serialno", false)? != self.serial {
            return Err("TWRP 기기가 바뀌었습니다.".into());
        }
        let serial = self.shell("getprop ro.serialno", false)?;
        if serial.is_empty() {
            return Err("TWRP serial을 확인할 수 없습니다.".into());
        }
        if !backup.manifest().started_in_edl {
            let raw = std::fs::read_to_string(backup.file_path(crate::backup::IDENTITY_NAME)?)
                .map_err(|e| e.to_string())?;
            if saved_serial(&raw) != Some(serial.as_str()) {
                return Err("TWRP serial이 시작 시 ADB 기록과 다릅니다.".into());
            }
        }
        // EDL starts have no ADB identity record; the disk GUID below binds
        // them to the original device, together with model/slot and live serial.
        self.serial = serial;
        // Identify the GPT disk through sysfs, never assume a UFS /dev/sdX name.
        // Only read two 4096-byte sectors to a private /tmp file.
        self.reads += 1;
        let remote = format!("{}/identity_header_{}.bin", self.remote, self.reads);
        self.shell(&format!("mkdir -p -m 700 {}", self.remote), false)?;
        let command = format!(
            "p=$(readlink -f /dev/block/bootdevice/by-name/recovery{}) && p=${{p##*/}} && parent=$(readlink -f /sys/class/block/\"$p\"/..) && parent=${{parent##*/}} && test \"$parent\" != \"$p\" && test \"$(cat /sys/class/block/\"$parent\"/queue/logical_block_size)\" = 4096 && dd if=/dev/block/\"$parent\" of={remote} bs=4096 count=2 2>/dev/null",
            self.slot.suffix()
        );
        self.shell(&command, false)?;
        let work = tempfile::tempdir_in(backup.work_dir()?).map_err(|e| e.to_string())?;
        let header = self.pull(&remote, &work.path().join("header.bin"), Some(8192))?;
        if header.get(4096..4104) != Some(b"EFI PART".as_slice()) {
            return Err("TWRP에서 읽은 GPT 헤더가 잘못되었습니다.".into());
        }
        let original =
            std::fs::read(backup.file_path("gpt_main4.bin")?).map_err(|e| e.to_string())?;
        if crate::storage::disk_guid(&header)? != crate::storage::disk_guid(&original)? {
            return Err("TWRP GPT disk GUID가 백업 기기와 다릅니다.".into());
        }
        Ok(())
    }
    fn active_keymaster(&mut self) -> Result<&'static str, String> {
        let current = self.shell("getprop ro.boot.slot_suffix", false)?;
        active_keymaster_path(self.slot, &current)
    }
    fn prepare(&mut self, log: &mut Vec<String>) -> Result<(), String> {
        use sha2::{Digest, Sha256};
        let hash: String = Sha256::digest(HELPER)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != HELPER_SHA {
            return Err("내장 DeviceInfo helper 해시 불일치".into());
        }
        self.shell(&format!("mkdir -p -m 700 {}", self.remote), false)?;
        let helper = format!("{}/deviceinfo", self.remote);
        self.push_checked(HELPER, &helper, HELPER_SHA)?;
        self.shell(&format!("chmod 700 {helper}"), false)?;
        self.shell(&format!("{helper} --self-test"), true)?;
        let ta = self.active_keymaster()?;
        self.shell(&format!("test -r {ta}"), false)
            .map_err(|e| format!("활성 슬롯의 Keymaster 파티션을 읽을 수 없습니다: {ta}: {e}"))?;
        ltbox_core::live!(
            log,
            "DeviceInfo helper 준비 완료. 활성 슬롯 {}의 {}를 사용합니다 (공유 버퍼 4096).",
            self.slot.suffix(),
            ta
        );
        Ok(())
    }
    pub fn reboot_edl(&mut self) -> Result<(), String> {
        self.shell("reboot edl", false).map(|_| ())
    }
}
impl DeviceInfoIo for Recovery {
    fn read(&mut self, out: &Path, _log: &mut Vec<String>) -> Result<Vec<u8>, String> {
        let ta = self.active_keymaster()?;
        self.reads += 1;
        let prefix = format!("{}/read{}", self.remote, self.reads);
        self.shell(
            &format!(
                "{}/deviceinfo --profile {} --read --ta {} --out-prefix {prefix}",
                self.remote,
                crate::firmware::MODEL,
                ta
            ),
            true,
        )?;
        let path = PathBuf::from(format!("{}.before.bin", out.display()));
        let bytes = self.pull(
            &format!("{prefix}.before.bin"),
            &path,
            Some(deviceinfo::SHARED_SIZE as u64),
        )?;
        // Any 4096 bytes are returned: an edited magic must still be verifiable.
        Ok(bytes)
    }
    fn prepare(
        &mut self,
        before: &[u8],
        planned: &[u8],
        _log: &mut Vec<String>,
    ) -> Result<(), String> {
        self.prepared = None;
        let ta = self.active_keymaster()?;
        map::check_plan(deviceinfo::LAYOUT, before, planned)?;
        self.reads += 1;
        let tag = self.reads;
        let (before_hash, planned_hash) =
            (backup::digest_bytes(before), backup::digest_bytes(planned));
        let expected = format!("{}/expected{tag}.bin", self.remote);
        let planned_remote = format!("{}/planned{tag}.bin", self.remote);
        self.push_checked(before, &expected, &before_hash)?;
        self.push_checked(planned, &planned_remote, &planned_hash)?;
        let command = self.helper_command(ta, tag);
        self.shell(
            &format!("{command} --out-prefix {}/dry{tag}", self.remote),
            true,
        )?;
        self.prepared = Some(Prepared {
            tag,
            before: before_hash,
            planned: planned_hash,
        });
        Ok(())
    }
    fn apply(
        &mut self,
        before: &[u8],
        planned: &[u8],
        out: &Path,
        _log: &mut Vec<String>,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        // One write per successful dry-run of the identical plan.
        let prepared = self
            .prepared
            .take()
            .ok_or("DeviceInfo dry-run 기록이 없어 쓰기를 요청하지 않았습니다.")?;
        if prepared.before != backup::digest_bytes(before)
            || prepared.planned != backup::digest_bytes(planned)
        {
            return Err("dry-run과 다른 DeviceInfo 계획이라 쓰기를 요청하지 않았습니다.".into());
        }
        // Recheck after dry-run and immediately before the only write request.
        let ta = self.active_keymaster()?;
        let tag = prepared.tag;
        let command = self.helper_command(ta, tag);
        // Once only. No retry path around this call.
        let output = self
            .shell_output(
                &format!(
                    "{command} --apply-once --out-prefix {}/apply{tag}",
                    self.remote
                ),
                true,
            )
            .map_err(|error| format!("DeviceInfo 쓰기 명령 연결 오류 (결과 불명): {error}"))?;
        write_outcome(&output)?;
        let same = self.pull(
            &format!("{}/apply{tag}.after_same_session.bin", self.remote),
            &PathBuf::from(format!("{}.same.bin", out.display())),
            Some(deviceinfo::SHARED_SIZE as u64),
        )?;
        let fresh = self.pull(
            &format!("{}/apply{tag}.after_new_session.bin", self.remote),
            &PathBuf::from(format!("{}.fresh.bin", out.display())),
            Some(deviceinfo::SHARED_SIZE as u64),
        )?;
        Ok((same, fresh))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_slot_routes_keymaster_and_slot_changes_fail_before_helper_commands() {
        for (slot, values, path) in [
            (
                Slot::A,
                ["a", "_a", " A ", "_A\n"],
                "/dev/block/bootdevice/by-name/keymaster_a",
            ),
            (
                Slot::B,
                ["b", "_b", " B ", "_B\n"],
                "/dev/block/bootdevice/by-name/keymaster_b",
            ),
        ] {
            for value in values {
                assert_eq!(active_keymaster_path(slot, value).unwrap(), path);
                assert!(active_keymaster_path(slot.other(), value).is_err());
            }
            for unknown in ["", "_", "c", "unknown", "_a _b"] {
                assert!(active_keymaster_path(slot, unknown).is_err());
            }
        }
    }
    #[test]
    fn adb_identity_serial_binding_requires_an_explicit_nonempty_field() {
        assert_eq!(
            saved_serial(&crate::prepare::identity_record(Slot::B, "FAKE123")),
            Some("FAKE123")
        );
        assert_eq!(
            saved_serial("serialno: FAKE123\nmodel:TB520FU"),
            Some("FAKE123")
        );
        assert_eq!(saved_serial("serialno: \n"), None);
        assert_eq!(saved_serial("model:TB520FU"), None);
    }
    #[test]
    fn eof_or_bad_exit_cannot_succeed() {
        assert!(command_result(b"{}\n").is_err());
        assert!(command_result(b"{}\n__LAPISWITCH_EXIT__:40\n").is_err());
        assert_eq!(
            command_result(b"{}\n__LAPISWITCH_EXIT__:0\n").unwrap(),
            "{}"
        );
    }
    #[test]
    fn only_helper_exits_before_the_request_are_reported_as_not_sent() {
        assert!(write_outcome(b"{}\n__LAPISWITCH_EXIT__:0\n").is_ok());
        for code in ["1", "2", "3"] {
            let output = format!("{{}}\n{EXIT}{code}\n");
            assert!(
                write_outcome(output.as_bytes())
                    .unwrap_err()
                    .contains("쓰기 요청 전")
            );
        }
        for output in [
            "{}\n__LAPISWITCH_EXIT__:40\n",
            "{}\n__LAPISWITCH_EXIT__:139\n",
            "{}\n__LAPISWITCH_EXIT__:\n",
            "{\"phase\":\"WRITE_REQUEST_BEGIN\"}\n",
        ] {
            assert!(
                write_outcome(output.as_bytes())
                    .unwrap_err()
                    .contains("결과 불명")
            );
        }
    }
    #[test]
    fn streaming_handles_chunk_boundaries() {
        let mut o = Output {
            all: vec![],
            line: vec![],
            stream: true,
        };
        o.write_all(b"{\"phase\":").unwrap();
        o.write_all(b"\"TEST\"}\n__LAPISWITCH_EXIT__:0\n").unwrap();
        assert_eq!(command_result(&o.all).unwrap(), "{\"phase\":\"TEST\"}");
    }
}
