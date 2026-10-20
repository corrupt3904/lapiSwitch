//! S1/S1-E/S2, without partition writes. Android ADB reports the running
//! slot and serial, then the device reboots straight to EDL; bootloader
//! fastboot is never used. Transport operations are injectable.

use std::path::Path;

use ltbox_core::live;

use crate::avb_info::{AvbSummary, KeyClass};
use crate::firmware::{FirmwareReport, MODEL};
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
    #[error(
        "기기가 연결되어 있지 않습니다. Android에서 USB 디버깅을 허용해 ADB로 연결하거나 EDL로 연결해 주세요."
    )]
    NoDevice,
    #[error(
        "Fastboot에서는 시작할 수 없습니다. 기기를 Android로 부팅하고 USB 디버깅을 허용한 뒤 ADB로 연결해 주세요."
    )]
    FastbootUnsupported,
    #[error("{step}: {message}")]
    Device { step: &'static str, message: String },
    #[error("TB520FU 모델을 확인할 수 없습니다: {0}")]
    WrongModel(String),
    #[error("ADB에서 활성 슬롯(a/b)을 읽을 수 없습니다.")]
    MissingSlot,
    #[error("ADB에서 기기 시리얼을 읽을 수 없습니다.")]
    MissingSerial,
    #[error("adb_identity.txt 저장 실패: {0}")]
    SaveIdentity(String),
}

#[derive(Debug, Clone)]
pub struct PrepareReport {
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

fn operation<T>(step: &'static str, result: Result<T, String>) -> Result<T, PrepareError> {
    result.map_err(|message| PrepareError::Device { step, message })
}

/// Shared by the GUI readiness gate and the hardware preflight.
pub fn supported_model(model: &str) -> bool {
    let model = model.trim();
    ltbox_core::model::fingerprint_model_match(&model.to_ascii_uppercase(), MODEL)
        || model.eq_ignore_ascii_case("lapis")
}

fn check_model(model: Option<&str>) -> Result<(), PrepareError> {
    let model = model.unwrap_or("").trim();
    if supported_model(model) {
        Ok(())
    } else {
        Err(PrepareError::WrongModel(model.to_owned()))
    }
}

/// The identity record binds the later TWRP session to the same device.
pub fn identity_record(slot: Slot, serial: &str) -> String {
    format!(
        "transport: adb\nmodelname: {MODEL}\nserialno: {serial}\ncurrent-slot: {}\n",
        slot.suffix()
    )
}

/// `save_identity` must durably save the ADB identity record before the EDL
/// reboot. If saving fails, the device stays in Android. The caller chooses the
/// eventual backup location; no device identifiers enter the source tree.
pub fn prepare<D: DeviceIo>(
    device: &mut D,
    firmware: &FirmwareReport,
    save_identity: &mut dyn FnMut(&str) -> Result<(), String>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    prepare_with_confirmation(device, firmware, save_identity, &mut || Ok(()), log)
}

/// The GUI must obtain an explicit answer before opening an EDL-start session.
pub fn prepare_with_confirmation<D: DeviceIo>(
    device: &mut D,
    firmware: &FirmwareReport,
    save_identity: &mut dyn FnMut(&str) -> Result<(), String>,
    confirm_edl: &mut dyn FnMut() -> Result<(), String>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    operation("USB 기기 확인", device.ensure_single_target())?;
    let started_in = operation("연결 모드 확인", device.detect())?;
    live!(log, "S1: 연결 모드 {started_in:?}");
    match started_in {
        Connection::None => Err(PrepareError::NoDevice),
        Connection::Fastboot => Err(PrepareError::FastbootUnsupported),
        Connection::Edl => {
            operation("EDL 시작 확인", confirm_edl())?;
            operation("USB 기기 확인", device.ensure_single_target())?;
            let session = operation(
                "EDL 연결 (파티션 쓰기 전)",
                device.open_edl(&firmware.loader, log),
            )?;
            probe_or_reset(device, session, None, log)
        }
        Connection::Adb => prepare_adb(device, firmware, save_identity, log),
    }
}

fn prepare_adb<D: DeviceIo>(
    device: &mut D,
    firmware: &FirmwareReport,
    save_identity: &mut dyn FnMut(&str) -> Result<(), String>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    let identity = operation("ADB 모델·슬롯 확인", device.adb_identity())?;
    check_model(identity.model.as_deref())?;
    let slot = identity.slot.ok_or(PrepareError::MissingSlot)?;
    let serial = operation("ADB 시리얼 확인", device.adb_serial())?;
    let serial = serial.trim();
    if serial.is_empty() || serial.chars().any(char::is_control) {
        return Err(PrepareError::MissingSerial);
    }
    let raw = identity_record(slot, serial);
    save_identity(&raw).map_err(PrepareError::SaveIdentity)?;
    live!(
        log,
        "S1: ADB 활성 슬롯 {}, 기기 확인 기록 저장",
        slot.suffix()
    );
    operation("USB 기기 확인", device.ensure_single_target())?;
    live!(log, "S2: ADB에서 EDL로 재부팅합니다.");
    if let Err(error) = device.adb_reboot("edl") {
        live!(log, "S2: ADB EDL 재부팅 응답: {error}. 연결을 확인합니다.");
    }
    operation("USB 기기 확인", device.ensure_single_target())?;
    operation("EDL 대기 (45초, 파티션 쓰기 전)", device.wait_edl())?;
    let session = operation(
        "EDL 연결 (파티션 쓰기 전)",
        device.open_edl(&firmware.loader, log),
    )?;
    let mut prepared = probe_or_reset(device, session, Some(slot), log)?;
    prepared.report.started_in = Connection::Adb;
    prepared.report.identity = Some(raw);
    Ok(prepared)
}

/// A failed probe returns the open session to EDL before reporting the error.
fn probe_or_reset<D: DeviceIo>(
    device: &mut D,
    mut session: D::Session,
    expected_slot: Option<Slot>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    match probe_edl(device, &mut session, expected_slot, log) {
        Ok(report) => Ok(PreparedDevice { report, session }),
        Err(error) => {
            if let Err(reset) = device.reset_to_edl(&mut session, log) {
                live!(log, "EDL 복귀 실패: {reset}");
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
    expected_slot: Option<Slot>,
    log: &mut Vec<String>,
) -> Result<PrepareReport, PrepareError> {
    let slot = operation(
        "EDL GPT 활성 슬롯 확인",
        device.edl_active_slot(session, expected_slot, log),
    )?;
    let mut vbs = [None, None];
    for (i, slot) in [Slot::A, Slot::B].into_iter().enumerate() {
        vbs[i] = operation(
            "EDL vbmeta_system 읽기",
            device.read_avb(session, &format!("vbmeta_system{}", slot.suffix()), log),
        )?;
    }
    let matches_model = vbs
        .iter()
        .flatten()
        .filter_map(AvbSummary::build_fingerprint)
        .any(|fp| ltbox_core::model::fingerprint_model_match(&fp, MODEL));
    if !matches_model {
        return Err(PrepareError::WrongModel("EDL vbmeta_system_a/b".into()));
    }
    let active = usize::from(slot == Slot::B);
    if !vbs[active]
        .as_ref()
        .and_then(AvbSummary::build_fingerprint)
        .is_some_and(|fp| ltbox_core::model::fingerprint_model_match(&fp, MODEL))
    {
        return Err(PrepareError::WrongModel(
            "EDL 활성 슬롯 vbmeta_system".into(),
        ));
    }
    live!(
        log,
        "S1-E: GPT 슬롯 속성 확인 완료. 활성 슬롯 {}",
        slot.suffix()
    );
    Ok(PrepareReport {
        started_in: Connection::Edl,
        slot,
        device_key: vbs[active].as_ref().map(AvbSummary::key_class),
        identity: None,
    })
}
