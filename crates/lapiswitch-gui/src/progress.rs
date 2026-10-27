//! Weighted work milestones and real firmware bytes, not a time-driven counter.
use lapiswitch_core::recovery_flow::Phase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Recovery,
    Restore,
    /// DeviceInfo dump: the recovery flow without writes or firmware.
    Dump,
    /// Bootloader unlock/relock: the recovery flow without firmware.
    Bootloader,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Check,
    Device,
    Backup,
    StageRecovery,
    Twrp,
    DeviceInfo,
    /// Read and save DeviceInfo (dump).
    Dump,
    Edl,
    Originals,
    Firmware,
    Finish,
}
impl Step {
    pub fn title(self, language: crate::settings::Language) -> &'static str {
        match self {
            Self::Check => language.tr("step_check_files"),
            Self::Device => language.tr("step_device_slot"),
            Self::Backup => language.tr("step_back_up_originals"),
            Self::StageRecovery => language.tr("step_prepare_gpt_twrp"),
            Self::Twrp => language.tr("step_connect_twrp"),
            Self::DeviceInfo => language.tr("step_deviceinfo_verify"),
            Self::Dump => language.tr("step_read_save_deviceinfo"),
            Self::Edl => language.tr("step_connect_edl"),
            Self::Originals => language.tr("step_restore_verify"),
            Self::Firmware => language.tr("step_install_firmware"),
            Self::Finish => language.tr("step_finish_reboot"),
        }
    }
}
impl Flow {
    fn bounds(self, index: usize) -> (f32, f32) {
        if self == Self::Recovery {
            // Early preparation 20%, firmware 79%, successful finalization 1%.
            const BOUNDS: [f32; 11] = [0., 2., 4., 8., 10., 12., 14., 17., 20., 99., 100.];
            (BOUNDS[index], BOUNDS[index + 1])
        } else {
            let unit = 100.0 / self.steps().len() as f32;
            (index as f32 * unit, (index + 1) as f32 * unit)
        }
    }
    pub fn steps(self) -> &'static [Step] {
        use Step::*;
        match self {
            Self::Recovery => &[
                Check,
                Device,
                Backup,
                StageRecovery,
                Twrp,
                DeviceInfo,
                Edl,
                Originals,
                Firmware,
                Finish,
            ],
            Self::Restore => &[Check, Edl, Originals, Finish],
            Self::Bootloader => &[
                Check,
                Device,
                Backup,
                StageRecovery,
                Twrp,
                DeviceInfo,
                Edl,
                Originals,
                Finish,
            ],
            Self::Dump => &[
                Check,
                Device,
                Backup,
                StageRecovery,
                Twrp,
                Dump,
                Edl,
                Originals,
                Finish,
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunProgress {
    pub flow: Flow,
    pub current: usize,
    percent: f32,
    complete: bool,
}
impl RunProgress {
    pub fn new(flow: Flow) -> Self {
        Self {
            flow,
            current: 0,
            percent: 0.0,
            complete: false,
        }
    }
    pub fn step(&mut self, step: Step) {
        if self.complete {
            return;
        }
        if let Some(index) = self.flow.steps().iter().position(|s| *s == step) {
            // Delayed state/log events must never move the UI backwards.
            self.current = self.current.max(index);
            self.percent = self.percent.max(self.flow.bounds(self.current).0);
        }
    }
    pub fn recovery_phase(&mut self, phase: Phase) {
        if self.flow == Flow::Restore {
            return;
        }
        let step = match phase {
            Phase::Planned | Phase::WritePending | Phase::RegionVerified
                if self.flow == Flow::Dump =>
            {
                Step::Dump
            }
            Phase::Preflight => Step::Device,
            Phase::BackedUp | Phase::Staging | Phase::Staged => Step::StageRecovery,
            Phase::RecoveryRequested | Phase::RecoveryWaiting => Step::Twrp,
            Phase::Planned | Phase::WritePending | Phase::RegionVerified => Step::DeviceInfo,
            Phase::EdlRequested | Phase::EdlWaiting => Step::Edl,
            Phase::Restoring | Phase::OriginalsRestored => Step::Originals,
            Phase::FirmwareWriting => Step::Firmware,
            Phase::FinalReboot | Phase::Done | Phase::Restored => Step::Finish,
        };
        self.step(step);
    }
    pub fn firmware_bytes(&mut self, completed: u64, total: u64) {
        if self.complete || self.flow.steps()[self.current] != Step::Firmware || total == 0 {
            return;
        }
        let fraction = (completed as f64 / total as f64).clamp(0.0, 1.0);
        let (start, end) = self.flow.bounds(self.current);
        let value = start as f64 + fraction * (end - start) as f64;
        self.percent = self.percent.max(value as f32).min(99.0);
    }
    pub fn finish(&mut self, success: bool) {
        if success {
            self.current = self.flow.steps().len() - 1;
            self.percent = 100.0;
            self.complete = true;
        }
    }
    pub fn percent(&self) -> f32 {
        self.percent
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_milestone_is_visible_and_only_success_reaches_one_hundred() {
        for flow in [Flow::Recovery, Flow::Restore, Flow::Dump, Flow::Bootloader] {
            let mut progress = RunProgress::new(flow);
            for (index, step) in flow.steps().iter().enumerate() {
                progress.step(*step);
                assert_eq!(progress.current, index);
                assert!(progress.percent() < 100.0);
            }
            let stopped_at = progress.percent();
            progress.finish(false);
            assert_eq!(progress.percent(), stopped_at);
            progress.finish(true);
            assert_eq!(progress.percent(), 100.0);
            assert!(progress.complete());
        }
    }
    #[test]
    fn firmware_transfer_occupies_its_own_step_and_leaves_finalization() {
        let mut p = RunProgress::new(Flow::Recovery);
        p.firmware_bytes(100, 100); // Stale transfer before firmware.
        assert_eq!(p.percent(), 0.0);
        p.recovery_phase(Phase::FirmwareWriting);
        assert_eq!(p.percent(), 20.0);
        p.firmware_bytes(50, 100);
        assert_eq!(p.percent(), 59.5);
        p.firmware_bytes(100, 0);
        p.firmware_bytes(20, 100); // New/repeated transfer sample.
        assert_eq!(p.percent(), 59.5);
        p.firmware_bytes(u64::MAX, 100);
        assert_eq!(p.percent(), 99.0);
        p.recovery_phase(Phase::FinalReboot);
        p.firmware_bytes(1, 1);
        assert_eq!(p.percent(), 99.0);
        assert_eq!(p.flow.steps()[p.current], Step::Finish);
    }
    #[test]
    fn resume_waits_and_delayed_events_never_fake_or_reverse_progress() {
        let mut p = RunProgress::new(Flow::Recovery);
        p.recovery_phase(Phase::EdlWaiting);
        assert_eq!(p.percent(), 14.0);
        for _ in 0..400 {
            p.recovery_phase(Phase::EdlWaiting);
            p.recovery_phase(Phase::RecoveryWaiting);
        }
        assert_eq!(p.percent(), 14.0);
        assert_eq!(p.flow.steps()[p.current], Step::Edl);
        p.finish(true);
        p.recovery_phase(Phase::FirmwareWriting);
        assert_eq!(p.percent(), 100.0);
    }
}
