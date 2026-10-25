mod archive;
mod conflicts;
mod country_view;
mod dashboard_view;
mod editor;
mod fluent;
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
    /// DeviceInfo edit setup: EDL loader and consent.
    Edit,
    /// DeviceInfo dump setup: EDL loader, save location and consent.
    Dump,
    /// DeviceInfo parsing setup: the file and its device's field map.
    Parse,
    /// The parsed file's fields.
    ParseView,
    Firmware,
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
    Edit,
    Dump,
    Resume,
    Restore,
}
#[derive(Debug, Clone, Copy)]
enum Picker {
    Firmware,
    Output,
    Backup,
    Loader,
    EditorFirmware,
    Export,
    DumpRoot,
    DeviceInfoFile,
}
#[derive(Debug, Clone)]
enum Message {
    Tick,
    Next,
    Back,
    Home,
    Convert,
    Edit,
    Dump,
    Parse,
    /// Show the parsed fields.
    ParseShow,
    Editor(editor::EditorMessage),
    Recovery,
    Settings,
    Resume,
    Pick(Picker),
    Picked(Picker, Option<PathBuf>),
    Country(String),
    Rollback(lapiswitch_core::deviceinfo::RollbackMode),
    RollbackOpen,
    RollbackCancel,
    RegionNoticeOpen,
    RegionNoticeClose,
    RollbackApply,
    /// The window's new width, used to shorten long paths.
    Resized(f32),
    /// Show a result folder, or a file in its folder, in the file manager.
    OpenFolder(PathBuf),
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
    /// Light, dark or follow Windows (Settings).
    Theme(crate::settings::ThemeChoice),
    /// Windows' current light or dark mode.
    SystemTheme(iced::theme::Mode),
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
    ConfirmUnrecognizedFirmware(bool),
    ConfirmRollbackReset(bool),
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
    rollback_open: bool,
    rollback_draft: lapiswitch_core::deviceinfo::RollbackMode,
    window_width: f32,
    recovery_transition: Option<lapiswitch_core::recovery_flow::Transition>,
    recovery_detail: Option<String>,
    country_open: bool,
    license_open: bool,
    region_notice_open: bool,
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
    /// The folder the last DeviceInfo dump was saved in.
    dump_folder: Option<PathBuf>,
    progress: RunProgress,
    /// Rollback indices read from the run log: (image, firmware, device).
    comparison: Vec<(String, String, String)>,
    wait_control: worker::WaitControl,
    wait_ticket: Option<worker::WaitTicket>,
    stop_confirmation: Option<worker::WaitTicket>,
    close_warning: bool,
    reboot_confirmation: Option<Sender<()>>,
    edl_confirmation: Option<Sender<bool>>,
    firmware_confirmation: Option<Sender<bool>>,
    /// Keep-mode rollback prompt: comparison details and the worker's answer.
    rollback_confirmation: Option<(String, Sender<bool>)>,
    /// The live DeviceInfo editor while the tablet waits in TWRP.
    editor: Option<editor::Editor>,
    /// The DeviceInfo file opened on the parsing page.
    parsed: Option<Parsed>,
    /// The country picker opened by Continue on the firmware page: Select
    /// moves on to the final check, Cancel stays.
    choosing_from_next: bool,
    /// Windows is in dark mode; used when the theme follows the system.
    system_dark: bool,
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
        let state_path =
            default_path.unwrap_or_else(|_| std::env::temp_dir().join("lapiSwitch/state.json"));
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
            rollback_open: false,
            rollback_draft: lapiswitch_core::deviceinfo::RollbackMode::Reset,
            window_width: if std::env::args().any(|a| a == "--compact") {
                900.0
            } else {
                1080.0
            },
            recovery_transition: None,
            recovery_detail: None,
            country_open: false,
            license_open: false,
            region_notice_open: false,
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
            dump_folder: None,
            progress: RunProgress::new(Flow::Recovery),
            comparison: vec![],
            wait_control: worker::WaitControl::default(),
            wait_ticket: None,
            stop_confirmation: None,
            close_warning: false,
            reboot_confirmation: None,
            edl_confirmation: None,
            firmware_confirmation: None,
            rollback_confirmation: None,
            editor: None,
            parsed: None,
            choosing_from_next: false,
            system_dark: false,
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
            app.system_dark = std::env::args().any(|arg| arg == "--dark");
            app.seed_preview(preview.trim_start_matches("--demo="));
            app.snapshot =
                std::env::args().find_map(|arg| arg.strip_prefix("--snapshot=").map(PathBuf::from));
        } else {
            app.refresh_pending();
            worker::drivers(app.tx.clone());
            app.driver_checking = true;
        }
        let theme = if app.demo {
            Task::none()
        } else {
            iced::system::theme().map(Message::SystemTheme)
        };
        (app, theme)
    }
    /// The effective dark mode: Settings, or Windows when it follows the system.
    fn dark(&self) -> bool {
        match self.preferences.theme {
            crate::settings::ThemeChoice::System => self.system_dark,
            crate::settings::ThemeChoice::Light => false,
            crate::settings::ThemeChoice::Dark => true,
        }
    }
    /// The text of a `lang/<code>.json` key in the chosen language.
    fn tr(&self, key: &'static str) -> &'static str {
        self.preferences.language.tr(key)
    }
    fn busy(&self) -> bool {
        self.activity.is_some() || self.adb_killing || self.fix.closing
    }
    fn options(&self) -> Option<FlashOptions> {
        self.country.as_ref()?;
        FlashOptions::new(self.country.as_deref().filter(|v| *v != "—")).ok()
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
            Page::Firmware | Page::Confirm | Page::Resume => true,
            Page::Progress | Page::Result => !matches!(
                self.run_kind,
                RunKind::Restore | RunKind::Edit | RunKind::Dump
            ),
            _ => false,
        }
    }
    fn fastboot_start_text(&self) -> &'static str {
        self.tr("app_conversion_cannot_start_from_fastboot_boot")
    }
    /// The checked firmware names another model than the connected device.
    /// EDL reports no model, so the run identifies the device itself then.
    fn firmware_model_differs(&self) -> bool {
        match (&self.report, self.device.supported()) {
            (Some(report), Some(device)) => report.device != device,
            _ => false,
        }
    }
    fn can_start(&self) -> bool {
        self.start_blocker().is_none()
    }
    fn start_blocker(&self) -> Option<&'static str> {
        self.start_issue().map(|(_, reason)| reason)
    }
    /// Why the run cannot start. A problem with the device, drivers or
    /// firmware carries the severity of the InfoBar shown above the
    /// buttons; a step not done yet (`None`) is only a quiet hint.
    fn start_issue(&self) -> Option<(Option<fluent::Severity>, &'static str)> {
        use fluent::Severity::{Error, Warning};
        let issue = if self.busy() {
            (None, self.tr("app_wait_for_the_current_operation_to"))
        } else if self.deferred.is_some() {
            (None, self.tr("app_rechecking_the_device_before_starting"))
        } else if self.startup_error.is_some() {
            (Some(Error), self.tr("app_the_saved_run_cannot_be_checked"))
        } else if !self.device.reachable() {
            (
                Some(Warning),
                self.tr("app_the_device_connection_could_not_be"),
            )
        } else if self.device.unsupported_model() {
            (
                Some(Error),
                self.tr("app_unsupported_device_connect_a_supported_device"),
            )
        } else if self.device.dump_only() && self.page != Page::Dump {
            (
                Some(Error),
                self.tr("app_this_device_supports_deviceinfo_dumps_only"),
            )
        } else if self.device.fastboot() {
            (Some(Warning), self.fastboot_start_text())
        } else if !self.device.startable() {
            (
                Some(Warning),
                self.tr("app_the_device_model_is_unknown_check"),
            )
        } else if !self.drivers_ready() {
            (
                Some(Warning),
                self.tr("app_check_or_install_the_required_drivers"),
            )
        } else if self.page == Page::Dump {
            if self.loader.is_none() {
                (None, self.tr("app_select_the_edl_loader_file"))
            } else if self.preferences.dump_root.is_none() {
                (None, self.tr("app_choose_where_to_save_the_dump"))
            } else if !self.agreed {
                (None, self.tr("app_read_the_deviceinfo_dump_notice_and"))
            } else {
                return None;
            }
        } else if self.page == Page::Edit {
            if self.loader.is_none() {
                (None, self.tr("app_select_the_edl_loader_file"))
            } else if !self.agreed {
                (None, self.tr("app_read_the_deviceinfo_edit_notice_and"))
            } else {
                return None;
            }
        } else if self.folder.is_none()
            || !self
                .report
                .as_ref()
                .is_some_and(|f| Some(f.region) == self.region)
        {
            (None, self.tr("app_choose_a_firmware_folder_and_complete"))
        } else if self.firmware_model_differs() {
            (Some(Error), self.tr("app_the_firmware_is_for_a_different"))
        } else if self.options().is_none() {
            (None, self.tr("app_choose_a_country_code_or_do"))
        } else if !self.agreed {
            (None, self.tr("app_read_the_data_erasure_notice_and"))
        } else {
            return None;
        };
        Some(issue)
    }
    fn can_restore(&self) -> bool {
        self.restore_issue().is_none()
    }
    /// Why the original restore cannot start, shown like `start_issue`. It
    /// starts only from TWRP (ADB Recovery), where DeviceInfo is judged
    /// first, or from EDL.
    fn restore_issue(&self) -> Option<(Option<fluent::Severity>, &'static str)> {
        use fluent::Severity::{Error, Warning};
        let issue = if self.busy() {
            (None, self.tr("app_wait_for_the_current_operation_to"))
        } else if !self.device.reachable() {
            (
                Some(Warning),
                self.tr("app_the_device_connection_could_not_be"),
            )
        } else if self.device.unsupported_model() {
            (
                Some(Error),
                self.tr("app_unsupported_device_connect_a_supported_device"),
            )
        } else if !matches!(
            self.device.snapshot.mode,
            crate::device::Mode::Recovery | crate::device::Mode::Edl
        ) {
            (
                Some(Warning),
                self.tr("app_original_restore_starts_only_from_twrp"),
            )
        } else if !self.drivers_ready() {
            (
                Some(Warning),
                self.tr("app_check_or_install_the_required_drivers"),
            )
        } else if self.recovery_summary.is_none() {
            (None, self.tr("app_choose_a_backup_folder_and_complete"))
        } else if self.loader.is_none() {
            (None, self.tr("app_select_the_edl_loader_file"))
        } else if !self.agreed {
            (None, self.tr("app_read_the_restore_notice_and_select"))
        } else {
            return None;
        };
        Some(issue)
    }
    fn can_resume(&self) -> bool {
        !self.busy()
            && !self.device.unsupported_model()
            && self.startup_error.is_none()
            && self.agreed
            && self.pending.as_ref().is_some_and(|p| {
                matches!(
                    p.action,
                    ResumeAction::Wait | ResumeAction::Flash | ResumeAction::Finalize
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
            RunKind::Dump => Flow::Dump,
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
        self.firmware_confirmation = None;
        self.rollback_confirmation = None;
        self.editor = None;
        self.dump_folder = None;
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
            Message::Resized(width) => self.window_width = width,
            Message::OpenFolder(path) => {
                if !self.demo
                    && let Err(error) = open_in_file_manager(&path)
                {
                    self.error = Some(format!("{}: {error}", path.display()));
                }
            }
            Message::FocusMove(reverse) => {
                return if reverse {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            // Previews keep the theme chosen on the command line.
            Message::SystemTheme(mode) if !self.demo => {
                self.system_dark = mode == iced::theme::Mode::Dark;
            }
            Message::SystemTheme(_) => {}
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
                    && self.rollback_confirmation.is_none()
                    && self.editor.is_none()
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
                    Ok(()) => self.notice = Some(self.tr("app_log_saved").into()),
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
            Message::ConfirmUnrecognizedFirmware(proceed) => {
                if let Some(tx) = self.firmware_confirmation.take() {
                    let _ = tx.send(proceed);
                }
            }
            Message::ConfirmRollbackReset(reset) => {
                if let Some((_, tx)) = self.rollback_confirmation.take() {
                    let _ = tx.send(reset);
                }
            }
            // The editor is open only during a run; otherwise the parsing
            // page's viewer takes the search and the persistent sets.
            Message::Editor(message) if self.editor.is_none() => {
                if let Some((_, view)) = self.parsed.as_mut().and_then(|p| p.view.as_mut()) {
                    view.view_update(message);
                }
            }
            Message::Editor(message) => self.editor_update(message),
            Message::ParseShow if self.parsed.as_ref().is_some_and(|p| p.view.is_some()) => {
                self.page = Page::ParseView;
            }
            Message::ConfirmRecoveryReboot => {
                if let Some(confirmation) = self.reboot_confirmation.take() {
                    let delivered = confirmation.send(());
                    if !self.demo && delivered.is_err() {
                        self.error = Some(
                            self.tr("app_could_not_deliver_confirmation_check_the")
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
                            self.tr("app_adb_server_stop_requested_rechecking_the")
                                .into(),
                        )
                    }
                    Err(error) => {
                        self.error = Some(format!(
                            "{}: {error}",
                            self.tr("app_could_not_stop_adb_server")
                        ))
                    }
                }
                self.device.snapshot = Default::default();
                self.device.checked = None;
                self.device.next = std::time::Instant::now();
            }
            Message::CountryOpen
            | Message::CountrySearch(_)
            | Message::Country(_)
            | Message::CountryCancel
            | Message::CountryApply
                if self.editor.is_some() =>
            {
                self.editor_country(message)
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
            _ if self.rollback_open
                && !matches!(
                    message,
                    Message::Rollback(_) | Message::RollbackApply | Message::RollbackCancel
                ) => {}
            _ if self.archive_confirmation.is_some()
                && !matches!(
                    message,
                    Message::CancelArchiveRun | Message::ConfirmArchiveRun
                ) => {}
            Message::CancelArchiveRun => self.archive_confirmation = None,
            Message::ConfirmArchiveRun => return self.confirm_archive_run(),
            Message::Next => {
                self.error = None;
                self.page = match self.page {
                    Page::Firmware if self.report.is_some() => {
                        // Choose the country code, then the rollback
                        // handling; cancelling either stays here.
                        self.country_open = true;
                        self.country_search.clear();
                        self.country_draft = self.country.clone();
                        self.choosing_from_next = true;
                        Page::Firmware
                    }
                    other => other,
                };
            }
            Message::Back => {
                self.error = None;
                self.agreed = false;
                self.page = match self.page {
                    Page::Firmware => Page::Dashboard,
                    Page::Confirm => Page::Firmware,
                    Page::Settings => self.return_page,
                    Page::ParseView => Page::Parse,
                    _ => Page::Dashboard,
                };
            }
            Message::Home => {
                self.refresh_pending();
                self.page = Page::Dashboard;
                self.agreed = false;
                self.error = None;
            }
            Message::Convert if !self.device.unsupported_model() && !self.device.dump_only() => {
                self.page = Page::Firmware;
                self.agreed = false;
                self.error = None;
            }
            Message::Edit if !self.device.unsupported_model() && !self.device.dump_only() => {
                self.page = Page::Edit;
                self.agreed = false;
                self.error = None;
            }
            Message::Dump if !self.device.unsupported_model() => {
                self.page = Page::Dump;
                self.agreed = false;
                self.error = None;
            }
            Message::Parse => {
                self.page = Page::Parse;
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
                self.choosing_from_next = false;
            }
            Message::CountryApply if self.country_open && self.country_draft.is_some() => {
                self.country = self.country_draft.take();
                self.country_open = false;
                self.agreed = false;
                if std::mem::take(&mut self.choosing_from_next) {
                    self.page = Page::Confirm;
                }
            }
            Message::RollbackOpen if !self.busy() => {
                self.rollback_open = true;
                self.rollback_draft = self.rollback_mode;
            }
            Message::Rollback(mode) if self.rollback_open && !self.busy() => {
                self.rollback_draft = mode;
            }
            Message::RollbackCancel => self.rollback_open = false,
            Message::RegionNoticeOpen => self.region_notice_open = true,
            Message::RegionNoticeClose => self.region_notice_open = false,
            Message::RollbackApply if self.rollback_open && !self.busy() => {
                self.rollback_mode = self.rollback_draft;
                self.rollback_open = false;
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
            Message::Theme(theme) => {
                self.preferences.theme = theme;
                self.save_preferences();
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
                            Picker::Firmware
                            | Picker::Output
                            | Picker::Backup
                            | Picker::EditorFirmware
                            | Picker::DumpRoot => {
                                dialog.pick_folder().await.map(|f| f.path().to_owned())
                            }
                            Picker::Loader => dialog
                                .add_filter("Firehose loader", &["melf", "elf", "mbn"])
                                .pick_file()
                                .await
                                .map(|f| f.path().to_owned()),
                            Picker::DeviceInfoFile => dialog
                                .add_filter("DeviceInfo", &["bin"])
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
                self.folder = Some(path.clone());
                self.report = None;
                self.region = None;
                self.agreed = false;
                self.activity = Some(Activity::Inspect);
                worker::validate(self.tx.clone(), path);
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
            Picker::DumpRoot => {
                let previous = self.preferences.dump_root.replace(path);
                if self.save_preferences() {
                    self.agreed = false;
                } else {
                    self.preferences.dump_root = previous;
                }
            }
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
            Picker::EditorFirmware => self.editor_firmware_picked(Some(path)),
            Picker::DeviceInfoFile => {
                let opened = lapiswitch_core::deviceinfo::parse::open(&path);
                self.parse_opened(opened);
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
            Event::ConfirmUnrecognizedFirmware(tx) => {
                if self.activity == Some(Activity::Run)
                    && matches!(
                        self.run_kind,
                        RunKind::Convert | RunKind::Edit | RunKind::Dump
                    )
                {
                    self.firmware_confirmation = Some(tx);
                }
            }
            Event::EditDeviceInfo(layout, before, tx) => {
                if self.activity == Some(Activity::Run) && self.run_kind != RunKind::Restore {
                    self.editor = Some(editor::Editor::new(layout, before, tx));
                }
            }
            Event::EditorFirmware(result) => self.editor_firmware_checked(result),
            Event::ConfirmRollbackReset(details, tx) => {
                if self.activity == Some(Activity::Run) && self.run_kind != RunKind::Restore {
                    self.rollback_confirmation = Some((details, tx));
                }
            }
            Event::ConfirmEdlStart(tx) => {
                if self.activity == Some(Activity::Run) && self.run_kind != RunKind::Restore {
                    self.edl_confirmation = Some(tx);
                }
            }
            Event::Validated(result) => {
                self.activity = None;
                match result {
                    Ok(report) => {
                        // The package's vendor_boot DTB decides the target.
                        self.region = Some(report.region);
                        self.report = Some(report);
                    }
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
                        self.notice = Some(
                            self.tr("app_installer_completed_rechecking_drivers_reconnect_usb")
                                .into(),
                        );
                    }
                    Ok(false) => {
                        self.notice = Some(
                            self.tr("app_driver_installation_was_cancelled_use_install")
                                .into(),
                        )
                    }
                    Err(error) => {
                        self.error = Some(format!(
                            "{}: {error}",
                            self.tr("app_driver_installation_failed")
                        ))
                    }
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
            Event::Dumped(folder) => self.dump_folder = Some(folder),
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
                self.firmware_confirmation = None;
                self.rollback_confirmation = None;
                self.editor = None;
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
        if self.comparison.len() < 2
            && let Some(values) = lapiswitch_core::i18n::parse("core_rollback_compare", &line)
        {
            let value = |name: &str| {
                values
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.clone())
                    .unwrap_or_default()
            };
            self.comparison
                .push((value("partition"), value("image"), value("floor")));
        }
        if self.logs.len() >= 400 {
            self.logs.pop_front();
        }
        self.logs.push_back(line);
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(worker::POLL_INTERVAL).map(|_| Message::Tick),
            iced::system::theme_changes().map(Message::SystemTheme),
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                    modifiers,
                    ..
                }) if status == iced::event::Status::Ignored => {
                    Some(Message::FocusMove(modifiers.shift()))
                }
                iced::Event::Window(iced::window::Event::CloseRequested) => Some(Message::Close),
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::Resized(size.width))
                }
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
                let region = self.region.unwrap_or(Region::Row);
                self.region = Some(region);
                self.folder = Some(
                    r"D:\TB520FU_ROW_OPEN_USER_Q29300.0_A17_ZUI_18.5.10.037_ST_260929\image".into(),
                );
                self.report = Some(demo_report(region));
            }
            Picker::Backup => {
                self.recovery_folder = Some("Demo/backup/TB520FU".into());
                self.recovery_summary = Some("TB520FU · slot a · ROW · 5 files (DEMO)".into());
            }
            Picker::Loader => self.loader = Some("Demo/loader.melf".into()),
            Picker::DumpRoot => self.preferences.dump_root = Some("Demo/dumps".into()),
            Picker::DeviceInfoFile => {
                let layout = self
                    .device
                    .supported()
                    .and_then(|device| device.layout())
                    .expect("demo device map");
                let opened = lapiswitch_core::deviceinfo::parse::from_bytes(
                    std::path::Path::new(
                        r"Demo\dumps\Deviceinfo_dump_TB520FU_1792800000\deviceinfo.bin",
                    ),
                    demo_deviceinfo(layout),
                );
                self.parse_opened(opened);
            }
            Picker::EditorFirmware => {
                if let Some(editor) = self.editor.as_mut()
                    && let Some(region) = editor.target_region()
                {
                    editor.firmware_dir = Some("Demo/firmware/image".into());
                    editor.checking = false;
                    editor.firmware = Some(Ok(demo_report(region)));
                }
            }
            Picker::Output => {
                let _ = self.picked(Picker::Output, Some("Demo/lapiSwitch".into()));
            }
            Picker::Export => self.notice = Some(self.tr("app_preview_does_not_save_files").into()),
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
            "dashboard-twrp" => {
                self.page = Page::Dashboard;
                self.device.snapshot.mode = crate::device::Mode::Recovery;
                self.device.snapshot.android = "17".into();
                self.device.snapshot.slot = "_a".into();
                self.device.snapshot.firmware =
                    "twrp_lapis-eng 99.87.36 BP2A.250605.031.A2 1791987058 test-keys".into();
            }
            "dashboard-dump-only" | "dashboard-unsupported" => {
                self.page = Page::Dashboard;
                let (model, name) = if screen == "dashboard-dump-only" {
                    ("TB522FU", "Lenovo Legion Y900")
                } else {
                    ("TB321FU", "Unsupported tablet")
                };
                self.device.snapshot.model = model.into();
                self.device.snapshot.name = name.into();
            }
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
                self.page = Page::Firmware;
                self.country_open = true;
                self.country_draft = Some("KR".into());
            }
            "country-search" => {
                self.page = Page::Firmware;
                self.country_open = true;
                self.country_draft = Some("KR".into());
                self.country_search = "ko".into();
            }
            "region" => {
                self.page = Page::Firmware;
                self.region = None;
                self.report = None;
                self.folder = None;
                self.country = None;
            }
            "firmware" => self.page = Page::Firmware,
            "options" => self.page = Page::Confirm,
            "rollback" => {
                self.page = Page::Confirm;
                let _ = self.update(Message::RollbackOpen);
            }
            "region-notice" => {
                self.page = Page::Confirm;
                let _ = self.update(Message::RegionNoticeOpen);
            }
            "options-unselected" => {
                self.page = Page::Confirm;
                self.country = None;
            }
            "wrong-device" => {
                self.page = Page::Confirm;
                self.device.snapshot.model = "TB321FU".into();
                self.device.snapshot.name = "Unsupported tablet".into();
                self.agreed = true;
            }
            "model-mismatch" => {
                self.page = Page::Confirm;
                self.device.snapshot.model = "TB522FU".into();
                self.agreed = true;
            }
            "resume" | "confirm-pending" | "archive-run" => {
                self.page = if screen == "resume" {
                    Page::Resume
                } else {
                    Page::Confirm
                };
                self.agreed = screen != "resume";
                let state = demo_saved_state(Stage::S5);
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
            "options-country" => {
                self.page = Page::Firmware;
                let _ = self.update(Message::Next);
            }
            "custom-firmware-warning" => {
                self.reset_progress(RunKind::Convert);
                self.progress.step(Step::Backup);
                let (tx, _) = mpsc::channel();
                self.firmware_confirmation = Some(tx);
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
            "parse" | "parse-view" | "parse-view-long" | "parse-view-rollback" => {
                self.page = Page::Parse;
                self.demo_pick(Picker::DeviceInfoFile);
                if screen != "parse" {
                    self.page = Page::ParseView;
                }
                // The longest value: the 2048-byte public key.
                if screen == "parse-view-long" {
                    let _ = self.update(Message::Editor(editor::EditorMessage::Query(
                        "user_public_key".into(),
                    )));
                }
                if screen == "parse-view-rollback" {
                    let _ = self.update(Message::Editor(editor::EditorMessage::Query(
                        "rollback".into(),
                    )));
                }
            }
            "deviceinfo-edit" => {
                self.page = Page::Edit;
                self.loader = Some("Demo/loader.melf".into());
                self.agreed = true;
            }
            "dump" | "dump-only" => {
                self.page = Page::Dump;
                self.loader = Some("Demo/loader.melf".into());
                self.preferences.dump_root = Some("Demo/dumps".into());
                if screen == "dump-only" {
                    self.device.snapshot.model = "TB522FU".into();
                    self.device.snapshot.name = "Lenovo Legion Y900".into();
                }
            }
            "dump-result" => {
                self.reset_progress(RunKind::Dump);
                self.dump_folder = Some(
                    r"C:\TB520FU_Flashing Tool\TEST\DeviceInfo dumps\Deviceinfo_dump_TB520FU_1792000000"
                        .into(),
                );
                self.backup_path = Some(
                    r"C:\Users\LAPTOP\AppData\Local\lapiSwitch\backup\DeviceInfo_Dump\TB520FU_2026-10-23_120000"
                        .into(),
                );
                self.log_path = Some(
                    r"C:\Users\LAPTOP\AppData\Local\lapiSwitch\logs\lapiSwitch_2026-10-23_120000.log".into(),
                );
                self.event(Event::Finished(Ok(vec![])));
            }
            "deviceinfo-editor"
            | "deviceinfo-editor-custom"
            | "deviceinfo-editor-rollback"
            | "deviceinfo-editor-review"
            | "deviceinfo-editor-notices"
            | "deviceinfo-editor-custom-notices"
            | "deviceinfo-editor-country"
            | "deviceinfo-editor-persistent"
            | "deviceinfo-editor-long"
            | "deviceinfo-editor-rollback-open" => {
                self.reset_progress(RunKind::Edit);
                self.progress.step(Step::DeviceInfo);
                let (tx, _) = mpsc::channel();
                let layout = self
                    .device
                    .supported()
                    .and_then(|device| device.layout())
                    .expect("demo device map");
                let mut bytes = demo_deviceinfo(layout);
                if screen == "deviceinfo-editor-persistent" {
                    // Sample sets so the summaries show used and unused values.
                    for (key, value) in [
                        ("persistent[0].valid", "1"),
                        ("persistent[0].name_length", "23"),
                        ("persistent[0].name", "avb.managed_verity_mode"),
                        ("persistent[0].value_length", "4"),
                        ("persistent[1].valid", "1"),
                        ("persistent[1].name_length", "13"),
                        ("persistent[1].name", "avb.boot_lock"),
                        ("persistent[1].value_length", "32"),
                    ] {
                        if let Ok(field) = layout.field(key)
                            && let Ok(raw) = lapiswitch_core::deviceinfo::map::encode(&field, value)
                        {
                            bytes[field.range()].copy_from_slice(&raw);
                        }
                    }
                }
                let mut editor = editor::Editor::new(layout, bytes, tx);
                if matches!(
                    screen,
                    "deviceinfo-editor-custom"
                        | "deviceinfo-editor-custom-notices"
                        | "deviceinfo-editor-persistent"
                        | "deviceinfo-editor-long"
                ) {
                    editor.tab = editor::Tab::Advanced;
                }
                self.editor = Some(editor);
                if screen == "deviceinfo-editor-rollback" {
                    self.editor_update(editor::EditorMessage::Input(
                        "is_unlocked".into(),
                        "1".into(),
                    ));
                    self.editor_update(editor::EditorMessage::Rollback(
                        editor::RollbackChoice::Custom,
                    ));
                    self.editor_update(editor::EditorMessage::Input(
                        "rollback_index[3]".into(),
                        "20".into(),
                    ));
                }
                if matches!(
                    screen,
                    "deviceinfo-editor-notices"
                        | "deviceinfo-editor-custom-notices"
                        | "deviceinfo-editor-country"
                ) {
                    self.editor_update(editor::EditorMessage::Input("region".into(), "ROW".into()));
                    self.demo_pick(Picker::EditorFirmware);
                    let unlocked = if screen != "deviceinfo-editor-custom-notices" {
                        "1"
                    } else {
                        ""
                    };
                    self.editor_update(editor::EditorMessage::Input(
                        "is_unlocked".into(),
                        unlocked.into(),
                    ));
                }
                if screen == "deviceinfo-editor-long" {
                    self.editor_update(editor::EditorMessage::Query("user_public_key".into()));
                }
                if screen == "deviceinfo-editor-rollback-open" {
                    self.editor_update(editor::EditorMessage::RollbackOpen);
                }
                if screen == "deviceinfo-editor-country" {
                    let _ = self.update(Message::CountryOpen);
                }
                if screen == "deviceinfo-editor-review" {
                    self.editor_update(editor::EditorMessage::Input("region".into(), "ROW".into()));
                    self.demo_pick(Picker::EditorFirmware);
                    self.editor_update(editor::EditorMessage::Review(true));
                }
            }
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
                        std::time::Instant::now() - std::time::Duration::from_secs(31),
                    )
                    .unwrap();
                self.wait_ticket = Some(ticket);
                if screen.ends_with("confirm") {
                    self.stop_confirmation = Some(ticket);
                }
            }
            "rollback-confirm" => {
                self.page = Page::Progress;
                self.activity = Some(Activity::Run);
                self.progress = RunProgress::new(Flow::Recovery);
                self.progress.step(Step::DeviceInfo);
                let (tx, _) = mpsc::channel();
                self.rollback_confirmation = Some((
                    "boot: 펌웨어 1730764800 < 기기 1736035200 / vbmeta_system: 펌웨어 1730764800 < 기기 1736035200".into(),
                    tx,
                ));
            }
            "flash-result" => {
                self.reset_progress(RunKind::Convert);
                self.backup_path = Some(
                    r"C:\Users\LAPTOP\AppData\Local\lapiSwitch\backup\firmware_flashing\TB520FU_2026-10-23_120000"
                        .into(),
                );
                self.log_path = Some(
                    r"C:\Users\LAPTOP\AppData\Local\lapiSwitch\logs\lapiSwitch_2026-10-23_120000.log".into(),
                );
                self.event(Event::Finished(Ok(vec![])));
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
            self.comparison = ["boot", "vbmeta_system"]
                .map(|image| (image.into(), "1775347200".into(), "1775347200".into()))
                .into();
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
/// Synthetic DeviceInfo shaped like a device's map (no real dump data).
/// A DeviceInfo file opened for parsing and its viewer.
struct Parsed {
    opened: lapiswitch_core::deviceinfo::parse::Opened,
    /// The field map in use and how it was chosen; `None` until one is.
    view: Option<(lapiswitch_core::deviceinfo::parse::Source, editor::Editor)>,
}

impl App {
    fn parse_opened(&mut self, opened: Result<lapiswitch_core::deviceinfo::parse::Opened, String>) {
        match opened {
            Ok(opened) => {
                let view = opened.layout.map(|(layout, source)| {
                    (source, editor::Editor::viewer(layout, opened.bytes.clone()))
                });
                self.parsed = Some(Parsed { opened, view });
            }
            Err(error) => {
                self.parsed = None;
                self.error = Some(error);
            }
        }
    }
}

/// The demos' DeviceInfo: a real file from `--demo-deviceinfo=PATH` when it
/// is a complete buffer for `layout`, else a synthetic one.
fn demo_deviceinfo(layout: lapiswitch_core::deviceinfo::map::Layout) -> Vec<u8> {
    use lapiswitch_core::deviceinfo::SHARED_SIZE;
    if let Some(bytes) = std::env::args()
        .find_map(|arg| arg.strip_prefix("--demo-deviceinfo=").map(PathBuf::from))
        .and_then(|path| std::fs::read(path).ok())
        .filter(|bytes| layout.check_buffer(bytes).is_ok())
    {
        return bytes;
    }
    let mut b = vec![0u8; SHARED_SIZE];
    b[..13].copy_from_slice(b"ANDROID-BOOT!");
    b[0xCC8..0xCD0].copy_from_slice(b"deadbeef");
    let region = layout.region_offset();
    b[region..region + 3].copy_from_slice(b"PRC");
    b[region + 0x10..region + 0x18].copy_from_slice(b"beefdead");
    b[layout.rollback_start() + 8] = 1;
    b
}
/// Opens a folder in the file manager; a file is shown selected in its
/// folder.
fn open_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    use std::process::Command;
    let file = path.is_file();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("explorer");
        if file {
            command.raw_arg(format!("/select,\"{}\"", path.display()));
        } else {
            command.arg(path);
        }
        command.spawn().map(drop)
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        if file {
            command.arg("-R");
        }
        command.arg(path).spawn().map(drop)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let folder = if file {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        Command::new("xdg-open").arg(folder).spawn().map(drop)
    }
}
fn demo_saved_state(stage: Stage) -> lapiswitch_core::state::RunState {
    let fw = demo_report(Region::Row);
    lapiswitch_core::state::RunState {
        version: lapiswitch_core::state::STATE_VERSION,
        recovery: lapiswitch_core::recovery_flow::Progress {
            phase: lapiswitch_core::recovery_flow::Phase::RecoveryWaiting,
            attempt: None,
        },
        run_id: "demo".into(),
        stage,
        request: lapiswitch_core::state::Request::Convert {
            target: Region::Row,
            rollback_mode: lapiswitch_core::deviceinfo::RollbackMode::Reset,
        },
        device: Some(fw.device),
        loader: lapiswitch_core::state::SealedFile {
            path: fw.loader.clone(),
            size: 0,
            sha256: String::new(),
        },
        firmware: Some(lapiswitch_core::state::FirmwareBinding {
            region: Region::Row,
            firmware_dir: fw.image_dir,
            fingerprint: fw.fingerprint,
            indices: fw.indices,
            files: vec![],
        }),
        options: FlashOptions::new(Some("KR")).unwrap(),
        backup_dir: "Demo/backup".into(),
        backup_sha256: None,
        slot: None,
        persist_fingerprint: None,
        warnings: vec![],
        dump_root: None,
    }
}
fn demo_report(region: Region) -> FirmwareReport {
    FirmwareReport {
        device: lapiswitch_core::device::Device::find("TB520FU").expect("demo device"),
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
        .theme(|app: &App| fluent::iced_theme(app.dark()))
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
    fn a_parsed_file_is_detected_and_its_fields_shown_on_confirm() {
        use lapiswitch_core::deviceinfo::parse::Source;
        let mut app = App::base(true);
        app.seed_preview("parse");
        assert_eq!(app.page, Page::Parse);
        let view = |app: &App| {
            let (source, view) = app.parsed.as_ref().unwrap().view.as_ref().unwrap();
            (
                *source,
                view.layout.model(),
                view.query.clone(),
                view.view_only,
            )
        };
        assert_eq!(
            view(&app),
            (Source::Detected, "TB520FU", String::new(), true)
        );
        // Confirm shows the fields; Back returns to the file.
        let _ = app.update(Message::ParseShow);
        assert_eq!(app.page, Page::ParseView);
        let _ = app.update(Message::Back);
        assert_eq!(app.page, Page::Parse);
        let _ = app.update(Message::Editor(editor::EditorMessage::Query(
            "region".into(),
        )));
        assert_eq!(view(&app).2, "region");
        // Parsing needs no device and changes nothing.
        assert!(app.editor.is_none());
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
        assert!(app.start_blocker().unwrap().contains("지원 기기"));
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
        let builder = lapiswitch_core::backup::BackupBuilder::create(
            &app.output,
            lapiswitch_core::state::Request::Edit,
            None,
        )
        .unwrap();
        assert!(
            builder
                .path()
                .starts_with(selected.join("backup/DeviceInfo_Write"))
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
    fn saved(stage: Stage) -> lapiswitch_core::state::RunState {
        demo_saved_state(stage)
    }
    #[test]
    fn the_checked_firmware_decides_the_target_region() {
        let mut app = App::base(true);
        app.seed_preview("firmware");
        app.region = None;
        app.report = None;
        app.activity = Some(Activity::Inspect);
        for region in [Region::Prc, Region::Row] {
            app.event(Event::Validated(Ok(demo_report(region))));
            assert_eq!(app.region, Some(region));
            assert!(app.activity.is_none());
        }
        app.event(Event::Validated(Err("bad".into())));
        assert!(app.report.is_none());
    }
    #[test]
    fn rollback_prompt_needs_an_explicit_answer_and_is_cleared_on_finish() {
        for reset in [false, true] {
            for kind in [RunKind::Convert, RunKind::Resume] {
                let mut app = App::base(true);
                app.reset_progress(kind);
                app.activity = Some(Activity::Run);
                let (tx, rx) = mpsc::channel();
                app.event(Event::ConfirmRollbackReset(
                    "boot: 펌웨어 1 < 기기 2".into(),
                    tx,
                ));
                for _ in 0..3 {
                    let _ = app.update(Message::Tick);
                }
                assert!(rx.try_recv().is_err());
                assert!(app.rollback_confirmation.is_some());
                drop(app.view());
                let _ = app.update(Message::ConfirmRollbackReset(reset));
                assert_eq!(rx.try_recv(), Ok(reset));
                assert!(app.rollback_confirmation.is_none());
                let (tx, rx) = mpsc::channel();
                app.event(Event::ConfirmRollbackReset("details".into(), tx));
                app.event(Event::Finished(Err("aborted".into())));
                assert!(app.rollback_confirmation.is_none());
                assert!(rx.recv().is_err());
            }
        }
        // Original restore never compares rollback indices.
        let mut app = App::base(true);
        app.reset_progress(RunKind::Restore);
        app.activity = Some(Activity::Run);
        let (tx, rx) = mpsc::channel();
        app.event(Event::ConfirmRollbackReset("details".into(), tx));
        assert!(app.rollback_confirmation.is_none());
        assert!(rx.recv().is_err());
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
        for page in [Page::Firmware, Page::Confirm, Page::Resume] {
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
        assert!(app.options().unwrap().country.is_none());
        let _ = app.update(Message::CountryOpen);
        let _ = app.update(Message::Country("KR".into()));
        let _ = app.update(Message::CountryApply);
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
        for message in [Message::Recovery, Message::Convert, Message::Settings] {
            let _ = app.update(message);
        }
        assert_eq!(app.page, Page::Progress);
        assert_eq!(app.region, Some(Region::Row));
        assert!(app.options().is_some());
    }
    #[test]
    fn continue_chooses_the_country_then_shows_the_final_check() {
        use lapiswitch_core::deviceinfo::RollbackMode;
        let mut app = App::base(true);
        assert_eq!(app.region, None);
        let _ = app.update(Message::Convert);
        assert_eq!(app.page, Page::Firmware);
        // Continue needs a checked firmware.
        let _ = app.update(Message::Next);
        assert!(!app.country_open);
        app.event(Event::Validated(Ok(demo_report(Region::Prc))));
        assert_eq!(app.region, Some(Region::Prc));
        // Cancelling the country stays on the firmware check.
        let _ = app.update(Message::Next);
        assert!(app.country_open);
        let _ = app.update(Message::CountryCancel);
        assert_eq!(app.page, Page::Firmware);
        assert!(!app.country_open);
        // Selecting it goes straight to the final check, resetting by default.
        let _ = app.update(Message::Next);
        let _ = app.update(Message::Country("KR".into()));
        let _ = app.update(Message::CountryApply);
        assert_eq!(app.page, Page::Confirm);
        assert!(!app.rollback_open);
        assert_eq!(app.country.as_deref(), Some("KR"));
        assert_eq!(app.rollback_mode, RollbackMode::Reset);
        // From the final check each picker only closes.
        let _ = app.update(Message::CountryOpen);
        let _ = app.update(Message::CountryApply);
        assert_eq!(app.page, Page::Confirm);
        let _ = app.update(Message::RollbackOpen);
        let _ = app.update(Message::Rollback(RollbackMode::Keep));
        let _ = app.update(Message::RollbackCancel);
        assert_eq!(app.rollback_mode, RollbackMode::Reset);
        let _ = app.update(Message::RollbackOpen);
        let _ = app.update(Message::Rollback(RollbackMode::Keep));
        let _ = app.update(Message::RollbackApply);
        assert_eq!(app.page, Page::Confirm);
        assert_eq!(app.rollback_mode, RollbackMode::Keep);
        let _ = app.update(Message::RegionNoticeOpen);
        assert!(app.region_notice_open);
        let _ = app.update(Message::RegionNoticeClose);
        assert!(!app.region_notice_open);
        let _ = app.update(Message::Back);
        assert_eq!(app.page, Page::Firmware);
        let _ = app.update(Message::Back);
        assert_eq!(app.page, Page::Dashboard);
    }
    #[test]
    fn a_dump_only_device_opens_only_the_dump_and_the_dump_needs_its_choices() {
        let mut app = App::base(true);
        app.seed_preview("dump-only");
        assert!(app.device.dump_only());
        let _ = app.update(Message::Home);
        for message in [Message::Convert, Message::Edit] {
            let _ = app.update(message);
            assert_eq!(app.page, Page::Dashboard);
        }
        let _ = app.update(Message::Dump);
        assert_eq!(app.page, Page::Dump);
        app.loader = None;
        app.preferences.dump_root = None;
        assert!(app.start_blocker().unwrap().contains("EDL"));
        app.demo_pick(Picker::Loader);
        assert!(app.start_blocker().unwrap().contains("저장 위치"));
        app.demo_pick(Picker::DumpRoot);
        assert!(app.start_blocker().unwrap().contains("동의"));
        app.agreed = true;
        assert!(app.can_start(), "{:?}", app.start_blocker());
        // The flashing check refuses it too, with a problem InfoBar.
        app.page = Page::Edit;
        assert!(matches!(
            app.start_issue(),
            Some((Some(fluent::Severity::Error), reason)) if reason.contains("덤프만")
        ));
        // A writable device still opens every task.
        app.device.snapshot.model = "TB520FU".into();
        let _ = app.update(Message::Home);
        let _ = app.update(Message::Edit);
        assert_eq!(app.page, Page::Edit);
    }
    #[test]
    fn the_restore_starts_only_from_twrp_or_edl_with_its_choices() {
        use crate::device::Mode;
        let mut app = App::base(true);
        app.seed_preview("recovery");
        app.activity = None;
        app.recovery_summary = Some("TB520FU · slot a".into());
        app.agreed = true;
        for (mode, allowed) in [
            (Mode::Adb, false),
            (Mode::Fastboot, false),
            (Mode::Recovery, true),
            (Mode::Edl, true),
        ] {
            app.device.snapshot.mode = mode;
            assert_eq!(app.can_restore(), allowed, "{mode:?}");
            if !allowed {
                assert!(
                    matches!(
                        app.restore_issue(),
                        Some((Some(fluent::Severity::Warning), reason)) if reason.contains("TWRP")
                    ),
                    "{mode:?} {:?}",
                    app.restore_issue().map(|i| i.1)
                );
            }
        }
        app.agreed = false;
        assert!(app.restore_issue().unwrap().1.contains("동의"));
        app.loader = None;
        assert!(app.restore_issue().unwrap().1.contains("EDL"));
        app.recovery_summary = None;
        assert!(app.restore_issue().unwrap().1.contains("백업 폴더"));
    }
    #[test]
    fn a_dump_run_shows_its_folder_on_the_result() {
        let mut app = App::base(true);
        app.seed_preview("dump-result");
        assert_eq!(app.page, Page::Result);
        assert_eq!(app.progress.flow, Flow::Dump);
        assert!(app.dump_folder.is_some());
        drop(app.view());
        app.reset_progress(RunKind::Dump);
        assert!(app.dump_folder.is_none());
        app.event(Event::Dumped("Demo/dumps/x".into()));
        assert_eq!(
            app.dump_folder.as_deref(),
            Some(std::path::Path::new("Demo/dumps/x"))
        );
    }
    #[test]
    fn firmware_for_another_model_blocks_the_start() {
        let mut app = App::base(true);
        app.seed_preview("confirm");
        app.agreed = true;
        assert!(app.can_start(), "{:?}", app.start_blocker());
        app.seed_preview("model-mismatch");
        assert!(app.firmware_model_differs());
        assert!(!app.can_start());
        // EDL names no model; the run checks the device itself.
        app.device.snapshot.model.clear();
        assert!(!app.firmware_model_differs());
        let _view = app.view();
    }
    #[test]
    fn long_paths_are_shortened_in_the_middle() {
        let path = std::path::Path::new(
            r"D:\TB520FU_ROW_OPEN_USER_Q29300.0_A17_ZUI_18.5.10.037_ST_260929\image",
        );
        assert_eq!(view::short_path(path, 1000.0), path.display().to_string());
        let short = view::short_path(path, 400.0);
        assert!(short.starts_with(r"D:\…"), "{short}");
        assert!(short.ends_with(r"_260929\image"), "{short}");
        assert!(short.chars().count() <= (400.0 / view::PATH_CHAR) as usize);
    }
    #[test]
    fn resume_gate_blocks_recovery_uncertain_or_corrupt_state() {
        let mut app = App::base(true);
        app.agreed = true;
        assert!(!app.can_resume());
        for action in [ResumeAction::Recovery, ResumeAction::Restart] {
            let state = saved(Stage::S5);
            app.pending = Some(Pending { state, action });
            assert!(!app.can_resume());
        }
        for action in [
            ResumeAction::Wait,
            ResumeAction::Flash,
            ResumeAction::Finalize,
        ] {
            let state = saved(Stage::S5);
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
                "rollback-confirm",
                "preflight",
                "options-country",
                "rollback",
                "region-notice",
                "edl-start-warning",
                "deviceinfo-edit",
                "dump",
                "dump-only",
                "dashboard-dump-only",
                "dashboard-twrp",
                "dashboard-unsupported",
                "dump-result",
                "flash-result",
                "deviceinfo-editor",
                "deviceinfo-editor-custom",
                "deviceinfo-editor-rollback",
                "deviceinfo-editor-review",
                "parse",
                "parse-view",
                "parse-view-long",
                "parse-view-rollback",
                "deviceinfo-editor-long",
                "deviceinfo-editor-rollback-open",
                "deviceinfo-editor-notices",
                "deviceinfo-editor-custom-notices",
                "deviceinfo-editor-country",
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
                // Korean in light, English in dark: every page in both themes.
                let mut app = App::base(true);
                app.seed_preview(screen);
                app.preferences.language = language;
                app.system_dark = language == Language::English;
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
    fn new_conversion_defaults_to_reset_and_mode_is_locked_while_running() {
        let mut app = App::base(true);
        app.country = Some("KR".into());
        assert_eq!(
            app.rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Reset
        );
        let _ = app.update(Message::RollbackOpen);
        let _ = app.update(Message::Rollback(
            lapiswitch_core::deviceinfo::RollbackMode::Keep,
        ));
        assert_eq!(
            app.rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Reset
        );
        let _ = app.update(Message::RollbackApply);
        assert_eq!(
            app.rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Keep
        );
        app.activity = Some(Activity::Run);
        let _ = app.update(Message::RollbackOpen);
        let _ = app.update(Message::Rollback(
            lapiswitch_core::deviceinfo::RollbackMode::Reset,
        ));
        let _ = app.update(Message::RollbackApply);
        assert_eq!(
            app.rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Keep
        );
        // Flash options carry only the country; the mode lives in the request.
        assert!(serde_json::from_str::<FlashOptions>(r#"{"country":null}"#).is_ok());
        assert!(
            serde_json::from_str::<FlashOptions>(r#"{"country":null,"rollback_mode":"KEEP"}"#)
                .is_err()
        );
    }
    #[test]
    fn custom_rom_confirmation_requires_an_explicit_answer_and_is_cleared_on_finish() {
        for answer in [false, true] {
            let mut app = App::base(true);
            app.reset_progress(RunKind::Convert);
            let (tx, rx) = mpsc::channel();
            app.event(Event::ConfirmUnrecognizedFirmware(tx));
            let _ = app.update(Message::Tick);
            assert!(app.firmware_confirmation.is_some());
            assert!(rx.try_recv().is_err());
            let _ = app.update(Message::ConfirmUnrecognizedFirmware(answer));
            assert_eq!(rx.try_recv(), Ok(answer));
            assert!(app.firmware_confirmation.is_none());
            let (tx, rx) = mpsc::channel();
            app.event(Event::ConfirmUnrecognizedFirmware(tx));
            app.event(Event::Finished(Err("aborted".into())));
            assert!(app.firmware_confirmation.is_none());
            assert!(rx.recv().is_err());
        }
        for kind in [RunKind::Resume, RunKind::Restore] {
            let mut app = App::base(true);
            app.reset_progress(kind);
            let (tx, rx) = mpsc::channel();
            app.event(Event::ConfirmUnrecognizedFirmware(tx));
            assert!(app.firmware_confirmation.is_none());
            assert!(rx.recv().is_err());
        }
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
    fn new_progress_survives_state_polling_and_confirmation_waits() {
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
    fn resume_chooses_twrp_and_preserves_wait_progress() {
        use lapiswitch_core::recovery_flow::Phase;
        let mut app = App::base(true);
        let state = demo_saved_state(Stage::S5);
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
#[cfg(test)]
mod theme_tests {
    use super::*;
    use crate::settings::ThemeChoice;

    #[test]
    fn the_theme_follows_windows_unless_light_or_dark_is_chosen() {
        let mut app = App::base(false);
        // Start from the default, whatever this machine has saved.
        app.preferences.theme = ThemeChoice::System;
        app.system_dark = false;
        assert!(!app.dark());
        let _ = app.update(Message::SystemTheme(iced::theme::Mode::Dark));
        assert!(app.dark());
        app.preferences.theme = ThemeChoice::Light;
        assert!(!app.dark());
        app.preferences.theme = ThemeChoice::Dark;
        let _ = app.update(Message::SystemTheme(iced::theme::Mode::Light));
        assert!(app.dark());
        app.preferences.theme = ThemeChoice::System;
        assert!(!app.dark());
        // Previews keep the theme chosen on the command line.
        let mut preview = App::base(true);
        let _ = preview.update(Message::SystemTheme(iced::theme::Mode::Dark));
        assert!(!preview.dark());
        // The saved choice round-trips.
        let json = serde_json::to_string(&ThemeChoice::Dark).unwrap();
        assert_eq!(json, "\"dark\"");
        assert_eq!(
            serde_json::from_str::<crate::settings::Preferences>("{}")
                .unwrap()
                .theme,
            ThemeChoice::System
        );
    }
}
