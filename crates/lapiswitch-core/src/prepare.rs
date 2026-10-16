//! S1/S1-E/S2, without partition writes. Based on LTBox's `flash/full.rs`,
//! `flash/mod.rs` and `edl_transition.rs`; transport operations are injectable.

use std::path::Path;
use std::time::Duration;

use ltbox_core::live;
use ltbox_device::fastboot::FastbootVars;

use crate::avb_info::{AvbSummary, KeyClass};
use crate::firmware::{FirmwareReport, MODEL};
use crate::rollback::{self, DeviceFloors, Downgrade};
use crate::slot::Slot;

pub mod native;

pub const FASTBOOT_TIMEOUT: Duration = Duration::from_secs(60);
pub const ROLLBACK_REJECTED_MESSAGE: &str =
    "이 펌웨어는 기기보다 롤백인덱스가 낮습니다. 롤백인덱스가 같거나 더 높은 펌웨어를 선택하세요.";

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

/// Result of waiting for Android after `fastboot reboot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    Adb,
    Edl,
}

/// No flash/erase API is available during preparation. Only a missing
/// partition or invalid AVB may yield `Ok(None)` from `read_avb`; transport
/// and disk errors must yield `Err` so they cannot lower a rollback estimate.
pub trait DeviceIo {
    type Session;

    fn ensure_single_target(&mut self) -> Result<(), String>;
    fn detect(&mut self) -> Result<Connection, String>;
    fn adb_identity(&mut self) -> Result<AdbIdentity, String>;
    fn adb_reboot(&mut self, target: &str) -> Result<(), String>;
    fn wait_fastboot(&mut self, timeout: Duration) -> Result<(), String>;
    fn fastboot_vars(&mut self) -> Result<FastbootVars, String>;
    fn reboot_system(&mut self) -> Result<(), String>;
    fn wait_adb_or_edl(&mut self) -> Result<Arrival, String>;
    fn wait_edl(&mut self) -> Result<(), String>;
    fn open_edl(&mut self, loader: &Path, log: &mut Vec<String>) -> Result<Self::Session, String>;
    fn read_avb(
        &mut self,
        session: &mut Self::Session,
        partition: &str,
        log: &mut Vec<String>,
    ) -> Result<Option<AvbSummary>, String>;
    fn edl_active_slot(
        &mut self,
        session: &mut Self::Session,
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
    #[error("기기가 연결되어 있지 않습니다. ADB, fastboot 또는 EDL로 연결해 주세요.")]
    NoDevice,
    #[error("{step}: {message}")]
    Device { step: &'static str, message: String },
    #[error("TB520FU 모델을 확인할 수 없습니다: {0}")]
    WrongModel(String),
    #[error("fastboot 활성 슬롯(a/b)을 읽을 수 없습니다.")]
    MissingSlot,
    #[error("ADB 슬롯({adb:?})과 fastboot 슬롯({fastboot:?})이 다릅니다.")]
    SlotMismatch { adb: Slot, fastboot: Slot },
    #[error("fastboot 롤백 기준값을 읽을 수 없습니다. getvar 결과와 드라이버를 확인해 주세요.")]
    MissingRollback,
    #[error("getvar.txt 저장 실패: {0}")]
    SaveGetvar(String),
    #[error("EDL에서 boot/vbmeta_system AVB 정보를 읽을 수 없습니다.")]
    InvalidEdlAvb,
    #[error("{} {details}", ROLLBACK_REJECTED_MESSAGE)]
    Downgrade {
        images: Vec<Downgrade>,
        details: String,
    },
}

#[derive(Debug, Clone)]
pub struct PrepareReport {
    pub started_in: Connection,
    pub slot: Slot,
    pub slot_estimated: bool,
    pub floors: DeviceFloors,
    pub device_key: Option<KeyClass>,
    /// None on EDL start; otherwise saved by the caller before S2 begins.
    pub getvar_all: Option<String>,
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

pub(crate) fn check_rollback(
    firmware: &FirmwareReport,
    floors: DeviceFloors,
    log: &mut Vec<String>,
) -> Result<(), PrepareError> {
    let device = match floors {
        DeviceFloors::PerLocation(indices) => Some(indices),
        DeviceFloors::Aggregate(index) => Some(rollback::RollbackIndices {
            boot: index,
            vbmeta_system: index,
        }),
        DeviceFloors::None => None,
    };
    for (partition, firmware_index, device_index) in [
        (
            "boot",
            firmware.indices.boot,
            device.map(|indices| indices.boot),
        ),
        (
            "vbmeta_system",
            firmware.indices.vbmeta_system,
            device.map(|indices| indices.vbmeta_system),
        ),
    ] {
        let floor = device_index
            .map(|index| index.to_string())
            .unwrap_or_else(|| "없음 (0/1)".into());
        live!(
            log,
            "롤백 비교 {partition}: 펌웨어 {firmware_index}, 기기 {floor}"
        );
    }
    rollback::check(firmware.indices, floors).map_err(|images| {
        let details = images
            .iter()
            .map(|image| {
                format!(
                    "{}: 펌웨어 {} < 기기 {}",
                    image.partition, image.firmware, image.floor
                )
            })
            .collect::<Vec<_>>()
            .join(" / ");
        PrepareError::Downgrade { images, details }
    })
}

/// `save_getvar` must durably save the raw dump (including on a rejected
/// downgrade). If saving fails, do not reboot into EDL. The caller chooses the
/// eventual backup location; no device identifiers enter the source tree.
pub fn prepare<D: DeviceIo>(
    device: &mut D,
    firmware: &FirmwareReport,
    save_getvar: &mut dyn FnMut(&str) -> Result<(), String>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    prepare_with_mode(
        device,
        firmware,
        save_getvar,
        crate::deviceinfo::RollbackMode::Keep,
        log,
    )
}

pub fn prepare_with_mode<D: DeviceIo>(
    device: &mut D,
    firmware: &FirmwareReport,
    save_getvar: &mut dyn FnMut(&str) -> Result<(), String>,
    rollback_mode: crate::deviceinfo::RollbackMode,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    prepare_with_confirmation(
        device,
        firmware,
        save_getvar,
        rollback_mode,
        &mut || Ok(()),
        log,
    )
}

/// The GUI must obtain an explicit answer before opening an EDL-start session.
pub fn prepare_with_confirmation<D: DeviceIo>(
    device: &mut D,
    firmware: &FirmwareReport,
    save_getvar: &mut dyn FnMut(&str) -> Result<(), String>,
    rollback_mode: crate::deviceinfo::RollbackMode,
    confirm_edl: &mut dyn FnMut() -> Result<(), String>,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    operation("USB 기기 확인", device.ensure_single_target())?;
    let started_in = operation("연결 모드 확인", device.detect())?;
    live!(log, "S1: 연결 모드 {started_in:?}");
    match started_in {
        Connection::None => Err(PrepareError::NoDevice),
        Connection::Edl => {
            operation("EDL 시작 확인", confirm_edl())?;
            operation("USB 기기 확인", device.ensure_single_target())?;
            prepare_edl(device, firmware, rollback_mode, log)
        }
        Connection::Adb | Connection::Fastboot => {
            let report = match prepare_fastboot(
                device,
                firmware,
                started_in,
                save_getvar,
                rollback_mode,
                log,
            ) {
                Ok(report) => report,
                Err(error) => {
                    // Preserve the actual error even if this best-effort reset fails.
                    // Do not reboot a newly ambiguous USB target.
                    if device.ensure_single_target().is_ok()
                        && let Err(reset) = device.reboot_system()
                    {
                        live!(log, "시스템 재부팅 실패: {reset}");
                    }
                    return Err(error);
                }
            };
            transition_to_edl(device, log)?;
            let session = operation(
                "EDL 연결 (파티션 쓰기 전)",
                device.open_edl(&firmware.loader, log),
            )?;
            Ok(PreparedDevice { report, session })
        }
    }
}

fn prepare_fastboot<D: DeviceIo>(
    device: &mut D,
    firmware: &FirmwareReport,
    started_in: Connection,
    save_getvar: &mut dyn FnMut(&str) -> Result<(), String>,
    rollback_mode: crate::deviceinfo::RollbackMode,
    log: &mut Vec<String>,
) -> Result<PrepareReport, PrepareError> {
    let adb_slot = if started_in == Connection::Adb {
        let identity = operation("ADB 모델 확인", device.adb_identity())?;
        check_model(identity.model.as_deref())?;
        operation("USB 기기 확인", device.ensure_single_target())?;
        if let Err(error) = device.adb_reboot("bootloader") {
            live!(
                log,
                "bootloader 재부팅 응답 오류, 연결 확인을 계속합니다: {error}"
            );
        }
        operation(
            "fastboot 대기 (60초)",
            device.wait_fastboot(FASTBOOT_TIMEOUT).map_err(|error| format!(
                "{error} 만약 기기가 Bootloader 모드라면 수동으로 재부팅한 뒤 다시 시도해 주세요."
            )),
        )?;
        identity.slot
    } else {
        None
    };
    operation("USB 기기 확인", device.ensure_single_target())?;
    let vars = operation("fastboot 정보 읽기", device.fastboot_vars())?;
    if !vars.raw_getvar_all.trim().is_empty() {
        save_getvar(&vars.raw_getvar_all).map_err(PrepareError::SaveGetvar)?;
    }
    // LTBox get_all_vars is best-effort and can return Ok with empty fields.
    // Keep mode requires a readable floor. Reset mode does not rely on it,
    // but still requires the raw response, verified model and active slot.
    if vars.raw_getvar_all.trim().is_empty()
        || (rollback_mode == crate::deviceinfo::RollbackMode::Keep
            && vars.rollback_indices.is_empty())
    {
        return Err(PrepareError::MissingRollback);
    }
    check_model(vars.model.as_deref())?;
    let slot = vars
        .current_slot
        .as_deref()
        .and_then(Slot::parse)
        .ok_or(PrepareError::MissingSlot)?;
    if let Some(adb) = adb_slot
        && adb != slot
    {
        return Err(PrepareError::SlotMismatch {
            adb,
            fastboot: slot,
        });
    }
    let floors = rollback::floors_from_fastboot(&vars.rollback_indices);
    live!(
        log,
        "S1: 활성 슬롯 {}, 롤백 기준값 {floors:?}",
        slot.suffix()
    );
    if rollback_mode == crate::deviceinfo::RollbackMode::Keep {
        check_rollback(firmware, floors, log)?;
    } else {
        live!(
            log,
            "롤백 초기화 선택: 펌웨어의 낮은 index를 허용합니다. TWRP에서 32개 index를 0으로 초기화·검증해야 플래시할 수 있습니다."
        );
    }
    live!(log, "S1: 롤백 정책 적용 완료 ({rollback_mode:?})");
    Ok(PrepareReport {
        started_in,
        slot,
        slot_estimated: false,
        floors,
        device_key: None,
        getvar_all: Some(vars.raw_getvar_all),
    })
}

fn transition_to_edl<D: DeviceIo>(
    device: &mut D,
    log: &mut Vec<String>,
) -> Result<(), PrepareError> {
    operation("USB 기기 확인", device.ensure_single_target())?;
    live!(
        log,
        "S2: 시스템으로 재부팅한 뒤 ADB와 EDL 연결을 감시합니다."
    );
    let arrival = match device.reboot_system() {
        Ok(()) => device.wait_adb_or_edl(),
        Err(error) => Err(error),
    };
    match arrival {
        Ok(Arrival::Edl) => live!(log, "S2: EDL이 먼저 감지되었습니다."),
        Ok(Arrival::Adb) => {
            operation("USB 기기 확인", device.ensure_single_target())?;
            if let Err(error) = device.adb_reboot("edl") {
                live!(
                    log,
                    "S2: ADB EDL 전환 실패: {error}. 기기를 직접 EDL로 전환해 주세요."
                );
            }
        }
        Err(error) => live!(log, "S2: {error}. 기기를 직접 EDL로 전환해 주세요."),
    }
    operation("USB 기기 확인", device.ensure_single_target())?;
    operation("EDL 대기 (45초, 파티션 쓰기 전)", device.wait_edl())
}

fn prepare_edl<D: DeviceIo>(
    device: &mut D,
    firmware: &FirmwareReport,
    rollback_mode: crate::deviceinfo::RollbackMode,
    log: &mut Vec<String>,
) -> Result<PreparedDevice<D::Session>, PrepareError> {
    let mut session = operation(
        "EDL 연결 (파티션 쓰기 전)",
        device.open_edl(&firmware.loader, log),
    )?;
    let result = probe_edl(device, &mut session, firmware, rollback_mode, log);
    match result {
        Ok(report) => Ok(PreparedDevice { report, session }),
        Err(error) => {
            if let Err(reset) = device.reset_to_edl(&mut session, log) {
                live!(log, "EDL 복귀 실패: {reset}");
            }
            Err(error)
        }
    }
}

fn probe_edl<D: DeviceIo>(
    device: &mut D,
    session: &mut D::Session,
    firmware: &FirmwareReport,
    rollback_mode: crate::deviceinfo::RollbackMode,
    log: &mut Vec<String>,
) -> Result<PrepareReport, PrepareError> {
    let slot = operation(
        "EDL GPT 활성 슬롯 확인",
        device.edl_active_slot(session, log),
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
    let mut boot = [None, None];
    for (i, slot) in [Slot::A, Slot::B].into_iter().enumerate() {
        boot[i] = operation(
            "EDL boot 읽기",
            device.read_avb(session, &format!("boot{}", slot.suffix()), log),
        )?
        .map(|info| info.rollback_index);
    }
    let indices = [
        vbs[0].as_ref().map(|info| info.rollback_index),
        vbs[1].as_ref().map(|info| info.rollback_index),
    ];
    let floors = DeviceFloors::PerLocation(
        rollback::floors_from_edl(boot, indices).ok_or(PrepareError::InvalidEdlAvb)?,
    );
    if rollback_mode == crate::deviceinfo::RollbackMode::Keep {
        check_rollback(firmware, floors, log)?;
    } else {
        live!(
            log,
            "롤백 초기화 선택: 펌웨어의 낮은 index를 허용합니다. TWRP에서 32개 index를 0으로 초기화·검증해야 플래시할 수 있습니다."
        );
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
        "S1-E: GPT 슬롯 속성 확인 완료. 활성 슬롯 {} (롤백 크기와 무관).",
        slot.suffix()
    );
    Ok(PrepareReport {
        started_in: Connection::Edl,
        slot,
        slot_estimated: false,
        floors,
        device_key: vbs[active].as_ref().map(AvbSummary::key_class),
        getvar_all: None,
    })
}
