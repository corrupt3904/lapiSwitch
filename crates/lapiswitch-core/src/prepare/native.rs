//! LTBox transport adapter. Vendored crates remain unmodified.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use ltbox_device::adb::AdbManager;
use ltbox_device::driver::{QcomDriverMode, qcom_driver_mode};
use ltbox_device::edl::{self, EdlError, EdlSession};
use ltbox_device::fastboot::{FastbootDevice, FastbootError, FastbootVars};

use super::{AdbIdentity, Arrival, Connection, DeviceIo};
use crate::avb_info::{self, AvbSummary};
use crate::slot::Slot;

pub struct NativeDevice {
    adb: AdbManager,
    fastboot: Option<FastbootDevice>,
    scratch: tempfile::TempDir,
}

impl NativeDevice {
    /// Scratch images contain device data; keep them in a unique directory
    /// under the caller's private output location and remove them on drop.
    pub fn new(work_dir: &Path) -> Result<Self, String> {
        let partition_dir = crate::backup::partition_directory(work_dir)?;
        let scratch = tempfile::Builder::new()
            .prefix("edl-probe-")
            .tempdir_in(partition_dir)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            adb: AdbManager::new(),
            fastboot: None,
            scratch,
        })
    }

    pub(crate) fn scratch_dir(&self) -> &Path {
        self.scratch.path()
    }

    /// Keep transport error types until a caller decides whether to wait again.
    pub(crate) fn open_edl_session(
        &mut self,
        loader: &Path,
        log: &mut Vec<String>,
    ) -> Result<EdlSession, EdlError> {
        self.fastboot = None;
        self.adb = AdbManager::new();
        EdlSession::open(loader, log)
    }

    fn fastboot(&mut self) -> Result<&mut FastbootDevice, String> {
        if self.fastboot.is_none() {
            self.fastboot = Some(FastbootDevice::open().map_err(|error| error.to_string())?);
        }
        Ok(self.fastboot.as_mut().expect("opened fastboot"))
    }

    fn edl_present() -> Result<bool, String> {
        let result = match qcom_driver_mode() {
            QcomDriverMode::Userspace => edl::find_edl_device(),
            QcomDriverMode::Kernel => edl::find_edl_port(),
        };
        match result {
            Ok(_) => Ok(true),
            Err(EdlError::PortNotFound) => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }
}

impl DeviceIo for NativeDevice {
    type Session = EdlSession;

    fn ensure_single_target(&mut self) -> Result<(), String> {
        ltbox_device::selection::ensure_single_usb_target().map_err(|error| error.to_string())
    }

    fn detect(&mut self) -> Result<Connection, String> {
        if Self::edl_present()? {
            return Ok(Connection::Edl);
        }
        match FastbootDevice::open() {
            Ok(device) => {
                self.fastboot = Some(device);
                return Ok(Connection::Fastboot);
            }
            Err(FastbootError::DeviceNotFound) => {}
            Err(error) => return Err(error.to_string()),
        }
        match self
            .adb
            .check_device_state()
            .map_err(|error| error.to_string())?
        {
            Some("device" | "recovery") => Ok(Connection::Adb),
            Some(state) => Err(format!(
                "ADB 상태: {state}. USB 디버깅 허용 또는 드라이버 상태를 확인해 주세요."
            )),
            None => Ok(Connection::None),
        }
    }

    fn adb_identity(&mut self) -> Result<AdbIdentity, String> {
        let model = self.adb.get_model().map_err(|error| error.to_string())?;
        let slot = self
            .adb
            .get_slot_suffix()
            .map_err(|error| error.to_string())?
            .as_deref()
            .and_then(Slot::parse);
        Ok(AdbIdentity { model, slot })
    }

    fn adb_reboot(&mut self, target: &str) -> Result<(), String> {
        let result = self.adb.reboot(target).map_err(|error| error.to_string());
        // A mode change invalidates the old USB claim even on response failure.
        self.adb = AdbManager::new();
        result
    }

    fn wait_fastboot(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            self.ensure_single_target()?;
            match FastbootDevice::open() {
                Ok(device) => {
                    self.fastboot = Some(device);
                    return Ok(());
                }
                Err(FastbootError::DeviceNotFound) => {}
                Err(error) => return Err(error.to_string()),
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "{}초 안에 fastboot에 연결되지 않았습니다.",
                    timeout.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    fn fastboot_vars(&mut self) -> Result<FastbootVars, String> {
        let device = self.fastboot()?;
        if device.is_userspace() {
            return Err(
                "fastbootd에 연결되어 있습니다. bootloader fastboot로 전환해 주세요.".into(),
            );
        }
        device.get_all_vars().map_err(|error| error.to_string())
    }

    fn reboot_system(&mut self) -> Result<(), String> {
        let result = self.fastboot()?.reboot().map_err(|error| error.to_string());
        self.fastboot = None;
        result
    }

    fn wait_adb_or_edl(&mut self) -> Result<Arrival, String> {
        // LTBox's ADB timeout is 120s. Poll with the SAME manager later used
        // for reboot edl, avoiding a second USB interface claim. Unlike the
        // bool probe, propagate multiple-device/enumeration errors.
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            self.ensure_single_target()?;
            if Self::edl_present()? {
                return Ok(Arrival::Edl);
            }
            match self
                .adb
                .check_device_state()
                .map_err(|error| error.to_string())?
            {
                Some("device" | "recovery") => return Ok(Arrival::Adb),
                Some(state) => return Err(format!("ADB 상태: {state}")),
                None => {}
            }
            if Instant::now() >= deadline {
                return Err("시스템 부팅 후 ADB/EDL 연결 대기 시간이 초과되었습니다.".into());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    fn wait_edl(&mut self) -> Result<(), String> {
        edl::wait_for_device()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn open_edl(&mut self, loader: &Path, log: &mut Vec<String>) -> Result<EdlSession, String> {
        self.open_edl_session(loader, log)
            .map_err(|error| error.to_string())
    }

    fn read_avb(
        &mut self,
        session: &mut EdlSession,
        partition: &str,
        log: &mut Vec<String>,
    ) -> Result<Option<AvbSummary>, String> {
        // Only fixed internal names become filenames.
        if !matches!(
            partition,
            "boot_a" | "boot_b" | "vbmeta_system_a" | "vbmeta_system_b"
        ) {
            return Err(format!("지원하지 않는 AVB 파티션: {partition}"));
        }
        let lun = ltbox_core::partition_lun::lun_for_partition(partition)
            .ok_or_else(|| format!("LUN을 찾을 수 없습니다: {partition}"))?;
        let path = self.scratch.path().join(format!("{partition}.img"));
        match session.dump_partition(partition, &path, 0, lun, log) {
            Ok(()) => {}
            Err(EdlError::PartitionNotFound(_)) => return Ok(None),
            Err(error) => return Err(error.to_string()),
        }
        // Confirm local I/O independently: an I/O fault must not look like
        // an invalid AVB image on an unused slot. Avoid re-reading huge boots.
        let mut first_byte = [0u8; 1];
        File::open(&path)
            .and_then(|mut file| file.read_exact(&mut first_byte))
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let info = avb_info::read_avb(&path);
        // read_avb's external parser erases I/O error types. A successfully
        // dumped image that fails AVB parsing is treated as invalid, as LTBox.
        if let Err(error) = &info {
            ltbox_core::live!(log, "{partition}: AVB 파싱 불가: {error}");
        }
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        Ok(info.ok())
    }

    fn edl_active_slot(
        &mut self,
        session: &mut EdlSession,
        log: &mut Vec<String>,
    ) -> Result<Slot, String> {
        let work = tempfile::tempdir_in(self.scratch.path()).map_err(|e| e.to_string())?;
        let header_path = work.path().join("header.bin");
        session
            .dump_partition_at("gpt_header4", &header_path, 4, 0, 2, log)
            .map_err(|e| e.to_string())?;
        let header = std::fs::read(&header_path).map_err(|e| e.to_string())?;
        let geometry = crate::storage::primary_geometry(&header)?;
        if geometry.size_bytes()? > 1024 * 1024 {
            return Err("GPT 크기가 허용 범위를 초과합니다.".into());
        }
        let path = work.path().join("gpt.bin");
        session
            .dump_partition_at("gpt_main4", &path, 4, 0, geometry.num_sectors as usize, log)
            .map_err(|e| e.to_string())?;
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.get(..header.len()) != Some(header.as_slice()) {
            return Err("활성 슬롯 확인 중 GPT 헤더가 변경되었습니다.".into());
        }
        let gpt = crate::storage::validate_gpt(&bytes)?;
        if gpt
            .entries()
            .iter()
            .filter(|entry| matches!(entry.name.as_str(), "boot_a" | "boot_b"))
            .all(|entry| (entry.attributes >> 48) & 0xff == 0)
        {
            ltbox_core::live!(
                log,
                "S1-E: GPT 슬롯 제어 값이 초기 상태입니다. Lapis 부트로더의 초기 기본 슬롯 A를 사용합니다."
            );
        }
        gpt.active_slot()
    }

    fn reset_to_edl(
        &mut self,
        session: &mut EdlSession,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        session.reset_to_edl(log).map_err(|error| error.to_string())
    }
}
