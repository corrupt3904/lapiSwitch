//! LTBox transport adapter. Vendored crates remain unmodified.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use ltbox_device::adb::AdbManager;
use ltbox_device::driver::{QcomDriverMode, qcom_driver_mode};
use ltbox_device::edl::{self, EdlError, EdlSession};
use ltbox_device::fastboot::{FastbootDevice, FastbootError};

use super::{AdbIdentity, Connection, DeviceIo};
use crate::avb_info::{self, AvbSummary};
use crate::slot::Slot;

pub struct NativeDevice {
    adb: AdbManager,
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
        self.adb = AdbManager::new();
        EdlSession::open(loader, log)
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
        // Detected only so preparation can refuse it; the handle is dropped.
        match FastbootDevice::open() {
            Ok(_) => return Ok(Connection::Fastboot),
            Err(FastbootError::DeviceNotFound) => {}
            Err(error) => return Err(error.to_string()),
        }
        match self
            .adb
            .check_device_state()
            .map_err(|error| error.to_string())?
        {
            Some("device" | "recovery") => Ok(Connection::Adb),
            Some(state) => Err(crate::msg!("core_adb_state", state = state)),
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

    fn adb_serial(&mut self) -> Result<String, String> {
        self.adb
            .shell("getprop ro.serialno")
            .map(|serial| serial.trim().to_owned())
            .map_err(|error| error.to_string())
    }

    fn adb_reboot(&mut self, target: &str) -> Result<(), String> {
        let result = self.adb.reboot(target).map_err(|error| error.to_string());
        // A mode change invalidates the old USB claim even on response failure.
        self.adb = AdbManager::new();
        result
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
        if !matches!(partition, "vbmeta_system_a" | "vbmeta_system_b") {
            return Err(crate::msg!(
                "core_avb_unsupported_partition",
                partition = partition
            ));
        }
        let lun = ltbox_core::partition_lun::lun_for_partition(partition)
            .ok_or_else(|| crate::msg!("core_lun_not_found", partition = partition))?;
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
            ltbox_core::live!(
                log,
                "{}",
                crate::msg!(
                    "core_avb_parse_failed",
                    partition = partition,
                    error = error
                )
            );
        }
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        Ok(info.ok())
    }

    fn edl_active_slot(
        &mut self,
        session: &mut EdlSession,
        expected: Option<Slot>,
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
            return Err(crate::msg!("core_gpt_size_too_large"));
        }
        let path = work.path().join("gpt.bin");
        session
            .dump_partition_at("gpt_main4", &path, 4, 0, geometry.num_sectors as usize, log)
            .map_err(|e| e.to_string())?;
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if bytes.get(..header.len()) != Some(header.as_slice()) {
            return Err(crate::msg!("core_gpt_header_changed_slot"));
        }
        let gpt = crate::storage::validate_gpt(&bytes)?;
        let slot = gpt.active_slot(expected)?;
        if gpt
            .entries()
            .iter()
            .filter(|entry| matches!(entry.name.as_str(), "boot_a" | "boot_b"))
            .all(|entry| (entry.attributes >> 48) & 0xff == 0)
        {
            ltbox_core::live!(
                log,
                "{}",
                crate::msg!("core_s1e_slot_from_adb", slot = slot.suffix())
            );
        }
        Ok(slot)
    }

    fn reset_to_edl(
        &mut self,
        session: &mut EdlSession,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        session.reset_to_edl(log).map_err(|error| error.to_string())
    }
}
