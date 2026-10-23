//! Confirm and archive an older run as part of starting the reviewed conversion.
use super::*;

impl App {
    pub(super) fn can_archive_run(&self) -> bool {
        matches!(self.page, Page::Confirm | Page::Edit)
            && self.can_start()
            && self.pending.is_some()
            && !self.fix.confirm
            && !self.country_open
            && self.reboot_confirmation.is_none()
            && self.edl_confirmation.is_none()
    }

    pub(super) fn start_conversion(&mut self) -> Task<Message> {
        self.refresh_pending();
        if !matches!(self.page, Page::Confirm | Page::Edit | Page::Dump) || !self.can_start() {
            self.error = self.start_blocker().map(str::to_owned);
            return Task::none();
        }
        if let Some(pending) = &self.pending {
            self.archive_confirmation = Some(pending.state.clone());
            return Task::none();
        }
        self.begin_start_probe()
    }

    fn begin_start_probe(&mut self) -> Task<Message> {
        self.error = None;
        if self.demo {
            return self.update(Message::StartVerified);
        }
        self.deferred = Some(Message::StartVerified);
        self.poll_device(false);
        Task::none()
    }

    pub(super) fn start_verified(&mut self) {
        // A second instance may have left a run while the device was probed.
        self.refresh_pending();
        if !matches!(self.page, Page::Confirm | Page::Edit | Page::Dump) || !self.can_start() {
            self.error = self.start_blocker().map(str::to_owned);
        } else if let Some(pending) = &self.pending {
            self.archive_confirmation = Some(pending.state.clone());
        } else if self.page == Page::Dump {
            self.launch(
                worker::Action::Dump {
                    device: self.device.supported(),
                    loader: self.loader.clone().expect("validated loader"),
                    output: self.output.clone(),
                    dump_root: self
                        .preferences
                        .dump_root
                        .clone()
                        .expect("validated dump location"),
                },
                RunKind::Dump,
            );
        } else if self.page == Page::Edit {
            self.launch(
                worker::Action::Edit {
                    device: self.device.supported(),
                    loader: self.loader.clone().expect("validated loader"),
                    output: self.output.clone(),
                },
                RunKind::Edit,
            );
        } else {
            self.launch(
                worker::Action::Full {
                    approved: Box::new(self.report.clone().expect("validated firmware")),
                    rollback_mode: self.rollback_mode,
                    options: self.options().expect("validated options"),
                    output: self.output.clone(),
                },
                RunKind::Convert,
            );
        }
    }

    pub(super) fn confirm_archive_run(&mut self) -> Task<Message> {
        if !self.can_archive_run() {
            return Task::none();
        }
        let Some(expected) = self.archive_confirmation.take() else {
            return Task::none();
        };
        self.activity = Some(Activity::Archive);
        self.error = None;
        if self.demo {
            return Task::done(Message::RunArchived(Ok("Demo/history/archived-run".into())));
        }
        let path = self.state_path.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    StateStore::new(path).archive_for_new_run(&expected)
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()))
            },
            Message::RunArchived,
        )
    }

    pub(super) fn run_archived(&mut self, result: Result<PathBuf, String>) -> Task<Message> {
        if self.activity != Some(Activity::Archive) {
            return Task::none();
        }
        self.activity = None;
        match result {
            Ok(_) => {
                self.pending = None;
                self.refresh_pending();
                if self.startup_error.is_none() && self.pending.is_none() {
                    // Keep the reviewed NEW settings and consent. Reprobe before
                    // launch; neither the old run's settings nor slot is reused.
                    return self.begin_start_probe();
                }
                self.error = Some(self.tr("archive_another_run_appeared_or_its_record").into());
            }
            Err(error) => {
                self.refresh_pending();
                self.error = Some(error);
            }
        }
        Task::none()
    }
}

pub(super) fn modal(app: &App) -> iced::widget::Container<'_, Message> {
    use super::view::*;
    use iced::Fill;
    use iced::widget::{column, row, space};
    panel(
        column![
            label(app.tr("archive_archive_run_and_start_over")).size(24),
            label(app.tr("archive_stop_the_previous_run_and_prepare")),
            row![
                space().width(Fill),
                action(
                    app.tr("archive_cancel"),
                    Message::CancelArchiveRun,
                    !app.busy()
                )
                .style(soft_style),
                action(
                    app.tr("archive_confirm"),
                    Message::ConfirmArchiveRun,
                    app.can_archive_run()
                ),
            ]
            .spacing(12),
        ]
        .spacing(20),
    )
    .max_width(600)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> App {
        let mut app = App::base(true);
        app.seed_preview("confirm-pending");
        app
    }

    #[test]
    fn start_with_pending_opens_confirmation_and_cancel_preserves_both_runs_settings() {
        let mut app = ready();
        let region = app.region;
        assert!(app.can_start());
        let _ = app.update(Message::ConfirmArchiveRun);
        assert_eq!(app.activity, None);
        let _ = app.update(Message::Start);
        for message in [
            Message::Tick,
            Message::Home,
            Message::StartVerified,
            Message::Convert,
        ] {
            let _ = app.update(message);
            assert!(app.archive_confirmation.is_some());
            assert_eq!(app.page, Page::Confirm);
            assert_eq!(app.activity, None);
        }
        let _ = app.update(Message::CancelArchiveRun);
        assert!(app.archive_confirmation.is_none());
        assert!(app.pending.is_some());
        assert_eq!(app.region, region);
        assert!(app.agreed && app.can_start());
    }

    #[test]
    fn pending_does_not_bypass_connection_driver_firmware_or_consent_checks() {
        let mut app = ready();
        app.device.snapshot = Default::default();
        let _ = app.update(Message::Start);
        assert!(!app.can_start() && app.archive_confirmation.is_none());
        for activity in [
            Activity::Inspect,
            Activity::Dialog,
            Activity::Install,
            Activity::Archive,
            Activity::Run,
        ] {
            let mut app = ready();
            app.activity = Some(activity);
            let _ = app.update(Message::Start);
            assert!(!app.can_start() && app.archive_confirmation.is_none());
        }
        let mut app = ready();
        app.agreed = false;
        assert!(!app.can_start());
        app.agreed = true;
        app.report = None;
        assert!(!app.can_start());
        app.seed_preview("confirm-pending");
        app.startup_error = Some("state error".into());
        assert!(!app.can_start());
    }

    #[test]
    fn archive_success_starts_the_reviewed_new_conversion_without_resetting_options() {
        let mut app = ready();
        app.country = Some("US".into());
        app.rollback_mode = lapiswitch_core::deviceinfo::RollbackMode::Keep;
        let folder = app.folder.clone();
        let _ = app.update(Message::Start);
        let _ = app.update(Message::ConfirmArchiveRun);
        assert_eq!(app.activity, Some(Activity::Archive));
        assert!(app.pending.is_some());
        let _ = app.update(Message::RunArchived(Ok("history/record".into())));
        assert!(app.pending.is_none());
        assert_eq!(app.activity, Some(Activity::Run));
        assert_eq!(app.page, Page::Progress);
        assert_eq!(app.folder, folder);
        assert_eq!(app.country.as_deref(), Some("US"));
        assert_eq!(
            app.rollback_mode,
            lapiswitch_core::deviceinfo::RollbackMode::Keep
        );
    }

    #[test]
    fn failed_archive_keeps_pending_and_never_starts_writes() {
        let mut app = ready();
        let _ = app.update(Message::Start);
        let _ = app.update(Message::ConfirmArchiveRun);
        let _ = app.update(Message::RunArchived(Err("history access denied".into())));
        assert_eq!(app.activity, None);
        assert_eq!(app.page, Page::Confirm);
        assert!(app.pending.is_some());
        assert!(app.error.as_deref().unwrap().contains("access denied"));
        let _ = app.update(Message::Start);
        assert!(app.archive_confirmation.is_some());
    }

    #[test]
    fn a_pending_record_seen_after_the_probe_still_needs_confirmation() {
        let mut app = ready();
        let _ = app.update(Message::StartVerified);
        assert!(app.archive_confirmation.is_some());
        assert_eq!(app.activity, None);
    }

    #[test]
    fn disconnect_after_archival_blocks_the_new_run() {
        let mut app = ready();
        let _ = app.update(Message::Start);
        let _ = app.update(Message::ConfirmArchiveRun);
        app.device.snapshot = Default::default();
        let _ = app.update(Message::RunArchived(Ok("history/record".into())));
        assert_eq!(app.activity, None);
        assert!(app.error.is_some());
    }
}
