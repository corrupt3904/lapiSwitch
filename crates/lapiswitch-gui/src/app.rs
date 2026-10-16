mod archive;
mod conflicts;
mod country_view;
mod dashboard_view;
mod licenses_view;
mod settings_view;
mod view;
mod wait_ui;
use crate::progress::{Flow, RunProgress, Step};
use crate::settings::{Language, Preferences};
use crate::transcript;
use crate::worker::{self, Event, Pending};
use iced::{Element, Subscription, Task, Theme};
use lapiswitch_core::firmware::FirmwareReport;
use lapiswitch_core::region::Region;
use lapiswitch_core::rollback::RollbackIndices;
use lapiswitch_core::state::{FlashOptions, ResumeAction, Stage, StateStore};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Dashboard,
    Region,
    Firmware,
    Options,
    Confirm,
    Progress,
    Result,
    Recovery,
    Settings,
    Resume,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Activity {
    Inspect,
    Dialog,
    Install,
    Archive,
    Run,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunKind {
    Convert,
    Resume,
    Restore,
}
#[derive(Debug, Clone, Copy)]
enum Picker {
    Firmware,
    Output,
    Backup,
    Loader,
    Export,
}
#[derive(Debug, Clone)]
enum Message {
    Tick,
    Next,
    Back,
    Home,
    Convert,
    Recovery,
    Settings,
    Resume,
    Region(Region),
    Pick(Picker),
    Picked(Picker, Option<PathBuf>),
    Country(String),
    Rollback(lapiswitch_core::deviceinfo::RollbackMode),
    CountryOpen,
    LicenseOpen,
    LicenseClose,
    LicenseSearch(String),
    LicenseSelect(usize),
    CountrySearch(String),
    CountryCancel,
    CountryApply,
    Agree(bool),
    Start,
    StartVerified,
    RestoreStart,
    ResumeStart,
    CancelArchiveRun,
    ConfirmArchiveRun,
    RunArchived(Result<PathBuf, String>),
    Language(Language),
    Mask(bool),
    Kernel(bool),
    CheckDrivers,
    InstallDrivers,
    KillAdb,
    AdbKilled(Result<(), String>),
    CloseFix,
    ConfirmCloseFix,
    CancelCloseFix,
    FixPolled(Result<bool, String>),
    FixClosed(Result<(), ltbox_device::software_fix::CloseError>),
    StopWait(worker::WaitTicket),
    CancelStopWait,
    ConfirmStopWait(worker::WaitTicket),
    ConfirmRecoveryReboot,
    ConfirmEdlStart(bool),
    Close,
    Stay,
    Exit,
    Exported(Result<(), String>),
    Snapshot(iced::window::Screenshot),
    FocusMove(bool),
}
struct App {
    page: Page,
    return_page: Page,
    activity: Option<Activity>,
    /// No default: the user must pick PRC or ROW first.
    region: Option<Region>,
    folder: Option<PathBuf>,
    report: Option<FirmwareReport>,
    country: Option<String>,
    rollback_mode: lapiswitch_core::deviceinfo::RollbackMode,
    recovery_transition: Option<lapiswitch_core::recovery_flow::Transition>,
    recovery_detail: Option<String>,
    country_open: bool,
    license_open: bool,
    license_search: String,
    license_selection: usize,
    country_search: String,
    country_draft: Option<String>,
    agreed: bool,
    output: PathBuf,
    state_path: PathBuf,
    preferences_path: PathBuf,
    preferences: Preferences,
    startup_error: Option<String>,
    pending: Option<Pending>,
    archive_confirmation: Option<lapiswitch_core::state::RunState>,
    error: Option<String>,
    notice: Option<String>,
    driver_status: Option<ltbox_device::driver::DriverStatus>,
    driver_checking: bool,
    driver_error: Option<String>,
    driver_restart: bool,
    device: crate::device::Monitor,
    deferred: Option<Message>,
    queued: Vec<Task<Message>>,
    adb_killing: bool,
    fix: conflicts::SoftwareFix,
    recovery_folder: Option<PathBuf>,
    recovery_summary: Option<String>,
    loader: Option<PathBuf>,
    progress: RunProgress,
    comparison: Vec<String>,
    wait_control: worker::WaitControl,
    wait_ticket: Option<worker::WaitTicket>,
    stop_confirmation: Option<worker::WaitTicket>,
    close_warning: bool,
    reboot_confirmation: Option<Sender<()>>,
    edl_confirmation: Option<Sender<bool>>,
    logs: VecDeque<String>,
    log_path: Option<PathBuf>,
    backup_path: Option<PathBuf>,
    flash: Option<ltbox_device::edl::FlashProgress>,
    transfer_line: Option<String>,
    result: Option<Result<Vec<String>, String>>,
    run_kind: RunKind,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    demo: bool,
    demo_ticks: u8,
    snapshot: Option<PathBuf>,
    snapshot_ticks: u8,
}
impl App {
    fn base(demo: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let default_path = StateStore::default_path();
        let startup_error = default_path.as_ref().err().cloned();
        let state_path = default_path
            .unwrap_or_else(|_| std::env::temp_dir().join("tb520-region-tool/state.json"));
        let data = state_path.parent().expect("state parent").to_owned();
        let preferences_path = data.join("preferences.json");
        let preferences = if demo {
            Preferences::default()
        } else {
            Preferences::load(&preferences_path)
        };
        Self {
            page: Page::Dashboard,
            return_page: Page::Dashboard,
            activity: None,
            region: None,
            folder: None,
            report: None,
            country: None,
            rollback_mode: lapiswitch_core::deviceinfo::RollbackMode::Reset,
            recovery_transition: None,
            recovery_detail: None,
            country_open: false,
            license_open: false,
            license_search: String::new(),
            license_selection: 0,
            country_search: String::new(),
            country_draft: None,
            agreed: false,
            output: preferences.storage_root(&data),
            state_path,
            preferences_path,
            preferences,
            startup_error,
            pending: None,
            archive_confirmation: None,
            error: None,
            notice: None,
            driver_status: None,
            driver_checking: false,
            driver_error: None,
            driver_restart: false,
            device: crate::device::Monitor::default(),
            deferred: None,
            queued: vec![],
            adb_killing: false,
            fix: conflicts::SoftwareFix::default(),
            recovery_folder: None,
            recovery_summary: None,
            loader: None,
            progress: RunProgress::new(Flow::Recovery),
            comparison: vec![],
            wait_control: worker::WaitControl::default(),
            wait_ticket: None,
            stop_confirmation: None,
            close_warning: false,
            reboot_confirmation: None,
            edl_confirmation: None,
            logs: VecDeque::new(),
            log_path: None,
            backup_path: None,
            flash: None,
            transfer_line: None,
            result: None,
            run_kind: RunKind::Convert,
            tx,
            rx,
            demo,
            demo_ticks: 0,
            snapshot: None,
            snapshot_ticks: 0,
        }
    }
    fn new() -> (Self, Task<Message>) {
        let preview = std::env::args().find(|arg| arg == "--demo" || arg.starts_with("--demo="));
        let mut app = Self::base(preview.is_some());
        ltbox_core::live_sink::attach_gui();
        app.preferences.language.install();
        app.apply_driver_mode();
        if let Some(preview) = preview {
            let language = std::env::args().find_map(|arg| match arg.as_str() {
                "--english" | "--lang=en" => Some(Language::English),
                "--lang=ko" => Some(Language::Korean),
                "--lang=zh" => Some(Language::Chinese),
                "--lang=ja" => Some(Language::Japanese),
                "--lang=ru" => Some(Language::Russian),
                _ => None,
            });
            if let Some(language) = language {
                app.preferences.language = language;
                app.preferences.language.install();
            }
            app.seed_preview(preview.trim_start_matches("--demo="));
            app.snapshot =
                std::env::args().find_map(|arg| arg.strip_prefix("--snapshot=").map(PathBuf::from));
        } else {
            app.refresh_pending();
            worker::drivers(app.tx.clone());
            app.driver_checking = true;
        }
        (app, Task::none())
    }
    fn t(&self, ko: &'static str, en: &'static str) -> &'static str {
        self.preferences.language.text(ko, en)
    }
    fn busy(&self) -> bool {
        self.activity.is_some() || self.adb_killing || self.fix.closing
    }
    fn options(&self) -> Option<FlashOptions> {
        self.country.as_ref()?;
        FlashOptions::new(true, self.country.as_deref().filter(|v| *v != "—"))
            .map(|mut options| {
                options.rollback_mode = self.rollback_mode;
                options
            })
            .ok()
    }
    fn drivers_ready(&self) -> bool {
        !self.driver_checking
            && matches!(
                self.driver_status,
                Some(
                    ltbox_device::driver::DriverStatus::Present
                        | ltbox_device::driver::DriverStatus::NotWindows
                )
            )
    }
    fn drivers_installable(&self) -> bool {
        use ltbox_device::driver::DriverStatus::*;
        !self.driver_checking
            && matches!(
                self.driver_status,
                Some(
                    Missing(_)
                        | UdevRulesMissing
                        | UdevRulesStale
                        | UdevRulesNoPermission
                        | KernelDriverMissing
                )
            )
    }
    fn poll_device(&mut self, passive: bool) {
        if !self.demo
            && let Some(id) = self.device.begin()
        {
            worker::device(self.tx.clone(), id, passive);
        }
    }
    fn conversion_steps_visible(&self) -> bool {
        let page = if self.page == Page::Settings {
            self.return_page
        } else {
            self.page
        };
        match page {
            Page::Region | Page::Firmware | Page::Options | Page::Confirm | Page::Resume => true,
            Page::Progress | Page::Result => self.run_kind != RunKind::Restore,
            _ => false,
        }
    }
    fn can_start(&self) -> bool {
        self.start_blocker().is_none()
    }
    fn start_blocker(&self) -> Option<&'static str> {
        let reason = if self.busy() {
            self.t(
                "진행 중인 작업이 끝날 때까지 기다려 주세요.",
                "Wait for the current operation to finish.",
            )
        } else if self.deferred.is_some() {
            self.t(
                "시작 전 기기 연결을 다시 확인하고 있습니다.",
                "Rechecking the device before starting.",
            )
        } else if self.startup_error.is_some() {
            self.t(
                "이전 작업 기록을 확인할 수 없습니다. 위 오류를 확인해 주세요.",
                "The saved run cannot be checked. Review the error above.",
            )
        } else if !self.device.reachable() {
            self.t(
                "기기가 연결되어 있지 않거나 준비되지 않았습니다. 연결 안내를 확인해 주세요.",
                "The device is disconnected or not ready. Check the connection guidance.",
            )
        } else if self.device.unsupported_model() {
            self.t(
                "지원하지 않는 기기입니다. TB520FU(lapis)를 연결해 주세요.",
                "Unsupported device. Connect a TB520FU (lapis).",
            )
        } else if !self.device.startable() {
            self.t(
                "기기 모델을 확인할 수 없습니다. TB520FU 연결 상태를 확인해 주세요.",
                "The device model is unknown. Check the TB520FU connection.",
            )
        } else if !self.drivers_ready() {
            self.t(
                "필요한 드라이버를 확인하거나 설치해 주세요.",
                "Check or install the required drivers.",
            )
        } else if self.folder.is_none()
            || !self
                .report
                .as_ref()
                .is_some_and(|f| Some(f.region) == self.region)
        {
            self.t(
                "선택한 리전의 펌웨어 검사를 완료해 주세요.",
                "Complete the firmware check for the selected region.",
            )
        } else if self.options().is_none() {
            self.t(
                "국가 코드 또는 변경 안 함을 선택하고 적용해 주세요.",
                "Apply a country code or Do not change.",
            )
        } else if !self.agreed {
            self.t(
                "사용자 데이터 초기화 안내를 확인하고 동의 항목을 체크해 주세요.",
                "Read the data-erasure notice and select the consent checkbox.",
            )
        } else {
            return None;
        };
        Some(reason)
    }
    fn can_restore(&self) -> bool {
        !self.busy()
            && !self.device.unsupported_model()
            && self.agreed
            && self.recovery_summary.is_some()
            && self.loader.is_some()
    }
    fn can_resume(&self) -> bool {
        !self.busy()
            && !self.device.unsupported_model()
            && self.startup_error.is_none()
            && self.agreed
            && self.pending.as_ref().is_some_and(|p| {
                p.state.recovery.is_some()
                    && (p.state.options.wipe || p.action == ResumeAction::Finalize)
                    && matches!(
                        p.action,
                        ResumeAction::ResetThenWait
                            | ResumeAction::Wait
                            | ResumeAction::Flash
                            | ResumeAction::Finalize
                    )
            })
    }
    fn refresh_pending(&mut self) {
        if self.demo {
            return;
        }
        match worker::pending(&self.state_path) {
            Ok(pending) => {
                self.pending = pending;
                self.startup_error = None;
            }
            Err(error) => self.startup_error = Some(error),
        }
    }
    fn apply_driver_mode(&self) {
        let mode = if self.preferences.kernel && ltbox_device::driver::kernel_mode_supported() {
            ltbox_device::driver::QcomDriverMode::Kernel
        } else {
            ltbox_device::driver::QcomDriverMode::Userspace
        };
        ltbox_device::driver::set_qcom_driver_mode(mode);
    }
    fn save_preferences(&mut self) -> bool {
        if !self.demo
            && let Err(error) = self.preferences.save(&self.preferences_path)
        {
            self.error = Some(error);
            return false;
        }
        true
    }
    fn reset_progress(&mut self, kind: RunKind) {
        self.run_kind = kind;
        self.page = Page::Progress;
        self.activity = Some(Activity::Run);
        self.error = None;
        self.notice = None;
        self.progress = RunProgress::new(match kind {
            RunKind::Restore => Flow::Restore,
            _ => Flow::Recovery,
        });
        self.wait_control = worker::WaitControl::default();
        self.wait_ticket = None;
        self.stop_confirmation = None;
        self.transfer_line = None;
        self.result = None;
        self.close_warning = false;
        self.reboot_confirmation = None;
        self.edl_confirmation = None;
        self.recovery_transition = None;
        self.recovery_detail = None;
        self.logs.clear();
        self.comparison.clear();
        self.log_path = None;
        self.backup_path = None;
        self.flash = None;
        if !self.demo {
            ltbox_device::edl::clear_flash_progress();
            let _ = ltbox_core::live_sink::drain();
        }
    }
    fn launch(&mut self, action: worker::Action, kind: RunKind) {
        self.reset_progress(kind);
        if self.demo {
            self.demo_ticks = 1;
            return;
        }
        worker::launch(
            worker::Request {
                state_path: self.state_path.clone(),
                log_dir: transcript::log_dir(&self.output),
                mask_serial: self.preferences.mask_serial,
                wait_control: self.wait_control.clone(),
                action,
            },
            self.tx.clone(),
        );
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        // LTBox-style USB lease: finish an in-flight probe before operations,
        // installer elevation, or driver-mode changes can take ownership.
        if !self.busy()
            && self.device.in_flight.is_some()
            && self.deferred.is_none()
            && matches!(
                message,
                Message::Kernel(_)
                    | Message::InstallDrivers
                    | Message::ResumeStart
                    | Message::RestoreStart
                    | Message::KillAdb
                    | Message::CloseFix
                    | Message::ConfirmCloseFix
            )
        {
            self.deferred = Some(message);
            return Task::none();
        }
        match message {
            Message::FocusMove(reverse) => {
                return if reverse {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            Message::Rollback(mode) if !self.busy() => self.rollback_mode = mode,
            Message::Rollback(_) => {}
            Message::Tick => {
                while let Ok(event) = self.rx.try_recv() {
                    self.event(event);
                }
                if self.activity == Some(Activity::Run) && !self.demo {
                    self.flash = ltbox_device::edl::flash_progress();
                    if let Some(flash) = &self.flash {
                        self.progress.firmware_bytes(
                            flash.operation_completed_bytes,
                            flash.operation_total_bytes,
                        );
                    }
                } else if !matches!(self.activity, Some(Activity::Run | Activity::Install)) {
                    // A running job has exactly one dedicated log consumer.
                    for entry in ltbox_core::live_sink::drain() {
                        if matches!(entry.kind, ltbox_core::live_sink::Kind::Progress { .. }) {
                            self.event(Event::Transfer(Some(entry.line)));
                        } else {
                            self.transfer_line = None;
                            self.add_log(entry.line);
                        }
                    }
                }
                let monitor_task = self.poll_software_fix();
                self.queued.push(monitor_task);
                if self.activity != Some(Activity::Install)
                    && !self.adb_killing
                    && !self.fix.closing
                    && self.deferred.is_none()
                    && std::time::Instant::now() >= self.device.next
                {
                    self.poll_device(self.activity == Some(Activity::Run));
                }
                self.refresh_wait_stop();
                if self.demo
                    && self.demo_ticks > 0
                    && self.reboot_confirmation.is_none()
                    && self.edl_confirmation.is_none()
                {
                    self.demo_tick();
                }
                if self.demo && self.snapshot.is_some() {
                    self.snapshot_ticks += 1;
                    if self.snapshot_ticks == 3 {
                        return iced::window::oldest()
                            .and_then(iced::window::screenshot)
                            .map(Message::Snapshot);
                    }
                }
            }
            Message::Snapshot(screenshot) if self.demo => {
                if let Some(path) = self.snapshot.take() {
                    let saved = (|| -> Result<(), Box<dyn std::error::Error>> {
                        let file = std::fs::File::create(path)?;
                        let mut encoder =
                            png::Encoder::new(file, screenshot.size.width, screenshot.size.height);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        encoder.write_header()?.write_image_data(&screenshot.rgba)?;
                        Ok(())
                    })();
                    if let Err(error) = saved {
                        eprintln!("Preview snapshot: {error}");
                    }
                    return iced::exit();
                }
            }
            Message::Picked(picker, path) => return self.picked(picker, path),
            Message::Exported(result) => {
                self.activity = None;
                match result {
                    Ok(()) => {
                        self.notice = Some(self.t("로그를 저장했습니다.", "Log saved.").into())
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Message::RunArchived(result) => return self.run_archived(result),
            Message::Close if self.activity == Some(Activity::Archive) => {}
            Message::Close => {
                if matches!(self.activity, Some(Activity::Run | Activity::Install)) {
                    self.close_warning = true;
                } else {
                    return iced::exit();
                }
            }
            Message::Stay => self.close_warning = false,
            Message::Exit if self.close_warning => return iced::exit(),
            Message::StopWait(ticket) => self.open_wait_stop(ticket),
            Message::CancelStopWait => self.stop_confirmation = None,
            Message::ConfirmStopWait(ticket) => self.confirm_wait_stop(ticket),
            Message::ConfirmEdlStart(proceed) => {
                if let Some(tx) = self.edl_confirmation.take() {
                    let _ = tx.send(proceed);
                }
            }
            Message::ConfirmRecoveryReboot => {
                if let Some(confirmation) = self.reboot_confirmation.take() {
                    let delivered = confirmation.send(());
                    if !self.demo && delivered.is_err() {
                        self.error = Some(
                            self.t(
                                "확인을 전달하지 못했습니다. 저장된 작업 상태를 확인하세요.",
                                "Could not deliver confirmation. Check the saved run.",
                            )
                            .into(),
                        );
                    }
                }
            }
            Message::FixPolled(result) => self.fix_polled(result),
            Message::FixClosed(result) => self.fix_closed(result),
            Message::AdbKilled(result) => {
                self.adb_killing = false;
                match result {
                    Ok(()) => {
                        self.notice = Some(
                            self.t(
                                "ADB 서버 종료 요청을 보냈습니다. 연결을 다시 확인합니다.",
                                "ADB server stop requested. Rechecking the connection.",
                            )
                            .into(),
                        )
                    }
                    Err(error) => {
                        self.error = Some(format!(
                            "{}: {error}",
                            self.t("ADB 서버 종료 실패", "Could not stop ADB server")
                        ))
                    }
                }
                self.device.snapshot = Default::default();
                self.device.checked = None;
                self.device.next = std::time::Instant::now();
            }
            _ if self.busy() || self.deferred.is_some() => {}
            _ if self.fix.confirm
                && !matches!(message, Message::ConfirmCloseFix | Message::CancelCloseFix) => {}
            Message::KillAdb
                if self.device.snapshot.mode == crate::device::Mode::ServerBlocking =>
            {
                return self.kill_adb_server();
            }
            Message::CloseFix if self.can_close_software_fix() => self.fix.confirm = true,
            Message::ConfirmCloseFix => return self.confirm_close_software_fix(),
            Message::CancelCloseFix => self.fix.confirm = false,
            Message::LicenseOpen if self.page == Page::Settings => {
                self.license_open = true;
                self.license_search.clear();
                self.license_selection = 0;
            }
            Message::LicenseClose => self.license_open = false,
            Message::LicenseSearch(query) if self.license_open => self.license_search = query,
            Message::LicenseSelect(index)
                if self.license_open && index < crate::licenses::catalog().components.len() =>
            {
                self.license_selection = index;
            }
            _ if self.license_open => {}
            _ if self.country_open
                && !matches!(
                    message,
                    Message::Country(_)
                        | Message::CountrySearch(_)
                        | Message::CountryApply
                        | Message::CountryCancel
                ) => {}
            _ if self.archive_confirmation.is_some()
                && !matches!(
                    message,
                    Message::CancelArchiveRun | Message::ConfirmArchiveRun
                ) => {}
            Message::CancelArchiveRun => self.archive_confirmation = None,
            Message::ConfirmArchiveRun => return self.confirm_archive_run(),
            Message::Region(region) if self.region != Some(region) => {
                self.region = Some(region);
                self.report = None;
                self.folder = None;
                self.agreed = false;
                self.error = None;
            }
            Message::Next => {
                self.error = None;
                self.page = match self.page {
                    Page::Region if self.region.is_some() => Page::Firmware,
                    Page::Firmware if self.report.is_some() => Page::Options,
                    Page::Options if self.options().is_some() => {
                        self.agreed = false;
                        Page::Confirm
                    }
                    other => other,
                };
            }
            Message::Back => {
                self.error = None;
                self.agreed = false;
                self.page = match self.page {
                    Page::Region => Page::Dashboard,
                    Page::Firmware => Page::Region,
                    Page::Options => Page::Firmware,
                    Page::Confirm => Page::Options,
                    Page::Settings => self.return_page,
                    _ => Page::Dashboard,
                };
            }
            Message::Home => {
                self.refresh_pending();
                self.page = Page::Dashboard;
                self.agreed = false;
                self.error = None;
            }
            Message::Convert if !self.device.unsupported_model() => {
                self.page = Page::Region;
                self.agreed = false;
                self.error = None;
            }
            Message::Recovery => {
                self.page = Page::Recovery;
                self.agreed = false;
                self.error = None;
                if let Some(pending) = &self.pending {
                    self.recovery_folder = Some(pending.state.backup_dir.clone());
                    self.recovery_summary = None;
                }
                if let Some(folder) = self.recovery_folder.clone() {
                    self.activity = Some(Activity::Inspect);
                    worker::check_backup(self.tx.clone(), folder);
                }
            }
            Message::Settings if self.page != Page::Settings => {
                self.return_page = self.page;
                self.page = Page::Settings;
                self.error = None;
            }
            Message::Resume => {
                self.page = Page::Resume;
                self.agreed = false;
                self.error = None;
            }
            Message::CountryOpen => {
                self.country_open = true;
                self.country_search.clear();
                self.country_draft = self.country.clone();
            }
            Message::CountrySearch(query) if self.country_open => self.country_search = query,
            Message::Country(country)
                if self.country_open
                    && (country == "—" || crate::countries::find(&country).is_some()) =>
            {
                self.country_draft = Some(country);
            }
            Message::CountryCancel => {
                self.country_open = false;
                self.country_draft = None;
            }
            Message::CountryApply if self.country_open && self.country_draft.is_some() => {
                self.country = self.country_draft.take();
                self.country_open = false;
                self.agreed = false;
            }
            Message::Agree(agreed) => self.agreed = agreed,
            Message::Start => return self.start_conversion(),
            Message::StartVerified => self.start_verified(),
            Message::ResumeStart if self.can_resume() => {
                self.launch(worker::Action::Resume, RunKind::Resume)
            }
            Message::RestoreStart if self.can_restore() => {
                self.launch(
                    worker::Action::Restore {
                        backup: self.recovery_folder.clone().expect("verified folder"),
                        loader: self.loader.clone().expect("selected loader"),
                    },
                    RunKind::Restore,
                );
            }
            Message::Language(language) => {
                self.preferences.language = language;
                language.install();
                self.save_preferences();
            }
            Message::Mask(mask) => {
                self.preferences.mask_serial = mask;
                self.save_preferences();
            }
            Message::Kernel(kernel)
                if !self.driver_checking
                    && kernel != self.preferences.kernel
                    && (!kernel || ltbox_device::driver::kernel_mode_supported()) =>
            {
                self.preferences.kernel = kernel && ltbox_device::driver::kernel_mode_supported();
                self.apply_driver_mode();
                self.save_preferences();
                self.driver_status = None;
                self.driver_error = None;
                self.device.snapshot = crate::device::Snapshot::default();
                self.device.checked = None;
                self.device.next = std::time::Instant::now();
                if !self.demo {
                    self.driver_checking = true;
                    worker::drivers(self.tx.clone());
                }
            }
            Message::InstallDrivers if self.drivers_installable() => {
                self.activity = Some(Activity::Install);
                self.driver_restart = false;
                self.driver_error = None;
                self.notice = None;
                self.error = None;
                if self.demo {
                    self.event(Event::DriverInstalled(Ok(true)));
                    self.event(Event::Drivers(Ok(
                        ltbox_device::driver::DriverStatus::Present,
                    )));
                } else {
                    worker::install_drivers(
                        self.tx.clone(),
                        self.output.clone(),
                        self.preferences.mask_serial,
                    );
                }
            }
            Message::CheckDrivers if !self.driver_checking => {
                self.driver_error = None;
                if !self.demo {
                    self.driver_checking = true;
                    worker::drivers(self.tx.clone());
                } else {
                    self.driver_status = Some(ltbox_device::driver::DriverStatus::Present);
                }
            }
            Message::Pick(Picker::Output) if self.page != Page::Settings => {}
            Message::Pick(picker) => {
                if self.demo {
                    self.demo_pick(picker);
                    return Task::none();
                }
                self.activity = Some(Activity::Dialog);
                return Task::perform(
                    async move {
                        let dialog = rfd::AsyncFileDialog::new();
                        let path = match picker {
                            Picker::Firmware | Picker::Output | Picker::Backup => {
                                dialog.pick_folder().await.map(|f| f.path().to_owned())
                            }
                            Picker::Loader => dialog
                                .add_filter("Firehose loader", &["melf", "elf", "mbn"])
                                .pick_file()
                                .await
                                .map(|f| f.path().to_owned()),
                            Picker::Export => dialog
                                .set_file_name("tb520-session.log")
                                .save_file()
                                .await
                                .map(|f| f.path().to_owned()),
                        };
                        (picker, path)
                    },
                    |(picker, path)| Message::Picked(picker, path),
                );
            }
            _ => {}
        }
        Task::batch(std::mem::take(&mut self.queued))
    }
    fn picked(&mut self, picker: Picker, path: Option<PathBuf>) -> Task<Message> {
        self.activity = None;
        let Some(path) = path else {
            return Task::none();
        };
        self.error = None;
        self.notice = None;
        match picker {
            Picker::Firmware => {
                let Some(region) = self.region else {
                    return Task::none();
                };
                self.folder = Some(path.clone());
                self.report = None;
                self.agreed = false;
                self.activity = Some(Activity::Inspect);
                worker::validate(self.tx.clone(), path, region);
            }
            Picker::Output if self.page == Page::Settings => {
                let previous = self.preferences.storage_root.replace(path.clone());
                if self.save_preferences() {
                    self.output = path;
                    self.agreed = false;
                } else {
                    self.preferences.storage_root = previous;
                }
            }
            Picker::Output => {}
            Picker::Backup => {
                self.recovery_folder = Some(path.clone());
                self.recovery_summary = None;
                self.agreed = false;
                self.activity = Some(Activity::Inspect);
                worker::check_backup(self.tx.clone(), path);
            }
            Picker::Loader => {
                self.loader = Some(path);
                self.agreed = false;
            }
            Picker::Export => {
                if let Some(source) = self.log_path.clone() {
                    self.activity = Some(Activity::Dialog);
                    return Task::perform(
                        async move {
                            if source == path {
                                return Ok(());
                            }
                            std::fs::copy(source, path)
                                .map(|_| ())
                                .map_err(|e| e.to_string())
                        },
                        Message::Exported,
                    );
                }
            }
        }
        Task::none()
    }
    fn event(&mut self, event: Event) {
        match event {
            Event::ConfirmEdlStart(tx) => {
                if self.activity == Some(Activity::Run) && self.run_kind != RunKind::Restore {
                    self.edl_confirmation = Some(tx);
                }
            }
            Event::FirmwareRegionMismatch { firmware, target } => {
                self.activity = None;
                self.report = None;
                self.error = Some(
                    self.t(
                        "선택한 리전은 {target}이지만 펌웨어의 리전은 {firmware}입니다. 선택한 리전과 같은 펌웨어를 선택하세요.",
                        "The selected region is {target}, but the firmware region is {firmware}. Choose firmware for the selected region.",
                    )
                    .replace("{target}", target.token())
                    .replace("{firmware}", firmware.token()),
                );
            }
            Event::Validated(result) => {
                self.activity = None;
                match result {
                    Ok(report) => self.report = Some(report),
                    Err(error) => {
                        self.report = None;
                        self.error = Some(error);
                    }
                }
            }
            Event::BackupChecked(result) => {
                self.activity = None;
                match result {
                    Ok(summary) => self.recovery_summary = Some(summary),
                    Err(e) => {
                        self.recovery_summary = None;
                        self.error = Some(e);
                    }
                }
            }
            Event::Device(id, result) => {
                if self.device.finish(id, result)
                    && let Some(message) = self.deferred.take()
                {
                    // Revalidate against this snapshot, after USB handles drop.
                    let task = self.update(message);
                    self.queued.push(task);
                }
            }
            Event::Drivers(result) => {
                self.driver_checking = false;
                match result {
                    Ok(status) => {
                        self.driver_status = Some(status);
                        self.driver_error = None;
                    }
                    Err(error) => {
                        self.driver_status = None;
                        self.driver_error = Some(error);
                    }
                }
            }
            Event::DriverInstalled(result) => {
                if self.activity != Some(Activity::Install) {
                    return;
                }
                self.activity = None;
                self.close_warning = false;
                self.driver_checking = true;
                self.driver_status = None;
                self.device.next = std::time::Instant::now();
                match result {
                    Ok(true) => {
                        self.driver_restart = true;
                        self.notice = Some(self.t("설치 프로그램이 완료되었습니다. 드라이버를 다시 확인합니다. USB를 재연결하고 필요하면 PC를 재시작하세요.", "Installer completed. Rechecking drivers. Reconnect USB and restart the PC if needed.").into());
                    }
                    Ok(false) => self.notice = Some(self.t("드라이버 설치가 취소되었습니다. 설치 버튼을 눌러 다시 시도할 수 있습니다.", "Driver installation was cancelled. Use Install to try again.").into()),
                    Err(error) => self.error = Some(format!("{}: {error}", self.t("드라이버 설치 실패", "Driver installation failed"))),
                }
            }
            Event::Log(line) => self.add_log(line),
            Event::Transfer(line) => {
                self.transfer_line = line.map(|line| {
                    if self.preferences.mask_serial {
                        transcript::redact(&line)
                    } else {
                        line
                    }
                });
            }
            Event::LogPath(path) => self.log_path = Some(path),
            Event::Backup(path) => self.backup_path = Some(path),
            Event::Progress(step) => self.progress.step(step),
            Event::RecoveryPhase(phase) => {
                self.progress.recovery_phase(phase);
                self.refresh_wait_stop();
            }
            Event::RecoveryWait(ticket) => {
                self.wait_ticket = ticket;
                self.refresh_wait_stop();
            }
            Event::RecoveryStatus(text) => {
                self.recovery_detail = Some(text);
            }
            Event::ConfirmRecoveryReboot(transition, confirmation) => {
                if self.activity == Some(Activity::Run) && self.run_kind != RunKind::Restore {
                    self.recovery_transition = Some(transition);
                    self.reboot_confirmation = Some(confirmation);
                }
            }
            Event::Finished(result) => {
                self.progress.finish(result.is_ok());
                self.reboot_confirmation = None;
                self.edl_confirmation = None;
                self.recovery_transition = None;
                self.activity = None;
                self.wait_ticket = None;
                self.stop_confirmation = None;
                self.close_warning = false;

                self.transfer_line = None;
                self.page = Page::Result;
                self.result = Some(result);
                self.device.next = std::time::Instant::now();
                self.refresh_pending();
            }
        }
    }
    fn add_log(&mut self, line: String) {
        let line = if self.preferences.mask_serial {
            transcript::redact(&line)
        } else {
            line
        };
        if line.starts_with("롤백 비교 ") && self.comparison.len() < 2 {
            self.comparison.push(line.clone());
        }
        if self.logs.len() >= 400 {
            self.logs.pop_front();
        }
        self.logs.push_back(line);
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(worker::POLL_INTERVAL).map(|_| Message::Tick),
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                    modifiers,
                    ..
                }) if status == iced::event::Status::Ignored => {
                    Some(Message::FocusMove(modifiers.shift()))
                }
                iced::Event::Window(iced::window::Event::CloseRequested) => Some(Message::Close),
                _ => None,
            }),
        ])
    }
    fn view(&self) -> Element<'_, Message> {
        view::view(self)
    }
    fn demo_pick(&mut self, picker: Picker) {
        match picker {
            Picker::Firmware => {
                let region = *self.region.get_or_insert(Region::Row);
                self.folder = Some("Demo/firmware/image".into());
                self.report = Some(demo_report(region));
            }
            Picker::Backup => {
                self.recovery_folder = Some("Demo/backup/TB520FU".into());
                self.recovery_summary = Some("TB520FU · slot a · ROW · 5 files (DEMO)".into());
            }
            Picker::Loader => self.loader = Some("Demo/loader.melf".into()),
            Picker::Output => {
                let _ = self.picked(Picker::Output, Some("Demo/lapiSwitch".into()));
            }
            Picker::Export => {
                self.notice = Some(
                    self.t(
                        "미리보기에서는 파일을 저장하지 않습니다.",
                        "Preview does not save files.",
                    )
                    .into(),
                )
            }
        }
    }
    fn seed_preview(&mut self, screen: &str) {
        self.driver_status = Some(ltbox_device::driver::DriverStatus::Present);
        self.device.snapshot = crate::device::Snapshot {
            mode: crate::device::Mode::Adb,
            model: "TB520FU".into(),
            name: "Lenovo Legion Y700".into(),
            android: "15".into(),
            slot: "a".into(),
            firmware: "ZUI_17.5.10.309_ST".into(),
            ram: "12 GB".into(),
            storage: "256 GB".into(),
            serial: "DEMO-SERIAL".into(),
            ..Default::default()
        };
        self.device.checked = Some(std::time::Instant::now());
        self.demo_pick(Picker::Firmware);
        self.country = Some("KR".into());
        match screen {
            "dashboard" => self.page = Page::Dashboard,
            "drivers" => {
                self.page = Page::Dashboard;
                self.driver_status = Some(ltbox_device::driver::DriverStatus::KernelDriverMissing);
            }
            "disconnected" => {
                self.page = Page::Confirm;
                self.device.snapshot = Default::default();
                self.agreed = true;
            }
            "adb-conflict" => {
                self.page = Page::Dashboard;
                self.device.snapshot = crate::device::Snapshot {
                    mode: crate::device::Mode::ServerBlocking,
                    ..Default::default()
                };
            }
            "notifications" => {
                self.page = Page::Dashboard;
                self.fix.running = true;
                self.pending = Some(Pending {
                    state: demo_saved_state(Stage::S5),
                    action: ResumeAction::Wait,
                });
            }
            "software-fix" => {
                self.page = Page::Dashboard;
                self.fix.running = true;
            }
            "software-fix-confirm" => {
                self.page = Page::Dashboard;
                self.fix.running = true;
                self.fix.confirm = true;
            }
            "unauthorized" => {
                self.page = Page::Dashboard;
                self.device.snapshot = crate::device::Snapshot {
                    mode: crate::device::Mode::Unauthorized,
                    ..Default::default()
                };
            }
            "country" => {
                self.page = Page::Options;
                self.country_open = true;
                self.country_draft = Some("KR".into());
            }
            "country-search" => {
                self.page = Page::Options;
                self.country_open = true;
                self.country_draft = Some("KR".into());
                self.country_search = "ko".into();
            }
            "region" => {
                self.page = Page::Region;
                self.region = None;
                self.report = None;
                self.folder = None;
                self.country = None;
            }
            "firmware" => self.page = Page::Firmware,
            "options" => self.page = Page::Options,
            "options-unselected" => {
                self.page = Page::Options;
                self.country = None;
            }
            "wrong-device" => {
                self.page = Page::Confirm;
                self.device.snapshot.model = "TB321FU".into();
                self.device.snapshot.name = "Unsupported tablet".into();
                self.agreed = true;
            }
            "legacy-resume" => {
                self.page = Page::Resume;
                self.pending = Some(Pending {
                    state: demo_saved_state(Stage::S5),
                    action: ResumeAction::Recovery,
                });
            }
            "resume" | "confirm-pending" | "archive-run" => {
                self.page = if screen == "resume" {
                    Page::Resume
                } else {
                    Page::Confirm
                };
                self.agreed = screen != "resume";
                let mut state = demo_saved_state(Stage::S5);
                state.version = 2;
                state.recovery = Some(lapiswitch_core::recovery_flow::Progress {
                    phase: lapiswitch_core::recovery_flow::Phase::RecoveryWaiting,
                    attempt: None,
                });
                if screen == "archive-run" {
                    self.archive_confirmation = Some(state.clone());
                }
                self.pending = Some(Pending {
                    state,
                    action: ResumeAction::Wait,
                });
            }
            "settings" => self.page = Page::Settings,
            "licenses" | "licenses-search" | "licenses-twrp" => {
                self.page = Page::Settings;
                self.license_open = true;
                if screen == "licenses-search" || screen == "licenses-twrp" {
                    let name = if screen == "licenses-twrp" {
                        "TWRP / TeamWin"
                    } else {
                        "flag-icons"
                    };
                    self.license_search = name.into();
                    self.license_selection = crate::licenses::catalog()
                        .components
                        .iter()
                        .position(|c| c.name == self.license_search)
                        .unwrap();
                }
            }
            "progress" | "progress-short" | "progress-long" => {
                self.page = Page::Progress;
                self.activity = Some(Activity::Run);

                self.progress = RunProgress::new(Flow::Recovery);
                self.progress.step(Step::Firmware);
                self.progress.firmware_bytes(420, 1000);
                self.flash = Some(ltbox_device::edl::FlashProgress {
                    partition: "super".into(),
                    percent: 42,
                    completed_bytes: 420,
                    total_bytes: 1000,
                    operation_completed_bytes: 420,
                    operation_total_bytes: 1000,
                });
                for index in 0..30 {
                    self.add_log(if screen == "progress-long" {
                        format!(
                            "DEMO {index}: 검증한 백업과 펌웨어를 확인하고 있습니다. {}",
                            "TB520FU ".repeat(28)
                        )
                    } else {
                        format!("DEMO {index}: 전송 상태 확인")
                    });
                }
                self.transfer_line = Some("쓰기 super: 42% (4.2 GB / 10 GB)".into());
            }
            "firmware-mismatch" => {
                self.page = Page::Firmware;
                self.event(Event::FirmwareRegionMismatch {
                    firmware: Region::Prc,
                    target: Region::Row,
                });
            }
            "edl-start-warning" => {
                self.page = Page::Progress;
                self.activity = Some(Activity::Run);
                self.progress = RunProgress::new(Flow::Recovery);
                self.progress.step(Step::Device);
                let (tx, _) = mpsc::channel();
                self.edl_confirmation = Some(tx);
            }
            "confirm" => self.page = Page::Confirm,
            "twrp-warning" | "edl-warning" | "deviceinfo" | "twrp-wait" | "new-flash"
            | "finalizing" | "preflight" => {
                self.page = Page::Progress;
                self.activity = Some(Activity::Run);
                self.progress = RunProgress::new(Flow::Recovery);
                let (step, detail) = match screen {
                    "twrp-warning" | "twrp-wait" => (
                        Step::Twrp,
                        "TWRP ADB 연결을 기다립니다. PC와 태블릿의 USB 연결을 유지해 주세요.",
                    ),
                    "edl-warning" => (
                        Step::Edl,
                        "DeviceInfo 검증을 마쳤습니다. EDL로 재부팅합니다.",
                    ),
                    "preflight" => (Step::Check, "펌웨어와 공식 TWRP 파일을 검사합니다."),
                    "new-flash" => (
                        Step::Firmware,
                        "공식 펌웨어를 설치하고 사용자 데이터를 초기화합니다.",
                    ),
                    "finalizing" => (
                        Step::Finish,
                        "GPT·국가 코드·활성 슬롯을 확인한 뒤 시스템 부팅을 요청합니다.",
                    ),
                    _ => (
                        Step::DeviceInfo,
                        "TWRP에서 DeviceInfo를 백업하고 전체 바이트를 검증합니다.",
                    ),
                };
                self.progress.step(step);
                self.recovery_detail = Some(detail.into());
                if screen == "new-flash" {
                    self.flash = Some(ltbox_device::edl::FlashProgress {
                        partition: "super".into(),
                        percent: 42,
                        completed_bytes: 420,
                        total_bytes: 1000,
                        operation_completed_bytes: 420,
                        operation_total_bytes: 1000,
                    });
                    self.progress.firmware_bytes(420, 1000);
                }
                if matches!(screen, "twrp-warning" | "edl-warning") {
                    self.recovery_transition = Some(if screen == "twrp-warning" {
                        lapiswitch_core::recovery_flow::Transition::Twrp
                    } else {
                        lapiswitch_core::recovery_flow::Transition::Edl
                    });
                    let (confirmation, _) = mpsc::channel();
                    self.reboot_confirmation = Some(confirmation);
                }
            }
            "twrp-stop" | "edl-stop" | "twrp-stop-confirm" | "edl-stop-confirm" => {
                self.page = Page::Progress;
                self.activity = Some(Activity::Run);
                let phase = if screen.starts_with("twrp") {
                    lapiswitch_core::recovery_flow::Phase::RecoveryWaiting
                } else {
                    lapiswitch_core::recovery_flow::Phase::EdlWaiting
                };
                self.progress.recovery_phase(phase);
                self.recovery_detail = Some(
                    if screen.starts_with("twrp") {
                        "TWRP ADB 연결을 기다립니다."
                    } else {
                        "EDL 연결을 기다립니다."
                    }
                    .into(),
                );
                let ticket = self
                    .wait_control
                    .begin_at(
                        phase,
                        std::time::Instant::now() - std::time::Duration::from_secs(181),
                    )
                    .unwrap();
                self.wait_ticket = Some(ticket);
                if screen.ends_with("confirm") {
                    self.stop_confirmation = Some(ticket);
                }
            }
            "rollback-error" => {
                self.page = Page::Result;
                let error = format!(
                    "{} boot: 펌웨어 1730764800 < 기기 1736035200 / vbmeta_system: 펌웨어 1730764800 < 기기 1736035200",
                    lapiswitch_core::prepare::ROLLBACK_REJECTED_MESSAGE
                );
                self.add_log(format!("작업 중단: {error}"));
                self.result = Some(Err(error));
                self.backup_path = Some("Demo/backup/region_change/TB520FU".into());
            }
            "result" => {
                self.page = Page::Result;
                self.result = Some(Ok(vec!["DEMO: 화면 검증용 결과입니다.".into()]));
                self.progress.finish(true);
            }
            "recovery" => {
                self.page = Page::Recovery;
                self.demo_pick(Picker::Backup);
                self.demo_pick(Picker::Loader);
            }
            _ => {
                self.region = None;
                self.folder = None;
                self.report = None;
                self.country = None;
            }
        }
        if matches!(screen, "result" | "progress") {
            self.comparison = vec![
                "롤백 비교 boot: 펌웨어 1775347200, 기기 1775347200".into(),
                "롤백 비교 vbmeta_system: 펌웨어 1775347200, 기기 1775347200".into(),
            ];
            self.add_log("DEMO · 실제 기기에 연결하지 않는 미리보기입니다.".into());
            self.add_log(
                "S5: 자동 EDL 재진입은 보통 1분 정도 걸립니다. 연결을 계속 감시합니다.".into(),
            );
        }
    }
    fn demo_tick(&mut self) {
        self.demo_ticks += 1;

        let index =
            usize::from(self.demo_ticks.saturating_sub(1)) * self.progress.flow.steps().len() / 18;
        let step = self.progress.flow.steps()[index.min(self.progress.flow.steps().len() - 1)];
        self.progress.step(step);
        self.add_log(format!(
            "DEMO · STEP {:02} 화면 미리보기",
            self.progress.current + 1
        ));
        if self.demo_ticks >= 18 {
            self.demo_ticks = 0;
            self.event(Event::Finished(Ok(vec![])));
        }
    }
}
fn demo_saved_state(stage: Stage) -> lapiswitch_core::state::RunState {
    let fw = demo_report(Region::Row);
    lapiswitch_core::state::RunState {
        version: 1,
        recovery: None,
        gpt_policy: lapiswitch_core::gpt::CURRENT_POLICY,
        run_id: "demo".into(),
        stage,
        target_region: Region::Row,
        firmware_dir: fw.image_dir,
        fingerprint: fw.fingerprint,
        indices: fw.indices,
        files: vec![],
        options: FlashOptions::new(true, Some("KR")).unwrap(),
        backup_dir: "Demo/backup".into(),
        backup_sha256: None,
        slot: None,
        persist_fingerprint: None,
        warnings: vec![],
    }
}
fn demo_report(region: Region) -> FirmwareReport {
    FirmwareReport {
        image_dir: "Demo/firmware/image".into(),
        loader: "Demo/loader.melf".into(),
        region,
        fingerprint: format!(
            "Lenovo/TB520FU_{}/lapis:15/DEMO/user/release-keys",
            region.token()
        ),
        key_class: lapiswitch_core::avb_info::KeyClass::Lenovo,
        indices: RollbackIndices {
            boot: 1775347200,
            vbmeta_system: 1775347200,
        },
        raw_xmls: vec![],
        patch_xmls: vec![],
        decrypted: 0,
        missing_images: vec![],
        lun4_gpt_in_package: true,
    }
}
pub fn run() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title(crate::brand::NAME)
        .theme(|_: &App| {
            Theme::custom(
                crate::brand::NAME,
                iced::theme::Palette {
                    primary: iced::Color::from_rgb8(36, 89, 198),
                    text: iced::Color::from_rgb8(25, 39, 62),
                    ..iced::theme::Palette::LIGHT
                },
            )
        })
        .settings(iced::Settings {
            id: Some("org.tb520fu.region-tool".into()),
            default_font: iced::Font::with_name("Noto Sans KR"),
            default_text_size: iced::Pixels(15.0),
            ..Default::default()
        })
        .font(include_bytes!("../fonts/NotoSansKR-Regular.subset.otf") as &[u8])
        .window(iced::window::Settings {
            size: if std::env::args().any(|a| a == "--compact") {
                iced::Size::new(900.0, 680.0)
            } else {
                iced::Size::new(1080.0, 790.0)
            },
            min_size: Some(iced::Size::new(900.0, 680.0)),
            icon: Some(crate::brand::window_icon()),
            ..Default::default()
        })
        .exit_on_close_request(false)
        .subscription(App::subscription)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disconnected_unauthorized_stale_or_missing_driver_blocks_conversion() {
        let mut app = App::base(true);
        app.seed_preview("confirm");
        app.agreed = true;
        assert!(app.can_start());
        for mode in [
            crate::device::Mode::None,
            crate::device::Mode::Unauthorized,
            crate::device::Mode::Sideload,
            crate::device::Mode::ServerBlocking,
        ] {
            app.device.snapshot.mode = mode;
            assert!(!app.can_start());
            let _ = app.update(Message::Start);
            assert_ne!(app.activity, Some(Activity::Run));
        }
        app.device.snapshot.mode = crate::device::Mode::Edl;
        app.device.checked = Some(std::time::Instant::now() - crate::device::MAX_AGE);
        assert!(!app.can_start());
        app.device.checked = Some(std::time::Instant::now());
        app.driver_status = Some(ltbox_device::driver::DriverStatus::KernelDriverMissing);
        assert!(!app.can_start());
    }
    #[test]
    fn start_revalidates_after_probe_releases_usb_and_disconnect_cancels_it() {
        for connected in [false, true] {
            let mut app = App::base(true);
            app.seed_preview("confirm");
            app.agreed = true;
            let id = app.device.begin().unwrap();
            app.deferred = Some(Message::StartVerified);
            app.event(Event::Device(id + 1, Ok(Default::default())));
            assert_eq!(app.device.in_flight, Some(id));
            assert!(app.deferred.is_some());
            app.event(Event::Device(
                id,
                Ok(crate::device::Snapshot {
                    mode: if connected {
                        crate::device::Mode::Adb
                    } else {
                        crate::device::Mode::None
                    },
                    ..Default::default()
                }),
            ));
            assert!(app.device.in_flight.is_none());
            assert!(app.deferred.is_none());
            assert_eq!(app.activity == Some(Activity::Run), connected);
        }
    }
    #[test]
    fn installer_waits_for_poll_and_cancellation_does_not_mark_drivers_present() {
        let mut app = App::base(true);
        app.seed_preview("drivers");
        let id = app.device.begin().unwrap();
        let _ = app.update(Message::InstallDrivers);
        assert!(app.deferred.is_some());
        assert_eq!(app.activity, None);
        app.event(Event::Device(id, Ok(Default::default())));
        // Demo substitutes a completed installer without running the OS helper.
        assert!(app.drivers_ready());
        for result in [Ok(false), Err("installer failed".into())] {
            app.activity = Some(Activity::Install);
            app.event(Event::DriverInstalled(result));
            assert!(!app.drivers_ready());
            assert!(app.driver_checking);
            app.event(Event::Drivers(Ok(
                ltbox_device::driver::DriverStatus::KernelDriverMissing,
            )));
            assert!(app.drivers_installable());
        }
        app.event(Event::Drivers(Err("check failed".into())));
        assert!(!app.drivers_ready());
        assert!(app.driver_error.is_some());
    }
    #[test]
    fn country_popup_cancel_keeps_committed_choice_and_apply_invalidates_consent() {
        let mut app = App::base(true);
        app.seed_preview("options");
        app.agreed = true;
        let _ = app.update(Message::CountryOpen);
        let _ = app.update(Message::Country("US".into()));
        assert_eq!(app.country.as_deref(), Some("KR"));
        let _ = app.update(Message::CountryCancel);
        assert_eq!(app.country.as_deref(), Some("KR"));
        let _ = app.update(Message::CountryOpen);
        let _ = app.update(Message::Country("CN".into()));
        let _ = app.update(Message::Country("ZZ".into()));
        let _ = app.update(Message::CountryApply);
        assert_eq!(app.country.as_deref(), Some("CN"));
        assert!(!app.country_open);
        assert!(!app.agreed);
    }
    #[test]
    fn wrong_model_disables_all_write_starts_and_a_swap_cancels_deferred_start() {
        let mut app = App::base(true);
        app.seed_preview("confirm");
        app.agreed = true;
        app.pending = Some(Pending {
            state: saved(Stage::S5),
            action: ResumeAction::Wait,
        });
        app.recovery_summary = Some("verified".into());
        app.loader = Some("loader.melf".into());
        for model in ["TB321FU", "TB520FUX", "another-device"] {
            app.device.snapshot.model = model.into();
            assert!(!app.can_start());
            assert!(!app.can_resume());
            assert!(!app.can_restore());
            for message in [
                Message::Start,
                Message::StartVerified,
                Message::ResumeStart,
                Message::RestoreStart,
            ] {
                let _ = app.update(message);
                assert_ne!(app.activity, Some(Activity::Run));
            }
        }
        app.pending = None;
        assert!(app.start_blocker().unwrap().contains("TB520FU"));
        app.device.snapshot.model = "lapis".into();
        assert!(app.can_start());
        let id = app.device.begin().unwrap();
        app.deferred = Some(Message::StartVerified);
        app.event(Event::Device(
            id,
            Ok(crate::device::Snapshot {
                mode: crate::device::Mode::Fastboot,
                model: "TB321FU".into(),
                serial: "other".into(),
                ..Default::default()
            }),
        ));
        assert!(!app.can_start());
        assert_ne!(app.activity, Some(Activity::Run));
        assert!(app.error.as_deref().unwrap().contains("지원하지 않는"));
    }

    #[test]
    fn unified_location_is_saved_only_from_settings_and_does_not_move_pending_runs() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::base(true);
        app.seed_preview("options");
        app.preferences_path = dir.path().join("preferences.json");
        let original_output = app.output.clone();
        let selected = dir.path().join("chosen-location");
        let _ = app.picked(Picker::Output, Some(selected.clone()));
        assert_eq!(app.output, original_output);
        app.pending = Some(Pending {
            state: saved(Stage::S5),
            action: ResumeAction::Wait,
        });
        let saved_backup = app.pending.as_ref().unwrap().state.backup_dir.clone();
        app.page = Page::Settings;
        app.agreed = true;
        // Exercise only preferences I/O; never dispatch device workers.
        app.demo = false;
        let _ = app.picked(Picker::Output, Some(selected.clone()));
        assert_eq!(app.output, selected);
        assert_eq!(transcript::log_dir(&app.output), selected.join("logs"));
        let builder = lapiswitch_core::backup::BackupBuilder::create(&app.output).unwrap();
        assert!(
            builder
                .path()
                .starts_with(selected.join("backup/region_change"))
        );
        assert_eq!(
            Preferences::load(&app.preferences_path).storage_root,
            Some(selected)
        );
        assert_eq!(app.pending.as_ref().unwrap().state.backup_dir, saved_backup);
        assert!(!app.agreed);
        let previous_output = app.output.clone();
        app.preferences_path = dir.path().to_owned(); // not a writable settings file
        let _ = app.picked(Picker::Output, Some(dir.path().join("rejected")));
        assert_eq!(app.output, previous_output);
        assert_eq!(app.preferences.storage_root, Some(previous_output));
        assert!(app.error.is_some());
    }

    #[test]
    fn edl_start_modal_needs_explicit_answer_and_supports_cancel() {
        for proceed in [false, true] {
            let mut app = App::base(true);
            app.seed_preview("edl-start-warning");
            let (tx, rx) = mpsc::channel();
            app.event(Event::ConfirmEdlStart(tx));
            for _ in 0..3 {
                let _ = app.update(Message::Tick);
            }
            assert!(rx.try_recv().is_err());
            let _ = app.update(Message::Close);
            let _ = app.update(Message::Stay);
            assert!(rx.try_recv().is_err());
            assert!(app.edl_confirmation.is_some());
            let _ = app.update(Message::ConfirmEdlStart(proceed));
            assert_eq!(rx.try_recv().unwrap(), proceed);
            assert!(app.edl_confirmation.is_none());
        }
    }
    #[test]
    fn firmware_mismatch_is_localized_in_both_directions_and_clears_old_report() {
        for language in [Language::Korean, Language::English] {
            for (firmware, target) in [(Region::Prc, Region::Row), (Region::Row, Region::Prc)] {
                let mut app = App::base(true);
                app.seed_preview("options");
                app.preferences.language = language;
                app.event(Event::FirmwareRegionMismatch { firmware, target });
                let error = app.error.as_ref().unwrap();
                assert!(error.contains(firmware.token()) && error.contains(target.token()));
                assert!(error.contains(if language == Language::Korean {
                    "선택한 리전"
                } else {
                    "selected region"
                }));
                assert!(app.report.is_none());
                assert!(app.activity.is_none());
            }
        }
    }
    fn saved(stage: Stage) -> lapiswitch_core::state::RunState {
        demo_saved_state(stage)
    }
    #[test]
    fn rollback_alert_separates_guidance_from_details_without_changing_log_text() {
        let heading = lapiswitch_core::prepare::ROLLBACK_REJECTED_MESSAGE;
        let details = "boot: 펌웨어 1730764800 < 기기 1736035200 / vbmeta_system: 펌웨어 1730764800 < 기기 1736035200";
        let log_error = format!("{heading} {details}");
        assert!(!log_error.contains('\n'));
        assert_eq!(
            view::result_error_text(&log_error),
            format!("{heading}\n{details}")
        );
        assert_eq!(view::result_error_text("other error"), "other error");
    }
    #[test]
    fn conversion_steps_follow_conversion_flow_and_settings_origin() {
        let mut app = App::base(true);
        for page in [Page::Dashboard, Page::Recovery] {
            app.page = page;
            assert!(!app.conversion_steps_visible());
            app.return_page = page;
            app.page = Page::Settings;
            assert!(!app.conversion_steps_visible());
        }
        for page in [
            Page::Region,
            Page::Firmware,
            Page::Options,
            Page::Confirm,
            Page::Resume,
        ] {
            app.page = page;
            assert!(app.conversion_steps_visible());
            app.return_page = page;
            app.page = Page::Settings;
            assert!(app.conversion_steps_visible());
        }
        for page in [Page::Progress, Page::Result] {
            app.page = page;
            app.run_kind = RunKind::Restore;
            assert!(!app.conversion_steps_visible());
            app.run_kind = RunKind::Convert;
            assert!(app.conversion_steps_visible());
            app.run_kind = RunKind::Resume;
            assert!(app.conversion_steps_visible());
        }
    }
    #[test]
    fn destructive_start_requires_firmware_country_and_consent() {
        let mut app = App::base(true);
        assert!(!app.can_start());
        app.seed_preview("confirm");
        app.country = None;
        assert!(!app.can_start());
        app.country = Some("KR".into());
        assert!(!app.can_start());
        app.agreed = true;
        assert!(app.can_start());
        app.report = None;
        assert!(!app.can_start());
    }
    #[test]
    fn disabled_start_explains_the_same_condition_that_blocks_execution() {
        let mut app = App::base(true);
        app.seed_preview("confirm");
        assert!(app.start_blocker().unwrap().contains("동의"));
        app.agreed = true;
        assert!(app.can_start());
        assert!(app.start_blocker().is_none());
        app.startup_error = Some("missing journal".into());
        assert!(app.start_blocker().unwrap().contains("작업 기록"));
        assert!(!app.can_start());
        app.startup_error = None;
        app.pending = Some(Pending {
            state: saved(Stage::S5),
            action: ResumeAction::Recovery,
        });
        assert!(app.start_blocker().is_none());
        assert!(app.can_start());
        app.pending = None;
        app.country = None;
        assert!(app.start_blocker().unwrap().contains("국가 코드"));
        assert!(!app.can_start());
    }
    #[test]
    fn conversion_always_erases_data_and_requires_country_choice() {
        let mut app = App::base(true);
        assert!(app.options().is_none());
        let _ = app.update(Message::CountryOpen);
        let _ = app.update(Message::Country("—".into()));
        let _ = app.update(Message::CountryApply);
        assert!(app.options().unwrap().wipe);
        assert!(app.options().unwrap().country.is_none());
        let _ = app.update(Message::CountryOpen);
        let _ = app.update(Message::Country("KR".into()));
        let _ = app.update(Message::CountryApply);
        assert!(app.options().unwrap().wipe);
        assert_eq!(app.options().unwrap().country.as_deref(), Some("KR"));
    }
    #[test]
    fn driver_choices_are_exclusive_and_changes_are_blocked_during_operations() {
        let mut app = App::base(true);
        let _ = app.update(Message::Kernel(false));
        assert!(!app.preferences.kernel);
        if ltbox_device::driver::kernel_mode_supported() {
            let _ = app.update(Message::Kernel(true));
            assert!(app.preferences.kernel);
            app.activity = Some(Activity::Run);
            let _ = app.update(Message::Kernel(false));
            assert!(app.preferences.kernel);
        }
    }

    #[test]
    fn transfer_ui_updates_do_not_append_log_rows_and_are_cleared_at_finish() {
        let mut app = App::base(true);
        app.add_log("start".into());
        for i in 0..100 {
            app.event(Event::Transfer(Some(format!("쓰기 {i}%"))));
        }
        assert_eq!(app.logs.len(), 1);
        assert_eq!(app.transfer_line.as_deref(), Some("쓰기 99%"));
        app.event(Event::Finished(Ok(vec![])));
        assert!(app.transfer_line.is_none());
    }
    #[test]
    fn running_job_freezes_navigation_region_and_settings() {
        let mut app = App::base(true);
        app.seed_preview("twrp-wait");
        for message in [
            Message::Recovery,
            Message::Region(Region::Prc),
            Message::Settings,
        ] {
            let _ = app.update(message);
        }
        assert_eq!(app.page, Page::Progress);
        assert_eq!(app.region, Some(Region::Row));
        assert!(app.options().unwrap().wipe);
    }
    #[test]
    fn region_has_no_default_and_must_be_chosen_first() {
        let mut app = App::base(true);
        assert_eq!(app.region, None);
        assert_eq!(app.page, Page::Dashboard);
        let _ = app.update(Message::Convert);
        let _ = app.update(Message::Next);
        assert_eq!(app.page, Page::Region);
        let _ = app.update(Message::Region(Region::Prc));
        assert_eq!(app.region, Some(Region::Prc));
        let _ = app.update(Message::Next);
        assert_eq!(app.page, Page::Firmware);
    }
    #[test]
    fn region_change_invalidates_checked_firmware_and_consent() {
        let mut app = App::base(true);
        app.seed_preview("confirm");
        app.agreed = true;
        let _ = app.update(Message::Region(Region::Prc));
        assert!(app.report.is_none());
        assert!(!app.agreed);
    }
    #[test]
    fn resume_gate_blocks_recovery_uncertain_or_corrupt_state() {
        let mut app = App::base(true);
        app.agreed = true;
        assert!(!app.can_resume());
        for action in [
            ResumeAction::Recovery,
            ResumeAction::ResetUncertain,
            ResumeAction::Restart,
        ] {
            let mut state = saved(Stage::S5);
            state.version = 2;
            state.recovery = Some(Default::default());
            app.pending = Some(Pending { state, action });
            assert!(!app.can_resume());
        }
        for action in [
            ResumeAction::Wait,
            ResumeAction::ResetThenWait,
            ResumeAction::Flash,
            ResumeAction::Finalize,
        ] {
            let mut state = saved(Stage::S5);
            state.version = 2;
            state.recovery = Some(Default::default());
            app.pending = Some(Pending { state, action });
            assert!(app.can_resume());
        }
        app.startup_error = Some("corrupt journal".into());
        assert!(!app.can_resume());
        app.demo_pick(Picker::Firmware);
        app.country = Some("KR".into());
        assert!(!app.can_start());
    }
    #[test]
    fn all_pages_and_both_languages_build_without_device() {
        for language in [Language::Korean, Language::English] {
            for screen in [
                "--demo",
                "firmware",
                "options",
                "confirm",
                "result",
                "recovery",
                "settings",
                "licenses",
                "licenses-search",
                "licenses-twrp",
                "dashboard",
                "drivers",
                "disconnected",
                "unauthorized",
                "country",
                "country-search",
                "adb-conflict",
                "software-fix",
                "software-fix-confirm",
                "rollback-error",
                "preflight",
                "firmware-mismatch",
                "edl-start-warning",
                "twrp-wait",
                "deviceinfo",
                "new-flash",
                "finalizing",
                "twrp-warning",
                "edl-warning",
                "resume",
                "archive-run",
                "confirm-pending",
                "twrp-stop",
                "edl-stop",
                "twrp-stop-confirm",
                "edl-stop-confirm",
            ] {
                let mut app = App::base(true);
                app.seed_preview(screen);
                app.preferences.language = language;
                let _view = app.view();
            }
        }
        let mut app = App::base(true);
        app.page = Page::Resume;
        app.pending = Some(Pending {
            state: saved(Stage::S6),
            action: ResumeAction::Flash,
        });
        let _view = app.view();
    }
    #[test]
    fn transcript_tail_is_bounded_and_failed_validation_cannot_advance() {
        let mut app = App::base(true);
        app.seed_preview("firmware");
        app.event(Event::Validated(Err("wrong region".into())));
        let _ = app.update(Message::Next);
        assert_eq!(app.page, Page::Firmware);
        for i in 0..1000 {
            app.add_log(i.to_string());
        }
        assert_eq!(app.logs.len(), 400);
        assert_eq!(app.logs.front().unwrap(), "600");
    }
    #[test]
    fn new_conversion_defaults_to_reset_and_legacy_missing_option_keeps() {
        let mut app = App::base(true);
        app.country = Some("KR".into());
        assert_eq!(
            app.options().unwrap().rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Reset
        );
        let _ = app.update(Message::Rollback(
            lapiswitch_core::deviceinfo::RollbackMode::Keep,
        ));
        assert_eq!(
            app.options().unwrap().rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Keep
        );
        app.activity = Some(Activity::Run);
        let _ = app.update(Message::Rollback(
            lapiswitch_core::deviceinfo::RollbackMode::Reset,
        ));
        assert_eq!(
            app.options().unwrap().rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Keep
        );
        let old: FlashOptions = serde_json::from_str(r#"{"wipe":true,"country":null}"#).unwrap();
        assert_eq!(
            old.rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Keep
        );
    }
    #[test]
    fn recovery_wait_and_write_keep_explicit_reboot_confirmation() {
        let mut app = App::base(true);
        app.activity = Some(Activity::Run);
        app.event(Event::RecoveryStatus("TWRP waiting".into()));
        app.event(Event::RecoveryStatus("DeviceInfo writing".into()));
        let (tx, rx) = mpsc::channel();
        app.event(Event::ConfirmRecoveryReboot(
            lapiswitch_core::recovery_flow::Transition::Edl,
            tx,
        ));

        assert!(app.reboot_confirmation.is_some());
        assert!(rx.try_recv().is_err());
        let _ = app.update(Message::ConfirmRecoveryReboot);
        assert_eq!(rx.try_recv(), Ok(()));
    }
    #[test]
    fn new_progress_survives_legacy_state_polling_and_confirmation_waits() {
        use lapiswitch_core::recovery_flow::{Phase, Transition};
        let mut app = App::base(true);
        app.reset_progress(RunKind::Convert);
        app.event(Event::Progress(Step::Backup));
        assert_eq!(app.progress.percent(), 4.0);
        app.event(Event::RecoveryPhase(Phase::Planned));

        assert_eq!(app.progress.percent(), 12.0);
        let (tx, _rx) = mpsc::channel();
        app.event(Event::ConfirmRecoveryReboot(Transition::Edl, tx));

        assert_eq!(app.progress.percent(), 12.0);
        assert!(app.reboot_confirmation.is_some());
        app.event(Event::Finished(Err("connection lost".into())));
        assert_eq!(app.progress.percent(), 12.0);
        app.reset_progress(RunKind::Restore);
        assert_eq!(app.progress.flow, Flow::Restore);
        assert_eq!(app.progress.percent(), 0.0);
        app.event(Event::Progress(Step::Originals));
        assert_eq!(app.progress.percent(), 50.0);
        app.event(Event::Finished(Ok(vec![])));
        assert_eq!(app.progress.percent(), 100.0);
    }
    #[test]
    fn custom_boot_records_never_enable_resume_even_if_old_actions_say_wait_or_flash() {
        let mut app = App::base(true);
        app.agreed = true;
        for action in [
            ResumeAction::ResetThenWait,
            ResumeAction::Wait,
            ResumeAction::Flash,
            ResumeAction::Finalize,
        ] {
            app.pending = Some(Pending {
                state: saved(Stage::S5),
                action,
            });
            assert!(!app.can_resume());
        }
    }
    #[test]
    fn resume_chooses_twrp_and_preserves_wait_progress() {
        use lapiswitch_core::recovery_flow::{Phase, Progress};
        let mut app = App::base(true);
        let mut state = demo_saved_state(Stage::S5);
        state.version = 2;
        state.recovery = Some(Progress::default());
        app.pending = Some(Pending {
            state,
            action: ResumeAction::Wait,
        });
        app.reset_progress(RunKind::Resume);
        assert_eq!(app.progress.flow, Flow::Recovery);
        app.event(Event::RecoveryPhase(Phase::EdlWaiting));
        app.event(Event::RecoveryStatus("waiting".into()));
        assert_eq!(app.progress.percent(), 14.0);
    }
    #[test]
    fn preparation_log_text_cannot_jump_to_the_later_edl_reconnection_step() {
        let mut app = App::base(true);
        app.reset_progress(RunKind::Convert);
        app.event(Event::Progress(Step::Device));
        app.add_log("S2: 시스템으로 재부팅한 뒤 ADB와 EDL 연결을 감시합니다.".into());
        assert_eq!(app.progress.percent(), 2.0);
        assert_eq!(
            app.progress.flow.steps()[app.progress.current],
            Step::Device
        );
    }
}
