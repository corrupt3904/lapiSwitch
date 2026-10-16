//! Same explicit actions as LTBox: ADB host:kill and confirmed, elevated
//! Software Fix close. Detection never stops processes by itself.
use super::*;
use ltbox_device::software_fix::CloseError;
use std::time::{Duration, Instant};

pub(super) struct SoftwareFix {
    pub running: bool,
    pub checking: bool,
    pub closing: bool,
    pub confirm: bool,
    pub error: Option<String>,
    next: Instant,
}
impl Default for SoftwareFix {
    fn default() -> Self {
        Self {
            running: false,
            checking: false,
            closing: false,
            confirm: false,
            error: None,
            next: Instant::now(),
        }
    }
}
impl App {
    pub(super) fn poll_software_fix(&mut self) -> Task<Message> {
        if self.demo || !cfg!(windows) || self.fix.checking || Instant::now() < self.fix.next {
            return Task::none();
        }
        self.fix.checking = true;
        self.fix.next = Instant::now() + Duration::from_secs(2);
        Task::perform(
            async {
                tokio::task::spawn_blocking(ltbox_device::software_fix::is_running)
                    .await
                    .unwrap_or_else(|error| Err(error.to_string()))
            },
            Message::FixPolled,
        )
    }
    pub(super) fn can_close_software_fix(&self) -> bool {
        cfg!(windows) && self.fix.running && !self.busy() && self.deferred.is_none()
    }
    pub(super) fn kill_adb_server(&mut self) -> Task<Message> {
        self.adb_killing = true;
        self.error = None;
        if self.demo {
            return Task::done(Message::AdbKilled(Ok(())));
        }
        Task::perform(
            async {
                tokio::task::spawn_blocking(ltbox_device::adb::kill_adb_server)
                    .await
                    .unwrap_or_else(|error| {
                        Err(ltbox_device::adb::AdbError::Client(error.to_string()))
                    })
                    .map_err(|error| error.to_string())
            },
            Message::AdbKilled,
        )
    }
    pub(super) fn confirm_close_software_fix(&mut self) -> Task<Message> {
        if !self.fix.confirm {
            return Task::none();
        }
        self.fix.confirm = false;
        if !self.can_close_software_fix() {
            return Task::none();
        }
        self.fix.closing = true;
        self.fix.error = None;
        if self.demo {
            return Task::done(Message::FixClosed(Ok(())));
        }
        Task::perform(
            async {
                tokio::task::spawn_blocking(ltbox_device::software_fix::force_close)
                    .await
                    .unwrap_or_else(|error| Err(CloseError::Failed(error.to_string())))
            },
            Message::FixClosed,
        )
    }
    pub(super) fn fix_polled(&mut self, result: Result<bool, String>) {
        self.fix.checking = false;
        match result {
            Ok(running) => {
                self.fix.running = running;
                if !running {
                    self.fix.error = None;
                    self.fix.confirm = false;
                }
            }
            // Failed enumeration never proves that the process has exited.
            Err(error) => self.fix.error = Some(error),
        }
    }
    pub(super) fn fix_closed(&mut self, result: Result<(), CloseError>) {
        self.fix.closing = false;
        self.fix.next = Instant::now();
        self.fix.error = match result {
            Ok(()) => None,
            Err(CloseError::Cancelled) => Some(
                self.t(
                    "Software Fix 종료가 취소되었습니다. 다시 시도할 수 있습니다.",
                    "Closing Software Fix was cancelled. You can try again.",
                )
                .into(),
            ),
            Err(CloseError::Failed(error)) => Some(format!(
                "{}: {error}",
                self.t(
                    "Software Fix를 종료하지 못했습니다. 직접 종료하거나 다시 시도하세요.",
                    "Could not close Software Fix. Close it manually or try again."
                )
            )),
        };
        // LTBox keeps the banner until a successful absent-process poll.
        self.device.snapshot = Default::default();
        self.device.checked = None;
        self.device.next = Instant::now();
    }
}

pub(super) fn banner(app: &App) -> Option<Element<'_, Message>> {
    use super::view::*;
    use iced::widget::{column, row, space};
    use iced::{Alignment, Fill};
    let mut content = column![].spacing(10);
    let mut visible = false;
    if app.fix.running {
        visible = true;
        content = content.push(
            row![
                column![
                    label(app.t("Software Fix를 종료하세요.", "Close Software Fix.")),
                    small(app.t(
                        "기기에 연결하지 못하거나 연결이 끊길 수 있습니다.",
                        "It may prevent device access or disconnect the device."
                    ))
                ]
                .spacing(4)
                .width(Fill),
                action(
                    if app.fix.closing {
                        app.t("종료 중…", "Closing…")
                    } else {
                        app.t("강제 종료", "Force close")
                    },
                    Message::CloseFix,
                    app.can_close_software_fix()
                )
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
        if let Some(error) = &app.fix.error {
            content = content.push(small(error));
        }
    }
    if app.device.snapshot.mode == crate::device::Mode::ServerBlocking || app.adb_killing {
        visible = true;
        content = content.push(
            row![
                label(app.t(
                    "다른 ADB 서버가 기기 연결을 점유하고 있습니다.",
                    "Another ADB server is blocking device access."
                ))
                .width(Fill),
                space().width(4),
                action(
                    if app.adb_killing {
                        app.t("종료 중…", "Stopping…")
                    } else {
                        app.t("ADB 서버 종료", "Stop ADB server")
                    },
                    Message::KillAdb,
                    !app.busy() && app.deferred.is_none()
                )
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }
    visible.then(|| view::notification(content).into())
}
pub(super) fn modal(app: &App) -> iced::widget::Container<'_, Message> {
    use super::view::*;
    use iced::widget::{column, row};
    panel(
        column![
            label(app.t("Software Fix 종료", "Close Software Fix")).size(24),
            label(app.t(
                "Software Fix를 강제 종료합니다. Windows에서 관리자 권한을 요청합니다.",
                "Force close Software Fix. Windows will request administrator permission."
            )),
            row![
                iced::widget::space().width(iced::Fill),
                action(app.t("취소", "Cancel"), Message::CancelCloseFix, true).style(soft_style),
                action(
                    app.t("종료", "Close"),
                    Message::ConfirmCloseFix,
                    app.can_close_software_fix()
                )
            ]
            .spacing(12),
        ]
        .spacing(20),
    )
    .max_width(560)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_banner_survives_cancel_failure_and_failed_detection() {
        let mut app = App::base(true);
        app.fix_polled(Ok(true));
        app.fix_polled(Err("enumeration failed".into()));
        assert!(app.fix.running);
        for result in [
            Ok(()),
            Err(CloseError::Cancelled),
            Err(CloseError::Failed("denied".into())),
        ] {
            app.fix.closing = true;
            app.fix_closed(result);
            assert!(app.fix.running);
            assert!(!app.fix.closing);
        }
        app.fix_polled(Ok(false));
        assert!(!app.fix.running);
        assert!(app.fix.error.is_none());
    }
    #[cfg(windows)]
    #[test]
    fn close_requires_confirmation_and_is_blocked_during_work() {
        let mut app = App::base(true);
        app.fix.running = true;
        assert_eq!(app.confirm_close_software_fix().units(), 0);
        let _ = app.update(Message::CloseFix);
        assert!(app.fix.confirm);
        let _ = app.update(Message::CancelCloseFix);
        assert!(!app.fix.confirm);
        app.activity = Some(Activity::Run);
        assert!(!app.can_close_software_fix());
        let _ = app.update(Message::CloseFix);
        assert!(!app.fix.confirm);
        app.activity = None;
        let _ = app.update(Message::CloseFix);
        let task = app.update(Message::ConfirmCloseFix);
        assert!(app.fix.closing);
        // Demo only, and the task is not executed: no process is terminated.
        drop(task);
        assert!(!app.can_start());
    }
    #[test]
    fn adb_stop_waits_for_usb_poll_and_a_failure_can_be_retried() {
        let mut app = App::base(true);
        app.seed_preview("adb-conflict");
        let id = app.device.begin().unwrap();
        let _ = app.update(Message::KillAdb);
        assert!(app.deferred.is_some());
        assert!(!app.adb_killing);
        app.event(Event::Device(
            id,
            Ok(crate::device::Snapshot {
                mode: crate::device::Mode::ServerBlocking,
                ..Default::default()
            }),
        ));
        assert!(app.adb_killing);
        let _ = app.update(Message::AdbKilled(Err("unreachable".into())));
        assert!(!app.adb_killing);
        assert!(app.error.is_some());
        assert!(app.device.checked.is_none());
    }
    #[test]
    fn usb_deferred_stop_task_is_returned_to_the_runtime() {
        let mut app = App::base(true);
        app.seed_preview("adb-conflict");
        let id = app.device.begin().unwrap();
        let _ = app.update(Message::KillAdb);
        app.tx
            .send(Event::Device(
                id,
                Ok(crate::device::Snapshot {
                    mode: crate::device::Mode::ServerBlocking,
                    ..Default::default()
                }),
            ))
            .unwrap();
        let task = app.update(Message::Tick);
        assert!(task.units() > 0, "deferred task must not be discarded");
        assert!(app.adb_killing);
        assert!(app.queued.is_empty());
        drop(task);
    }
}
