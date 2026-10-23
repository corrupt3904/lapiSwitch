//! EDL session ownership across TWRP boot, reconnection and firmware writes.

use std::path::Path;
use std::time::{Duration, Instant};

use ltbox_device::driver::{QcomDriverMode, qcom_driver_mode};
use ltbox_device::edl::{self, EdlError};

use super::{Clock, FinishIo};
use crate::avb_info::AvbSummary;
use crate::firmware::FirmwareReport;
use crate::prepare::{DeviceIo, native::NativeDevice};
use crate::stage1::{ResetMode, StageIo, native::NativeStage};
use crate::storage::Partition;

fn session_or_wait<T>(
    result: Result<T, EdlError>,
    log: &mut Vec<String>,
) -> Result<Option<T>, String> {
    match result {
        Ok(session) => Ok(Some(session)),
        Err(error @ (EdlError::PortNotFound | EdlError::PortTimeout(_))) => {
            ltbox_core::live!(
                log,
                "{}",
                crate::msg!("core_edl_port_unstable", error = error)
            );
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
}

pub struct NativeWorkflow {
    stage: Option<NativeStage>,
    device: Option<NativeDevice>,
}

impl NativeWorkflow {
    pub fn from_stage(stage: NativeStage) -> Self {
        Self {
            stage: Some(stage),
            device: None,
        }
    }
    pub fn disconnected(device: NativeDevice) -> Self {
        Self {
            stage: None,
            device: Some(device),
        }
    }
    pub(crate) fn stage(&mut self) -> Result<&mut NativeStage, String> {
        self.stage
            .as_mut()
            .ok_or_else(|| crate::msg!("core_edl_no_session"))
    }
}

impl StageIo for NativeWorkflow {
    fn ensure_single_target(&mut self) -> Result<(), String> {
        ltbox_device::selection::ensure_single_usb_target().map_err(|error| error.to_string())
    }
    fn read_avb(
        &mut self,
        name: &str,
        log: &mut Vec<String>,
    ) -> Result<Option<AvbSummary>, String> {
        self.stage()?.read_avb(name, log)
    }
    fn scan(&mut self, log: &mut Vec<String>) -> Result<Vec<Partition>, String> {
        self.stage()?.scan(log)
    }
    fn validate_snapshot(&mut self, snapshot: &[Partition]) -> Result<(), String> {
        self.stage()?.validate_snapshot(snapshot)
    }
    fn confirm_proinfo_absent(&mut self, log: &mut Vec<String>) -> Result<(), String> {
        self.stage()?.confirm_proinfo_absent(log)
    }
    fn dump_at(
        &mut self,
        part: &Partition,
        path: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        self.stage()?.dump_at(part, path, log)
    }
    fn write_at(
        &mut self,
        part: &Partition,
        path: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        self.stage()?.write_at(part, path, log)
    }
    fn reset(&mut self, mode: ResetMode, log: &mut Vec<String>) -> Result<(), String> {
        self.stage()?.reset(mode, log)
    }
}

impl FinishIo for NativeWorkflow {
    fn probe_edl(&mut self) -> Result<Option<String>, String> {
        match match qcom_driver_mode() {
            QcomDriverMode::Userspace => edl::find_edl_device(),
            QcomDriverMode::Kernel => edl::find_edl_port(),
        } {
            Ok(port) => Ok(Some(port)),
            Err(EdlError::PortNotFound) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    fn reboot_recovery(&mut self, log: &mut Vec<String>) -> Result<(), String> {
        let stage = self
            .stage
            .take()
            .ok_or(crate::msg!("core_edl_no_session_recovery"))?;
        let (device, mut session) = stage.into_parts();
        self.device = Some(device);
        let result = session.reset(log).map_err(|error| error.to_string());
        drop(session);
        result
    }
    fn open_edl(&mut self, loader: &Path, log: &mut Vec<String>) -> Result<(), String> {
        if self.stage.is_some() {
            return Err(crate::msg!("core_edl_session_open"));
        }
        let device = self
            .device
            .as_mut()
            .ok_or(crate::msg!("core_no_device_adapter"))?;
        let session = device.open_edl(loader, log)?;
        self.stage = Some(NativeStage::new(
            self.device.take().expect("device exists"),
            session,
        ));
        Ok(())
    }
    fn try_open_edl(&mut self, loader: &Path, log: &mut Vec<String>) -> Result<bool, String> {
        if self.stage.is_some() {
            return Err(crate::msg!("core_edl_session_open"));
        }
        let device = self
            .device
            .as_mut()
            .ok_or(crate::msg!("core_no_device_adapter"))?;
        let Some(session) = session_or_wait(device.open_edl_session(loader, log), log)? else {
            return Ok(false);
        };
        self.stage = Some(NativeStage::new(
            self.device.take().expect("device exists"),
            session,
        ));
        Ok(true)
    }
    fn flash_firmware(
        &mut self,
        firmware: &FirmwareReport,
        wipe: bool,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        // Repeat LTBox selection and our case-insensitive exclusion before
        // using the already sealed XMLs. No XML edits or region overlays.
        let (raw, patch) = edl::collect_firmware_xmls_for_flash(&firmware.image_dir, false)
            .map_err(|error| error.to_string())?;
        if raw != firmware.raw_xmls || patch != firmware.patch_xmls {
            return Err(crate::msg!("core_firmware_xml_changed"));
        }
        for path in &raw {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if name.contains("blank_gpt") || name.contains("wipe_partitions") {
                return Err(crate::msg!("core_firmware_excluded_selected"));
            }
        }
        self.stage()?
            .session_mut()
            .flash_rawprogram_with_wipe(&raw, &patch, wipe, log)
            .map_err(|error| error.to_string())
    }
    fn set_active_a(&mut self, log: &mut Vec<String>) -> Result<(), String> {
        self.stage()?
            .session_mut()
            .set_active_slot_a(log)
            .map_err(|error| error.to_string())
    }
    fn final_reboot(&mut self, log: &mut Vec<String>) -> Option<String> {
        // reset() exposes the error so it can be retained as a warning while
        // preserving the same best-effort semantics as reset_tolerant().
        self.stage()
            .and_then(|stage| {
                stage
                    .session_mut()
                    .reset(log)
                    .map_err(|error| error.to_string())
            })
            .err()
    }
}

pub struct SystemClock {
    started: Instant,
}
impl Default for SystemClock {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}
impl Clock for SystemClock {
    fn seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
    fn sleep_one_second(&mut self) {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_typed_port_disappearance_and_stabilization_timeouts_allow_waiting_again() {
        let mut log = vec![];
        for error in [
            EdlError::PortNotFound,
            EdlError::PortTimeout(Duration::from_secs(45)),
        ] {
            assert!(
                session_or_wait::<()>(Err(error), &mut log)
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(log.len(), 2);
        assert_eq!(session_or_wait(Ok(7u8), &mut log).unwrap(), Some(7));
        for error in [
            EdlError::MultipleDevices,
            EdlError::Serial("USB access denied".into()),
            EdlError::Session("Sahara/Firehose failed".into()),
            // A misleading error string is never a substitute for its type.
            EdlError::Session("Timed out after 45s waiting for a stable EDL port".into()),
        ] {
            assert!(session_or_wait::<()>(Err(error), &mut log).is_err());
        }
        assert_eq!(log.len(), 2);
    }
}
