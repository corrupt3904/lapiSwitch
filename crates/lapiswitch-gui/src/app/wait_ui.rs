use super::*;
use worker::WaitTicket;

impl App {
    pub(super) fn can_stop_wait(&self, ticket: WaitTicket) -> bool {
        self.activity == Some(Activity::Run)
            && self.progress.flow != Flow::Restore
            && self.wait_ticket == Some(ticket)
            && self.reboot_confirmation.is_none()
            && self.edl_confirmation.is_none()
            && self.rollback_confirmation.is_none()
            && !self.close_warning
            && self.wait_control.can_stop(ticket)
    }
    pub(super) fn open_wait_stop(&mut self, ticket: WaitTicket) {
        if self.can_stop_wait(ticket) {
            self.stop_confirmation = Some(ticket);
        }
    }
    pub(super) fn confirm_wait_stop(&mut self, ticket: WaitTicket) {
        if self.stop_confirmation != Some(ticket) {
            return;
        }
        self.stop_confirmation = None;
        if self.can_stop_wait(ticket) && self.wait_control.request_stop(ticket) && self.demo {
            self.event(Event::Finished(Err(lapiswitch_core::msg!(
                "core_wait_stopped"
            ))));
        }
    }
    pub(super) fn refresh_wait_stop(&mut self) {
        if let Some(ticket) = self.stop_confirmation
            && !self.can_stop_wait(ticket)
        {
            self.stop_confirmation = None;
        }
    }
}

pub(super) fn button(app: &App) -> Option<Element<'_, Message>> {
    use super::view::*;
    let ticket = app.wait_ticket?;
    if !app.can_stop_wait(ticket) && !app.wait_control.stopped() {
        return None;
    }
    Some(
        action(
            if app.wait_control.stopped() {
                app.tr("wait_stopping")
            } else {
                app.tr("wait_stop_operation")
            },
            Message::StopWait(ticket),
            app.can_stop_wait(ticket),
        )
        .style(super::fluent::danger_style)
        .into(),
    )
}

pub(super) fn modal(app: &App, ticket: WaitTicket) -> iced::widget::Container<'_, Message> {
    use super::view::*;
    use iced::Fill;
    use iced::widget::{column, row, space};
    let message = if ticket.phase == lapiswitch_core::recovery_flow::Phase::RecoveryWaiting {
        app.tr("wait_stop_the_operation_you_can_resume")
    } else {
        app.tr("wait_stop_the_operation_you_can_resume_2")
    };
    panel(
        column![
            label(app.tr("wait_stop_operation")).size(24),
            label(message),
            row![
                space().width(Fill),
                action(app.tr("archive_cancel"), Message::CancelStopWait, true).style(soft_style),
                action(
                    app.tr("archive_confirm"),
                    Message::ConfirmStopWait(ticket),
                    app.can_stop_wait(ticket)
                )
                .style(super::fluent::danger_style)
            ]
            .spacing(12),
        ]
        .spacing(20),
    )
    .max_width(620)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lapiswitch_core::recovery_flow::Phase;
    use std::time::{Duration, Instant};
    fn ready(phase: Phase, elapsed: u64) -> (App, WaitTicket) {
        let mut app = App::base(true);
        app.reset_progress(RunKind::Convert);
        app.event(Event::RecoveryPhase(phase));
        let ticket = app
            .wait_control
            .begin_at(phase, Instant::now() - Duration::from_secs(elapsed))
            .unwrap();
        app.event(Event::RecoveryWait(Some(ticket)));
        (app, ticket)
    }
    #[test]
    fn thirty_seconds_only_enables_the_button_and_cancel_never_stops_monitoring() {
        for phase in [Phase::RecoveryWaiting, Phase::EdlWaiting] {
            let (mut early, ticket) = ready(phase, 29);
            let _ = early.update(Message::StopWait(ticket));
            assert!(early.stop_confirmation.is_none());
            assert!(button(&early).is_none());
            let (mut app, ticket) = ready(phase, 30);
            let _ = app.update(Message::Tick);
            assert!(app.stop_confirmation.is_none());
            assert!(button(&app).is_some());
            let _ = app.update(Message::StopWait(ticket));
            assert_eq!(app.stop_confirmation, Some(ticket));
            let _ = app.update(Message::CancelStopWait);
            assert!(!app.wait_control.stopped());
            assert!(app.can_stop_wait(ticket));
            assert_eq!(app.activity, Some(Activity::Run));
        }
    }
    #[test]
    fn confirmed_stop_preserves_the_result_instead_of_starting_the_next_stage() {
        for phase in [Phase::RecoveryWaiting, Phase::EdlWaiting] {
            let (mut app, ticket) = ready(phase, 30);
            let _ = app.update(Message::StopWait(ticket));
            let _ = app.update(Message::ConfirmStopWait(ticket));
            assert!(app.wait_control.stopped());
            assert_eq!(app.page, Page::Result);
            assert!(matches!(app.result, Some(Err(_))));
        }
    }
    #[test]
    fn a_connection_closes_the_old_dialog_and_stale_confirmations_cannot_stop_writes_or_new_waits()
    {
        let (mut app, old) = ready(Phase::RecoveryWaiting, 31);
        let _ = app.update(Message::StopWait(old));
        assert!(app.wait_control.finish_wait());
        app.event(Event::RecoveryWait(None));
        app.event(Event::RecoveryPhase(Phase::Planned));
        assert!(app.stop_confirmation.is_none());
        let _ = app.update(Message::ConfirmStopWait(old));
        assert!(!app.wait_control.stopped());
        let next = app
            .wait_control
            .begin_at(Phase::EdlWaiting, Instant::now() - Duration::from_secs(31))
            .unwrap();
        app.event(Event::RecoveryWait(Some(next)));
        let _ = app.update(Message::StopWait(next));
        let _ = app.update(Message::ConfirmStopWait(old));
        assert_eq!(app.stop_confirmation, Some(next));
        assert!(!app.wait_control.stopped());
    }
}
