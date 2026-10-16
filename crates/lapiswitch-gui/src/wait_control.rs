//! A confirmation belongs to one timed connection wait, never a later write.
use lapiswitch_core::recovery_flow::Phase;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const STOP_AFTER: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitTicket {
    generation: u64,
    pub phase: Phase,
    started: Instant,
}
#[derive(Debug, Default)]
struct State {
    generation: u64,
    active: Option<WaitTicket>,
    stopped: bool,
}
#[derive(Debug, Clone, Default)]
pub struct WaitControl(Arc<Mutex<State>>);
impl WaitControl {
    pub fn begin(&self, phase: Phase) -> Option<WaitTicket> {
        self.begin_at(phase, Instant::now())
    }
    pub(crate) fn begin_at(&self, phase: Phase, started: Instant) -> Option<WaitTicket> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped || !matches!(phase, Phase::RecoveryWaiting | Phase::EdlWaiting) {
            return None;
        }
        state.generation += 1;
        let ticket = WaitTicket {
            generation: state.generation,
            phase,
            started,
        };
        state.active = Some(ticket);
        Some(ticket)
    }
    pub fn can_stop(&self, ticket: WaitTicket) -> bool {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        !state.stopped && state.active == Some(ticket) && ticket.started.elapsed() >= STOP_AFTER
    }
    pub fn request_stop(&self, ticket: WaitTicket) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped || state.active != Some(ticket) || ticket.started.elapsed() < STOP_AFTER {
            return false;
        }
        state.stopped = true;
        true
    }
    /// Serialized with request_stop: an accepted stop always wins before writes.
    pub fn finish_wait(&self) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.active = None;
        !state.stopped
    }
    pub fn stopped(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn early_requests_and_stale_confirmations_cannot_stop_another_phase() {
        let control = WaitControl::default();
        let fresh = control.begin(Phase::RecoveryWaiting).unwrap();
        assert!(!control.can_stop(fresh) && !control.request_stop(fresh));
        let old = control
            .begin_at(Phase::RecoveryWaiting, Instant::now() - STOP_AFTER)
            .unwrap();
        assert!(control.can_stop(old));
        assert!(control.finish_wait());
        assert!(!control.request_stop(old));
        let next = control
            .begin_at(Phase::EdlWaiting, Instant::now() - STOP_AFTER)
            .unwrap();
        assert!(!control.request_stop(old));
        assert!(control.request_stop(next));
        assert!(!control.finish_wait());
        assert!(control.stopped());
        assert!(control.begin(Phase::EdlWaiting).is_none());
    }
    #[test]
    fn stop_and_connection_handoff_have_one_winner() {
        for _ in 0..64 {
            let control = WaitControl::default();
            let ticket = control
                .begin_at(Phase::EdlWaiting, Instant::now() - STOP_AFTER)
                .unwrap();
            let peer = control.clone();
            let thread = std::thread::spawn(move || peer.request_stop(ticket));
            let handed_off = control.finish_wait();
            let stopped = thread.join().unwrap();
            assert_ne!(handed_off, stopped);
            assert_eq!(control.stopped(), stopped);
        }
    }
}
