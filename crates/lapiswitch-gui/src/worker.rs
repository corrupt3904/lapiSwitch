//! Blocking TWRP/device work sends owned progress and confirmation events.
use crate::progress::Step;
pub(crate) use crate::wait_control::{WaitControl, WaitTicket};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use lapiswitch_core::backup::{self, BackupBuilder, VerifiedBackup};
use lapiswitch_core::firmware::{FirmwareReport, check_firmware};
use lapiswitch_core::prepare::{Connection, DeviceIo, native::NativeDevice};
use lapiswitch_core::region::Region;
use lapiswitch_core::stage1::{self, native::NativeStage};
use lapiswitch_core::state::{FlashOptions, ResumeAction, RunState, StateStore};
use lapiswitch_core::workflow::native::{NativeWorkflow, SystemClock};

#[derive(Debug, Clone)]
pub struct Pending {
    pub state: RunState,
    pub action: ResumeAction,
}

pub fn pending(path: &Path) -> Result<Option<Pending>, String> {
    let Some(state) = StateStore::new(path.to_owned()).load()? else {
        return Ok(None);
    };
    // Inspect the independent write journal even when payload verification
    // fails. A damaged backup must never turn into permission to start over.
    let mut action = state.resume_action(backup::transaction_stage_in(&state.backup_dir)?);
    if state.recovery.is_none() && action != ResumeAction::Restart {
        action = ResumeAction::Recovery;
    }
    Ok((action != ResumeAction::Restart).then_some(Pending { state, action }))
}

#[derive(Debug, Clone)]
pub struct Request {
    pub state_path: PathBuf,
    pub log_dir: PathBuf,
    pub mask_serial: bool,
    pub wait_control: WaitControl,
    pub action: Action,
}
#[derive(Debug, Clone)]
pub enum Action {
    Full {
        approved: Box<FirmwareReport>,
        options: FlashOptions,
        output: PathBuf,
    },
    Resume,
    Restore {
        backup: PathBuf,
        loader: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub enum Event {
    Validated(Result<FirmwareReport, String>),
    FirmwareRegionMismatch { firmware: Region, target: Region },
    ConfirmEdlStart(Sender<bool>),
    BackupChecked(Result<String, String>),
    Device(u64, Result<crate::device::Snapshot, String>),
    Drivers(Result<ltbox_device::driver::DriverStatus, String>),
    DriverInstalled(Result<bool, String>),
    Log(String),
    Transfer(Option<String>),
    LogPath(PathBuf),
    Backup(PathBuf),
    Progress(Step),
    RecoveryPhase(lapiswitch_core::recovery_flow::Phase),
    ConfirmRecoveryReboot(lapiswitch_core::recovery_flow::Transition, Sender<()>),
    RecoveryStatus(String),
    RecoveryWait(Option<WaitTicket>),
    Finished(Result<Vec<String>, String>),
}

pub fn validate(tx: Sender<Event>, folder: PathBuf, region: Region) {
    std::thread::spawn(move || {
        let event = match std::panic::catch_unwind(|| check_firmware(&folder, region)) {
            Ok(Err(lapiswitch_core::firmware::FirmwareError::WrongRegion { firmware, target })) => {
                Event::FirmwareRegionMismatch { firmware, target }
            }
            Ok(result) => Event::Validated(result.map_err(|e| e.to_string())),
            Err(_) => {
                Event::Validated(Err("펌웨어 검사 작업이 예기치 않게 종료되었습니다.".into()))
            }
        };
        let _ = tx.send(event);
    });
}

pub fn check_backup(tx: Sender<Event>, folder: PathBuf) {
    std::thread::spawn(move || {
        let result = VerifiedBackup::open_recovery(&folder).and_then(|b| {
            let assessment = lapiswitch_core::recovery_flow::restore::preflight(&b)?;
            Ok(format!(
                "{} · slot {} · {} · {} files",
                b.manifest().model,
                b.manifest().slot.suffix(),
                b.manifest().target_region.token(),
                b.manifest().files.len()
            ) + &format!(" · DeviceInfo: {}", assessment.description()))
        });
        let _ = tx.send(Event::BackupChecked(result));
    });
}

pub fn drivers(tx: Sender<Event>) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(ltbox_device::driver::check_required_drivers)
            .map_err(|_| "드라이버 확인 작업이 예기치 않게 종료되었습니다.".into());
        let _ = tx.send(Event::Drivers(result));
    });
}

pub fn device(tx: Sender<Event>, id: u64, passive: bool) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| crate::device::probe(passive))
            .unwrap_or_else(|_| Err("기기 연결 확인 작업이 예기치 않게 종료되었습니다.".into()));
        // The probe has returned and dropped every USB handle before replay.
        let _ = tx.send(Event::Device(id, result));
    });
}

/// Uses LTBox's signed installer / polkit path. No installer runs until the
/// user presses the install button. `false` means elevation was cancelled.
pub fn install_drivers(tx: Sender<Event>, storage_root: PathBuf, mask: bool) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let transcript = crate::transcript::Transcript::create(
                &crate::transcript::log_dir(&storage_root),
                mask,
            )?;
            let _ = tx.send(Event::LogPath(transcript.path().to_owned()));
            let pump = crate::transcript::Pump::start(transcript, tx.clone());
            let mut log = vec![];
            let result = match ltbox_device::driver::download_and_install(&mut log) {
                Ok(()) => Ok(true),
                Err(ltbox_device::driver::DriverError::InstallCancelled) => Ok(false),
                Err(error) => Err(error.to_string()),
            };
            pump.finish()?;
            result
        }))
        .unwrap_or_else(|_| Err("드라이버 설치 작업이 예기치 않게 종료되었습니다.".into()));
        let _ = tx.send(Event::DriverInstalled(result));
        // Even a successful process exit is not proof the driver is present.
        drivers(tx);
    });
}

pub struct ChannelUi {
    pub tx: Sender<Event>,
    wait_control: WaitControl,
    phase: lapiswitch_core::recovery_flow::Phase,
}
impl ChannelUi {
    fn confirm_edl_start(&mut self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(Event::ConfirmEdlStart(tx))
            .map_err(|_| "EDL 시작 안내를 표시할 수 없습니다.")?;
        match rx.recv() {
            Ok(true) => Ok(()),
            _ => Err("EDL에서 시작하는 작업을 취소했습니다. 기기에는 쓰지 않았습니다.".into()),
        }
    }
    fn new(tx: Sender<Event>, wait_control: WaitControl) -> Self {
        Self {
            tx,
            wait_control,
            phase: lapiswitch_core::recovery_flow::Phase::Preflight,
        }
    }
}
impl lapiswitch_core::recovery_flow::Ui for ChannelUi {
    fn phase(&mut self, phase: lapiswitch_core::recovery_flow::Phase) {
        self.wait_control.finish_wait();
        self.phase = phase;
        let _ = self.tx.send(Event::RecoveryWait(None));
        let _ = self.tx.send(Event::RecoveryPhase(phase));
    }
    fn confirm(
        &mut self,
        transition: lapiswitch_core::recovery_flow::Transition,
    ) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(Event::ConfirmRecoveryReboot(transition, tx))
            .map_err(|_| "안내를 표시할 수 없습니다.")?;
        rx.recv()
            .map_err(|_| "재부팅 확인이 취소되어 현재 단계를 보존했습니다.".into())
    }
    fn status(&mut self, text: &str, waiting: bool) {
        let ticket = if waiting {
            self.wait_control.begin(self.phase)
        } else {
            self.wait_control.finish_wait();
            None
        };
        let _ = self.tx.send(Event::RecoveryWait(ticket));
        let _ = self.tx.send(Event::RecoveryStatus(text.into()));
    }
    fn cancelled(&self) -> bool {
        self.wait_control.stopped()
    }
    fn finish_wait(&mut self) -> bool {
        let result = self.wait_control.finish_wait();
        let _ = self.tx.send(Event::RecoveryWait(None));
        result
    }
}
pub fn launch(request: Request, tx: Sender<Event>) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(request, &tx)))
            .unwrap_or_else(|_| {
                Err("작업이 예기치 않게 종료되었습니다. 저장된 단계와 백업을 확인해 주세요.".into())
            });
        let _ = tx.send(Event::Finished(result));
    });
}

fn run(request: Request, tx: &Sender<Event>) -> Result<Vec<String>, String> {
    // Fail before contacting hardware if the operation transcript cannot start.
    let transcript = crate::transcript::Transcript::create(&request.log_dir, request.mask_serial)?;
    let _ = tx.send(Event::LogPath(transcript.path().to_owned()));
    let store = StateStore::new(request.state_path.clone());
    let mut log = vec![];
    let mut ui = ChannelUi::new(tx.clone(), request.wait_control.clone());
    // The monitor owns the live sink and file, and flushes even on unwinding.
    // Drop stops/joins it before Finished is sent, preserving final log events.
    let pump = crate::transcript::Pump::start(transcript, tx.clone());
    let result = (|| {
        let _lock = store.lock()?;
        match request.action {
            Action::Full {
                approved,
                options,
                output,
            } => {
                let _ = tx.send(Event::Progress(Step::Check));
                ltbox_core::live!(
                    log,
                    "시작 전 펌웨어를 다시 검사하고 파일 해시를 기록합니다. 대용량 패키지는 시간이 걸릴 수 있습니다."
                );
                let firmware = check_firmware(&approved.image_dir, approved.region)
                    .map_err(|e| e.to_string())?;
                confirm_approval(&approved, &firmware)?;
                let _ = tx.send(Event::RecoveryStatus(
                    "공식 TWRP 파일을 준비하고 SHA-256을 확인합니다.".into(),
                ));
                let twrp = lapiswitch_core::recovery_asset::acquire(
                    &request
                        .state_path
                        .parent()
                        .ok_or("캐시 경로 없음")?
                        .join("cache/twrp"),
                    &mut log,
                )?;
                let builder = BackupBuilder::create(&output)?.for_recovery();
                let mut state = RunState::new(&firmware, options, builder.path())?;
                state.version = 2;
                state.recovery = Some(Default::default());
                store.begin(&state)?;
                let _ = tx.send(Event::Backup(builder.path().to_owned()));
                let _ = tx.send(Event::Progress(Step::Device));
                let _ = tx.send(Event::RecoveryStatus(
                    "기기와 활성 슬롯을 확인한 뒤 EDL로 전환합니다.".into(),
                ));
                let mut device = NativeDevice::new(builder.path())?;
                let prepared = lapiswitch_core::prepare::prepare_with_confirmation(
                    &mut device,
                    &firmware,
                    &mut |raw| builder.save_getvar(raw),
                    state.options.rollback_mode,
                    &mut || ui.confirm_edl_start(),
                    &mut log,
                )
                .map_err(|e| e.to_string())?;
                let mut stage = NativeStage::new(device, prepared.session);
                let _ = tx.send(Event::Progress(Step::Backup));
                let _ = tx.send(Event::RecoveryStatus(
                    "원본 GPT·활성 슬롯 Recovery·misc와 고유 파티션을 백업합니다.".into(),
                ));
                let backup = stage1::take_backup(
                    &mut stage,
                    &prepared.report,
                    &firmware,
                    builder,
                    &mut log,
                )?;
                state.bind_backup(&backup)?;
                lapiswitch_core::recovery_flow::checkpoint(
                    &mut state,
                    &store,
                    lapiswitch_core::recovery_flow::Phase::BackedUp,
                )?;
                let _ = tx.send(Event::Progress(Step::StageRecovery));
                let _ = tx.send(Event::RecoveryStatus(
                    "GPT를 수정하고 활성 슬롯 Recovery와 misc를 준비·검증합니다.".into(),
                ));
                lapiswitch_core::recovery_flow::stage(
                    &mut stage, &backup, &twrp, &mut state, &store, &mut log,
                )?;
                let mut device = NativeWorkflow::from_stage(stage);
                lapiswitch_core::recovery_flow::finish(
                    &mut device,
                    &backup,
                    &firmware,
                    &mut state,
                    &store,
                    &mut ui,
                    &mut log,
                )?;
                Ok(state.warnings)
            }
            Action::Resume => {
                let mut state = store.load()?.ok_or("저장된 작업이 없습니다.")?;
                if state.recovery.is_none() {
                    return Err(lapiswitch_core::state::LEGACY_CONVERSION_ENDED.into());
                }
                if state.recovery.is_some() {
                    let _ = tx.send(Event::RecoveryPhase(
                        state.recovery.as_ref().expect("checked").phase,
                    ));
                    let _ = tx.send(Event::RecoveryStatus(
                        "이어하기 전 백업과 펌웨어를 다시 검증합니다.".into(),
                    ));
                    let backup = VerifiedBackup::open(&state.backup_dir)?;
                    let phase = state.recovery.as_ref().expect("checked").phase;
                    if matches!(
                        phase,
                        lapiswitch_core::recovery_flow::Phase::Staging
                            | lapiswitch_core::recovery_flow::Phase::Restored
                            | lapiswitch_core::recovery_flow::Phase::Done
                            | lapiswitch_core::recovery_flow::Phase::Preflight
                            | lapiswitch_core::recovery_flow::Phase::BackedUp
                    ) {
                        return Err(
                            "이 단계는 원본 구성을 복구하거나 새 작업으로 시작하세요.".into()
                        );
                    }
                    let firmware = check_firmware(&state.firmware_dir, state.target_region)
                        .map_err(|e| e.to_string())?;
                    state.verify_binding(&backup, &firmware)?;
                    let _ = tx.send(Event::Backup(backup.path().to_owned()));
                    let mut native = NativeDevice::new(backup.path())?;
                    let mut device = if phase == lapiswitch_core::recovery_flow::Phase::Staged {
                        native.ensure_single_target()?;
                        if native.detect()? != Connection::Edl {
                            return Err("준비된 기기를 EDL로 연결하세요. 이미 TWRP로 진입했다면 재부팅 요청 전송 상태를 확인해야 합니다.".into());
                        }
                        let session = native.open_edl(&firmware.loader, &mut log)?;
                        NativeWorkflow::from_stage(NativeStage::new(native, session))
                    } else {
                        NativeWorkflow::disconnected(native)
                    };
                    lapiswitch_core::recovery_flow::finish(
                        &mut device,
                        &backup,
                        &firmware,
                        &mut state,
                        &store,
                        &mut ui,
                        &mut log,
                    )?;
                    return Ok(state.warnings);
                }
                Err(lapiswitch_core::state::LEGACY_CONVERSION_ENDED.into())
            }
            Action::Restore { backup, loader } => {
                let _ = tx.send(Event::Progress(Step::Check));
                let backup = VerifiedBackup::open_recovery(&backup)?;
                // A selected backup must not bypass an unrelated interrupted run.
                if let Ok(Some(state)) = store.load() {
                    let incomplete = backup::transaction_stage_in(&state.backup_dir)
                        .map(|transaction| {
                            state.resume_action(transaction) != ResumeAction::Restart
                        })
                        .unwrap_or(true);
                    if incomplete
                        && state.backup_dir.canonicalize().map_err(|e| e.to_string())?
                            != backup.path().canonicalize().map_err(|e| e.to_string())?
                    {
                        return Err("미완료 작업에 연결된 백업을 먼저 복구해 주세요.".into());
                    }
                }
                let assessment = lapiswitch_core::recovery_flow::restore::preflight(&backup)?;
                ltbox_core::live!(log, "원본 복구 DeviceInfo 판정: {:?}", assessment);
                if let Some(state) = store.load()?
                    && state.backup_dir.canonicalize().map_err(|e| e.to_string())?
                        == backup.path().canonicalize().map_err(|e| e.to_string())?
                    && state.recovery.as_ref().is_some_and(|p| {
                        matches!(
                            p.phase,
                            lapiswitch_core::recovery_flow::Phase::FirmwareWriting
                                | lapiswitch_core::recovery_flow::Phase::FinalReboot
                                | lapiswitch_core::recovery_flow::Phase::Done
                        )
                    })
                {
                    return Err("공식 펌웨어 설치가 시작된 작업입니다. 과거 Recovery/GPT로 되돌리지 않고 펌웨어 설치를 이어가세요.".into());
                }
                let twrp = if matches!(assessment,lapiswitch_core::recovery_flow::restore::Assessment::Verified{original,current} if original!=current)
                {
                    Some(lapiswitch_core::recovery_asset::acquire(
                        &request
                            .state_path
                            .parent()
                            .ok_or("캐시 경로 없음")?
                            .join("cache/twrp"),
                        &mut log,
                    )?)
                } else {
                    None
                };
                let _ = tx.send(Event::Backup(backup.path().to_owned()));
                let _ = tx.send(Event::Progress(Step::Edl));
                let mut device = NativeDevice::new(backup.path())?;
                device.ensure_single_target()?;
                if device.detect()? != Connection::Edl {
                    return Err("복구할 기기를 EDL로 연결해 주세요.".into());
                }
                let session = device.open_edl(&loader, &mut log)?;
                let mut device = NativeWorkflow::from_stage(NativeStage::new(device, session));
                let _ = tx.send(Event::Progress(Step::Originals));
                let warnings = lapiswitch_core::recovery_flow::restore::run(
                    &mut device,
                    &backup,
                    &loader,
                    twrp.as_deref(),
                    &mut lapiswitch_core::recovery_flow::restore::NativeConnector,
                    &mut SystemClock::default(),
                    &mut ui,
                    &mut log,
                )?;
                let mut report = lapiswitch_core::stage1::StageReport {
                    backup_dir: backup.path().into(),
                    warnings,
                };
                let _ = tx.send(Event::Progress(Step::Finish));
                match store.archive_restored(&backup) {
                    Ok(Some(path)) => ltbox_core::live!(
                        log,
                        "복구 완료된 작업 기록을 보관했습니다: {}",
                        path.display()
                    ),
                    Ok(None) => {}
                    Err(error) => report.warnings.push(format!(
                        "원본 복구는 완료했지만 이전 작업 기록 정리를 마치지 못했습니다: {error}"
                    )),
                }
                Ok(report.warnings)
            }
        }
    })();
    match &result {
        Ok(warnings) => {
            ltbox_core::live!(log, "작업 완료");
            for w in warnings {
                ltbox_core::live!(log, "경고: {w}");
            }
        }
        Err(error) => ltbox_core::live!(log, "작업 중단: {error}"),
    }
    pump.finish()?;
    result
}

pub const POLL_INTERVAL: Duration = Duration::from_millis(400);

fn confirm_approval(approved: &FirmwareReport, current: &FirmwareReport) -> Result<(), String> {
    if approved.fingerprint != current.fingerprint
        || approved.indices != current.indices
        || approved.key_class != current.key_class
        || approved.region != current.region
        || approved.loader != current.loader
        || approved.raw_xmls != current.raw_xmls
        || approved.patch_xmls != current.patch_xmls
        || approved.missing_images != current.missing_images
    {
        Err(
            "검사한 뒤 펌웨어 정보가 변경되었습니다. 폴더를 다시 선택하고 내용을 확인해 주세요."
                .into(),
        )
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_wait_events_follow_phase_and_retire_before_writes() {
        use lapiswitch_core::recovery_flow::{Phase, Ui};
        let (tx, events) = mpsc::channel();

        let control = WaitControl::default();
        let mut ui = ChannelUi::new(tx, control.clone());
        for (phase, text, waiting) in [
            (Phase::RecoveryWaiting, "TWRP wait", true),
            (Phase::Planned, "DeviceInfo write", false),
            (Phase::EdlWaiting, "EDL wait", true),
        ] {
            Ui::phase(&mut ui, phase);
            assert!(matches!(events.recv().unwrap(), Event::RecoveryWait(None)));
            assert!(
                matches!(events.recv().unwrap(), Event::RecoveryPhase(value) if value == phase)
            );
            Ui::status(&mut ui, text, waiting);
            let Event::RecoveryWait(ticket) = events.recv().unwrap() else {
                panic!("expected wait event")
            };
            assert_eq!(ticket.map(|t| t.phase), waiting.then_some(phase));
            assert!(
                matches!(events.recv().unwrap(), Event::RecoveryStatus(value) if value == text)
            );
            assert!(!Ui::cancelled(&ui));
            assert!(Ui::finish_wait(&mut ui));
            assert!(matches!(events.recv().unwrap(), Event::RecoveryWait(None)));
            if let Some(ticket) = ticket {
                assert!(!control.request_stop(ticket));
            }
        }
    }
    #[test]
    fn accepted_gui_stop_reaches_worker_and_cannot_be_cleared_by_handoff() {
        use lapiswitch_core::recovery_flow::{Phase, Ui};
        for phase in [Phase::RecoveryWaiting, Phase::EdlWaiting] {
            let (tx, _events) = mpsc::channel();

            let control = WaitControl::default();
            let mut ui = ChannelUi::new(tx, control.clone());
            Ui::phase(&mut ui, phase);
            Ui::status(&mut ui, "waiting", true);
            let ticket = control
                .begin_at(phase, std::time::Instant::now() - Duration::from_secs(181))
                .unwrap();
            assert!(control.request_stop(ticket));
            assert!(Ui::cancelled(&ui));
            assert!(!Ui::finish_wait(&mut ui));
            Ui::status(&mut ui, "handoff", false);
            Ui::phase(&mut ui, Phase::Planned);
            assert!(Ui::cancelled(&ui));
            assert!(!Ui::finish_wait(&mut ui));
        }
    }
    #[test]
    fn firmware_must_still_match_the_reviewed_build_before_device_work() {
        let approved = FirmwareReport {
            image_dir: "demo".into(),
            loader: "demo/loader.melf".into(),
            region: Region::Row,
            fingerprint: "Lenovo/TB520FU_ROW/approved".into(),
            key_class: lapiswitch_core::avb_info::KeyClass::Lenovo,
            indices: lapiswitch_core::rollback::RollbackIndices {
                boot: 10,
                vbmeta_system: 10,
            },
            raw_xmls: vec![],
            patch_xmls: vec![],
            decrypted: 0,
            missing_images: vec![],
            lun4_gpt_in_package: true,
        };
        assert!(confirm_approval(&approved, &approved).is_ok());
        let mut replaced = approved.clone();
        replaced.fingerprint = "Lenovo/TB520FU_ROW/replaced".into();
        assert!(confirm_approval(&approved, &replaced).is_err());
        replaced = approved.clone();
        replaced.indices.boot += 1;
        assert!(confirm_approval(&approved, &replaced).is_err());
        replaced = approved.clone();
        replaced.raw_xmls.push("new.xml".into());
        assert!(confirm_approval(&approved, &replaced).is_err());
    }
    #[test]
    fn startup_without_saved_state_needs_no_device() {
        let dir = tempfile::tempdir().unwrap();
        assert!(pending(&dir.path().join("state.json")).unwrap().is_none());
    }
}

#[cfg(test)]
mod edl_start_tests {
    use super::*;
    #[test]
    fn retired_pending_run_is_recovery_only_and_does_not_modify_saved_evidence() {
        use lapiswitch_core::{
            gpt::CURRENT_POLICY, rollback::RollbackIndices, slot::Slot, state::Stage,
        };
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::new(dir.path().join("state.json"));
        let state = RunState {
            version: 1,
            gpt_policy: CURRENT_POLICY,
            recovery: None,
            run_id: "retired-test".into(),
            stage: Stage::S5,
            target_region: Region::Row,
            firmware_dir: dir.path().join("missing-firmware"),
            fingerprint: "synthetic".into(),
            indices: RollbackIndices {
                boot: 0,
                vbmeta_system: 0,
            },
            files: vec![],
            options: FlashOptions::new(true, None).unwrap(),
            backup_dir: dir.path().to_owned(),
            backup_sha256: None,
            slot: Some(Slot::A),
            persist_fingerprint: None,
            warnings: vec![],
        };
        store.save(&state).unwrap();
        let journal = dir.path().join("transaction.json");
        std::fs::write(
            &journal,
            br#"{"version":1,"stage":"S4_DONE","recorded_at":"synthetic"}"#,
        )
        .unwrap();
        let before = std::fs::read(store.path()).unwrap();
        let journal_before = std::fs::read(&journal).unwrap();
        let old = pending(store.path()).unwrap().unwrap();
        assert_eq!(old.action, ResumeAction::Recovery);
        assert_eq!(std::fs::read(store.path()).unwrap(), before);
        assert_eq!(std::fs::read(journal).unwrap(), journal_before);
    }
    #[test]
    fn edl_start_never_proceeds_without_a_positive_response() {
        for response in [None, Some(false), Some(true)] {
            let (tx, events) = mpsc::channel();

            let join = std::thread::spawn(move || {
                ChannelUi::new(tx, WaitControl::default()).confirm_edl_start()
            });
            let Event::ConfirmEdlStart(tx) = events.recv_timeout(Duration::from_secs(2)).unwrap()
            else {
                panic!("confirmation missing")
            };
            assert!(!join.is_finished());
            if let Some(answer) = response {
                tx.send(answer).unwrap();
            }
            drop(tx);
            assert_eq!(join.join().unwrap().is_ok(), response == Some(true));
        }
    }
}
