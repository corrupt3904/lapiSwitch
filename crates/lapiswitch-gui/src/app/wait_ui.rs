use super::*;
use worker::WaitTicket;

impl App {
    pub(super) fn can_stop_wait(&self, ticket: WaitTicket) -> bool {
        self.activity == Some(Activity::Run)
            && self.progress.flow == Flow::Recovery
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
            self.event(Event::Finished(Err(
                "연결 대기를 중지했습니다. 백업과 작업 기록을 보존했습니다.".into(),
            )));
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
    use iced::widget::button;
    let ticket = app.wait_ticket?;
    if !app.can_stop_wait(ticket) && !app.wait_control.stopped() {
        return None;
    }
    Some(
        action(
            if app.wait_control.stopped() {
                app.t("중지 요청 중…", "Stopping…")
            } else {
                app.t("작업 중지", "Stop operation")
            },
            Message::StopWait(ticket),
            app.can_stop_wait(ticket),
        )
        .style(button::danger)
        .into(),
    )
}

pub(super) fn modal(app: &App, ticket: WaitTicket) -> iced::widget::Container<'_, Message> {
    use super::view::*;
    use iced::Fill;
    use iced::widget::{button, column, row, space};
    let message = if ticket.phase == lapiswitch_core::recovery_flow::Phase::RecoveryWaiting {
        app.t(
            "작업을 중지하시겠습니까? 추후 ADB(TWRP) 연결이 되었을 때 계속 진행하거나, EDL로 수동 진입하여 원본 복구를 진행할 수 있습니다",
            "Stop the operation? You can resume when ADB (TWRP) connects later, or enter EDL manually and restore the original backup.",
        )
    } else {
        app.t(
            "작업을 중지하시겠습니까? 추후 EDL 연결이 되었을 때 계속 진행하거나, 원본 복구를 진행할 수 있습니다",
            "Stop the operation? You can resume when EDL connects later, or restore the original backup.",
        )
    };
    panel(
        column![
            label(app.t("작업 중지", "Stop operation")).size(24),
            label(message),
            row![
                space().width(Fill),
                action(app.t("취소", "Cancel"), Message::CancelStopWait, true).style(soft_style),
                action(
                    app.t("확인", "Confirm"),
                    Message::ConfirmStopWait(ticket),
                    app.can_stop_wait(ticket)
                )
                .style(button::danger)
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
