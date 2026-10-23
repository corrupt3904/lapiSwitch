//! S1/S1-E/S2, without partition writes. Android ADB reports the running
//! slot and serial, then the device reboots straight to EDL; bootloader
//! fastboot is never used. Transport operations are injectable.

use std::path::Path;

use ltbox_core::live;

use crate::avb_info::{AvbSummary, KeyClass};
use crate::device::Device;
use crate::slot::Slot;

pub mod native;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    None,
    Adb,
    Fastboot,
    Edl,
}

#[derive(Debug, Clone)]
pub struct AdbIdentity {
    pub model: Option<String>,
    pub slot: Option<Slot>,
}

/// No flash/erase API is available during preparation. Only a missing
/// partition or invalid AVB may yield `Ok(None)` from `read_avb`; transport
/// and disk errors must yield `Err` so they cannot pass as a missing image.
/// Rollback indices are not read here: Keep mode compares them in TWRP.
pub trait DeviceIo {
    type Session;

    fn ensure_single_target(&mut self) -> Result<(), String>;
    fn detect(&mut self) -> Result<Connection, String>;
    fn adb_identity(&mut self) -> Result<AdbIdentity, String>;
    fn adb_serial(&mut self) -> Result<String, String>;
    fn adb_reboot(&mut self, target: &str) -> Result<(), String>;
    fn wait_edl(&mut self) -> Result<(), String>;
    fn open_edl(&mut self, loader: &Path, log: &mut Vec<String>) -> Result<Self::Session, String>;
    fn read_avb(
        &mut self,
        session: &mut Self::Session,
        partition: &str,
        log: &mut Vec<String>,
    ) -> Result<Option<AvbSummary>, String>;
    /// GPT slot-control flags, checked against the ADB slot when one exists.
    /// Blank flags are accepted only with an ADB slot, which is then used.
    fn edl_active_slot(
        &mut self,
        session: &mut Self::Session,
        expected: Option<Slot>,
        log: &mut Vec<String>,
    ) -> Result<Slot, String>;
    fn reset_to_edl(
        &mut self,
        session: &mut Self::Session,
        log: &mut Vec<String>,
    ) -> Result<(), String>;
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    #[error("{}", crate::msg!("core_prepare_no_device"))]
    NoDevice,
    #[error("{}", crate::msg!("core_prepare_fastboot"))]
    FastbootUnsupported,
    #[error("{step}: {message}")]
    Device { step: String, message: String },
    #[error("{}", crate::msg!("core_prepare_wrong_model", model = .0))]
    WrongModel(String),
    #[error("{}", crate::msg!("core_prepare_other_device", found = .found, expected = .expected))]
    OtherDevice { expected: Device, found: Device },
    #[error("{}", crate::msg!("core_prepare_missing_slot"))]
    MissingSlot,
    #[error("{}", crate::msg!("core_prepare_missing_serial"))]
    MissingSerial,
    #[error("{}", crate::msg!("core_prepare_identity_save", error = .0))]
    SaveIdentity(String),
}

#[derive(Debug, Clone)]
pub struct PrepareReport {
    /// From the ADB model, or the active slot's fingerprint on EDL start.
    pub device: Device,
    pub started_in: Connection,
    pub slot: Slot,
    pub device_key: Option<KeyClass>,
    /// None on EDL start; otherwise the ADB identity record saved before S2.
    pub identity: Option<String>,
}

/// Retains the open Firehose session for S3, avoiding a second loader upload.
pub struct PreparedDevice<S> {
    pub report: PrepareReport,
    pub session: S,
}

fn operation<T>(step: String, result: Result<T, String>) -> Result<T, PrepareError> {
    result.map_err(|message| PrepareError::Device { step, message })
}

/// Shared by the GUI readiness gate and the hardware preflight.
pub fn supported_model(model: &str) -> bool {
    Device::identify(model).is_some()
}

/// A run for a known device only continues on that device.
fn check_expected(expected: Option<Device>, found: Device) -> Result<Device, PrepareError> {
    match expected {
        Some(expected) if expected != found => Err(PrepareError::OtherDevice { expected, found }),
        _ => Ok(found),
    }
}

/// The identity record binds the later TWRP session to the same device.
pub fn identity_record(device: Device, slot: Slot, serial: &str) -> String {
    format!(
        "transport: adb\nmodelname: {device}\nserialno: {serial}\ncurrent-slot: {}\n",
        slot.suffix()
    )
}

/// `save_identity` must durably save the ADB identity record before the EDL
/// reboot. If saving fails, the device stays in Android. The caller chooses the
/// eventual backup location; no device identifiers enter the source tree.
pub fn prepare<D: DeviceIo>(
    device: &mut D,
    expected: Option<Device>,
    loader: &Path,
    save_identity: &mut dyn FnMut(&str) -> Result<(), String>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    prepare_with_confirmation(device, expected, loader, save_identity, &mut || Ok(()), log)
}

/// The GUI must obtain an explicit answer before opening an EDL-start session.
/// `expected` is the device a conversion's firmware names; an edit learns
/// the device here.
pub fn prepare_with_confirmation<D: DeviceIo>(
    device: &mut D,
    expected: Option<Device>,
    loader: &Path,
    save_identity: &mut dyn FnMut(&str) -> Result<(), String>,
    confirm_edl: &mut dyn FnMut() -> Result<(), String>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    operation(
        crate::msg!("core_step_usb_check"),
        device.ensure_single_target(),
    )?;
    let started_in = operation(crate::msg!("core_step_mode_check"), device.detect())?;
    live!(
        log,
        "{}",
        crate::msg!("core_s1_mode", started_in = format!("{:?}", started_in))
    );
    match started_in {
        Connection::None => Err(PrepareError::NoDevice),
        Connection::Fastboot => Err(PrepareError::FastbootUnsupported),
        Connection::Edl => {
            operation(crate::msg!("core_step_edl_start"), confirm_edl())?;
            operation(
                crate::msg!("core_step_usb_check"),
                device.ensure_single_target(),
            )?;
            let session = operation(
                crate::msg!("core_step_edl_connect"),
                device.open_edl(loader, log),
            )?;
            probe_or_reset(device, session, expected, None, log)
        }
        Connection::Adb => prepare_adb(device, expected, loader, save_identity, log),
    }
}

fn prepare_adb<D: DeviceIo>(
    device: &mut D,
    expected: Option<Device>,
    loader: &Path,
    save_identity: &mut dyn FnMut(&str) -> Result<(), String>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    let identity = operation(crate::msg!("core_step_adb_identity"), device.adb_identity())?;
    let model = identity.model.as_deref().unwrap_or("").trim();
    let found =
        Device::identify(model).ok_or_else(|| PrepareError::WrongModel(model.to_owned()))?;
    let found = check_expected(expected, found)?;
    let slot = identity.slot.ok_or(PrepareError::MissingSlot)?;
    let serial = operation(crate::msg!("core_step_adb_serial"), device.adb_serial())?;
    let serial = serial.trim();
    if serial.is_empty() || serial.chars().any(char::is_control) {
        return Err(PrepareError::MissingSerial);
    }
    let raw = identity_record(found, slot, serial);
    save_identity(&raw).map_err(PrepareError::SaveIdentity)?;
    live!(
        log,
        "{}",
        crate::msg!("core_s1_adb_slot", slot = slot.suffix())
    );
    operation(
        crate::msg!("core_step_usb_check"),
        device.ensure_single_target(),
    )?;
    live!(log, "{}", crate::msg!("core_s2_reboot_edl"));
    if let Err(error) = device.adb_reboot("edl") {
        live!(
            log,
            "{}",
            crate::msg!("core_s2_reboot_response", error = error)
        );
    }
    operation(
        crate::msg!("core_step_usb_check"),
        device.ensure_single_target(),
    )?;
    operation(crate::msg!("core_step_edl_wait"), device.wait_edl())?;
    let session = operation(
        crate::msg!("core_step_edl_connect"),
        device.open_edl(loader, log),
    )?;
    let mut prepared = probe_or_reset(device, session, Some(found), Some(slot), log)?;
    prepared.report.started_in = Connection::Adb;
    prepared.report.identity = Some(raw);
    Ok(prepared)
}

/// A failed probe returns the open session to EDL before reporting the error.
fn probe_or_reset<D: DeviceIo>(
    device: &mut D,
    mut session: D::Session,
    expected: Option<Device>,
    expected_slot: Option<Slot>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    match probe_edl(device, &mut session, expected, expected_slot, log) {
        Ok(report) => Ok(PreparedDevice { report, session }),
        Err(error) => {
            if let Err(reset) = device.reset_to_edl(&mut session, log) {
                live!(
                    log,
                    "{}",
                    crate::msg!("core_edl_reset_failed", reset = reset)
                );
            }
            Err(error)
        }
    }
}

/// Slot and model only. Rollback indices are compared later in TWRP from the
/// values actually stored in DeviceInfo, never estimated from AVB images.
fn probe_edl<D: DeviceIo>(
    device: &mut D,
    session: &mut D::Session,
    expected: Option<Device>,
    expected_slot: Option<Slot>,
    log: &mut Vec<String>,
) -> Result<PrepareReport, PrepareError> {
    let slot = operation(
        crate::msg!("core_step_edl_slot"),
        device.edl_active_slot(session, expected_slot, log),
    )?;
    let mut vbs = [None, None];
    for (i, slot) in [Slot::A, Slot::B].into_iter().enumerate() {
        vbs[i] = operation(
            crate::msg!("core_step_edl_vbmeta"),
            device.read_avb(session, &format!("vbmeta_system{}", slot.suffix()), log),
        )?;
    }
    // The active slot's fingerprint names the device; on ADB start it must
    // be the device ADB reported.
    let active = usize::from(slot == Slot::B);
    let found = vbs[active]
        .as_ref()
        .and_then(AvbSummary::build_fingerprint)
        .and_then(|fp| Device::identify(&fp))
        .ok_or_else(|| PrepareError::WrongModel(crate::msg!("core_edl_active_vbmeta")))?;
    let found = check_expected(expected, found)?;
    live!(
        log,
        "{}",
        crate::msg!("core_s1e_slot", slot = slot.suffix())
    );
    Ok(PrepareReport {
        device: found,
        started_in: Connection::Edl,
        slot,
        device_key: vbs[active].as_ref().map(AvbSummary::key_class),
        identity: None,
    })
}
