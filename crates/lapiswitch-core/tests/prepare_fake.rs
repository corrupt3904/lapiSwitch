//! S1/S1-E/S2 safety and ordering with an in-memory device. Never probes USB.

use std::collections::HashMap;
use std::path::Path;

use lapiswitch_core::avb_info::{AvbSummary, KeyClass};
use lapiswitch_core::firmware::FirmwareReport;
use lapiswitch_core::prepare::{
    self, AdbIdentity, Connection, DeviceIo, PrepareError, PreparedDevice,
};
use lapiswitch_core::region::Region;
use lapiswitch_core::rollback::RollbackIndices;
use lapiswitch_core::slot::Slot;

fn firmware() -> FirmwareReport {
    FirmwareReport {
        image_dir: "firmware/image".into(),
        loader: "loader.melf".into(),
        region: Region::Row,
        fingerprint: "qti/TB520FU/lapis:16/test_ROW:user/release-keys".into(),
        key_class: KeyClass::Lenovo,
        indices: RollbackIndices {
            boot: 20,
            vbmeta_system: 20,
        },
        raw_xmls: vec![],
        patch_xmls: vec![],
        decrypted: 0,
        missing_images: vec![],
        lun4_gpt_in_package: true,
    }
}

fn avb(index: u64, model: &str) -> AvbSummary {
    AvbSummary {
        rollback_index: index,
        rollback_index_location: 0,
        public_key_sha1: None,
        props: vec![(
            "com.android.build.system.fingerprint".into(),
            format!("qti/{model}/{model}:16/build_ROW:user/release-keys").into_bytes(),
        )],
    }
}

struct Fake {
    started_in: Connection,
    /// GPT slot-control result; `None` models blank (fresh-table) flags.
    gpt_slot: Option<Slot>,
    adb: AdbIdentity,
    serial: String,
    images: HashMap<String, Option<AvbSummary>>,
    fail: Vec<&'static str>,
    calls: Vec<String>,
}

impl Fake {
    fn new(started_in: Connection) -> Self {
        Self {
            started_in,
            gpt_slot: Some(Slot::A),
            adb: AdbIdentity {
                model: Some("TB520FU".into()),
                slot: Some(Slot::A),
            },
            serial: "FAKE0001".into(),
            images: HashMap::from([
                ("vbmeta_system_a".into(), Some(avb(10, "TB520FU"))),
                ("vbmeta_system_b".into(), Some(avb(9, "TB520FU"))),
            ]),
            fail: vec![],
            calls: vec![],
        }
    }

    fn call(&mut self, name: &str) -> Result<(), String> {
        self.calls.push(name.into());
        if self.fail.contains(&name) {
            Err(format!("injected {name} error"))
        } else {
            Ok(())
        }
    }

    fn contains(&self, call: &str) -> bool {
        self.calls.iter().any(|name| name == call)
    }

    fn important(&self) -> Vec<&str> {
        self.calls
            .iter()
            .filter(|name| name.as_str() != "single")
            .map(String::as_str)
            .collect()
    }

    fn run(&mut self) -> Result<PreparedDevice<usize>, PrepareError> {
        prepare::prepare(self, &firmware().loader, &mut |_| Ok(()), &mut vec![])
    }
}

impl DeviceIo for Fake {
    type Session = usize;
    fn ensure_single_target(&mut self) -> Result<(), String> {
        self.call("single")
    }
    fn detect(&mut self) -> Result<Connection, String> {
        self.call("detect")?;
        Ok(self.started_in)
    }
    fn adb_identity(&mut self) -> Result<AdbIdentity, String> {
        self.call("adb_identity")?;
        Ok(self.adb.clone())
    }
    fn adb_serial(&mut self) -> Result<String, String> {
        self.call("adb_serial")?;
        Ok(self.serial.clone())
    }
    fn adb_reboot(&mut self, target: &str) -> Result<(), String> {
        self.call(&format!("adb_reboot_{target}"))
    }
    fn wait_edl(&mut self) -> Result<(), String> {
        self.call("wait_edl")
    }
    fn open_edl(&mut self, _: &Path, _: &mut Vec<String>) -> Result<usize, String> {
        self.call("open_edl")?;
        Ok(42)
    }
    fn read_avb(
        &mut self,
        session: &mut usize,
        partition: &str,
        _: &mut Vec<String>,
    ) -> Result<Option<AvbSummary>, String> {
        assert_eq!(*session, 42);
        self.call(partition)?;
        Ok(self.images.get(partition).cloned().flatten())
    }
    /// Same rule as `PrimaryGpt::active_slot`.
    fn edl_active_slot(
        &mut self,
        _: &mut usize,
        expected: Option<Slot>,
        _: &mut Vec<String>,
    ) -> Result<Slot, String> {
        self.call("gpt_slot")?;
        match self.gpt_slot {
            None => expected.ok_or_else(|| "blank GPT slot flags".into()),
            Some(slot) if expected.is_some_and(|adb| adb != slot) => {
                Err("ADB/GPT slot mismatch".into())
            }
            Some(slot) => Ok(slot),
        }
    }
    fn reset_to_edl(&mut self, session: &mut usize, _: &mut Vec<String>) -> Result<(), String> {
        assert_eq!(*session, 42);
        self.call("reset_edl")
    }
}

#[test]
fn adb_flow_saves_the_identity_then_reboots_straight_to_edl() {
    let mut device = Fake::new(Connection::Adb);
    device.adb.slot = Some(Slot::B);
    device.gpt_slot = Some(Slot::B);
    device
        .images
        .insert("vbmeta_system_b".into(), Some(avb(10, "TB520FU")));
    let mut saved = String::new();
    let prepared = prepare::prepare(
        &mut device,
        &firmware().loader,
        &mut |raw| {
            saved = raw.into();
            Ok(())
        },
        &mut vec![],
    )
    .unwrap();
    assert_eq!(prepared.report.started_in, Connection::Adb);
    assert_eq!(prepared.report.slot, Slot::B);
    assert_eq!(prepared.session, 42);
    assert_eq!(prepared.report.identity.as_deref(), Some(saved.as_str()));
    assert_eq!(saved, prepare::identity_record(Slot::B, "FAKE0001"));
    assert!(saved.contains("serialno: FAKE0001\n"));
    assert!(saved.contains("current-slot: _b\n"));
    assert_eq!(
        device.important(),
        [
            "detect",
            "adb_identity",
            "adb_serial",
            "adb_reboot_edl",
            "wait_edl",
            "open_edl",
            "gpt_slot",
            "vbmeta_system_a",
            "vbmeta_system_b",
        ]
    );
}

#[test]
fn fastboot_start_is_refused_before_any_device_command() {
    let mut device = Fake::new(Connection::Fastboot);
    let mut saved = false;
    let result = prepare::prepare_with_confirmation(
        &mut device,
        &firmware().loader,
        &mut |_| {
            saved = true;
            Ok(())
        },
        &mut || panic!("only EDL starts ask"),
        &mut vec![],
    );
    assert!(matches!(result, Err(PrepareError::FastbootUnsupported)));
    assert!(result.err().unwrap().to_string().contains("ADB로 연결"));
    assert!(!saved);
    assert_eq!(device.important(), ["detect"]);
}

#[test]
fn edl_reboot_response_failure_is_accepted_if_edl_arrives() {
    let mut device = Fake::new(Connection::Adb);
    device.fail.push("adb_reboot_edl");
    assert!(device.run().is_ok());
    assert!(device.contains("wait_edl"));
}

#[test]
fn incorrect_adb_model_is_rejected_before_serial_save_or_reboot() {
    for model in [Some("TB520FUX"), Some("TB321FU"), None] {
        let mut device = Fake::new(Connection::Adb);
        device.adb.model = model.map(Into::into);
        let mut saved = false;
        let result = prepare::prepare(
            &mut device,
            &firmware().loader,
            &mut |_| {
                saved = true;
                Ok(())
            },
            &mut vec![],
        );
        assert!(
            matches!(result, Err(PrepareError::WrongModel(_))),
            "{model:?}"
        );
        assert!(!saved);
        assert!(!device.contains("adb_serial"));
        assert!(!device.contains("adb_reboot_edl"));
    }
}

#[test]
fn adb_market_model_with_lenovo_prefix_and_lapis_codename_are_accepted() {
    for model in ["Lenovo TB520FU", "TB520FU", "lapis"] {
        let mut device = Fake::new(Connection::Adb);
        device.adb.model = Some(model.into());
        assert!(device.run().is_ok(), "{model}");
    }
}

#[test]
fn missing_adb_slot_blocks_before_the_edl_reboot() {
    let mut device = Fake::new(Connection::Adb);
    device.adb.slot = None;
    assert!(matches!(device.run(), Err(PrepareError::MissingSlot)));
    assert!(!device.contains("adb_serial"));
    assert!(!device.contains("adb_reboot_edl"));
}

#[test]
fn empty_multiline_or_unreadable_serial_blocks_before_save_and_reboot() {
    for case in ["empty", "newline", "carriage", "error"] {
        let mut device = Fake::new(Connection::Adb);
        match case {
            "empty" => device.serial = "  ".into(),
            "newline" => device.serial = "ONE\nTWO".into(),
            "carriage" => device.serial = "ONE\rTWO".into(),
            _ => device.fail.push("adb_serial"),
        }
        let mut saved = false;
        let result = prepare::prepare(
            &mut device,
            &firmware().loader,
            &mut |_| {
                saved = true;
                Ok(())
            },
            &mut vec![],
        );
        assert!(result.is_err(), "{case}");
        if case != "error" {
            assert!(matches!(result, Err(PrepareError::MissingSerial)), "{case}");
        }
        assert!(!saved, "{case}");
        assert!(!device.contains("adb_reboot_edl"), "{case}");
    }
}

#[test]
fn surrounding_serial_whitespace_is_not_recorded() {
    let mut device = Fake::new(Connection::Adb);
    device.serial = " FAKE0001 \n".into();
    let prepared = device.run().unwrap();
    assert_eq!(
        prepared.report.identity.unwrap(),
        prepare::identity_record(Slot::A, "FAKE0001")
    );
}

#[test]
fn identity_save_failure_keeps_the_device_in_android() {
    let mut device = Fake::new(Connection::Adb);
    let result = prepare::prepare(
        &mut device,
        &firmware().loader,
        &mut |_| Err("disk full".into()),
        &mut vec![],
    );
    assert!(matches!(result, Err(PrepareError::SaveIdentity(_))));
    assert!(!device.contains("adb_reboot_edl"));
    assert!(!device.contains("wait_edl"));
}

#[test]
fn edl_timeout_and_handshake_failure_surface_without_reads() {
    for failed in ["wait_edl", "open_edl"] {
        let mut device = Fake::new(Connection::Adb);
        device.fail.push(failed);
        assert!(device.run().is_err());
        assert!(!device.contains("gpt_slot"));
        assert!(!device.contains("vbmeta_system_a"));
        assert!(!device.contains("reset_edl"));
    }
}

#[test]
fn adb_slot_must_agree_with_set_gpt_slot_flags() {
    for (adb, gpt) in [(Slot::A, Slot::B), (Slot::B, Slot::A)] {
        let mut device = Fake::new(Connection::Adb);
        device.adb.slot = Some(adb);
        device.gpt_slot = Some(gpt);
        assert!(device.run().is_err(), "{adb:?}/{gpt:?}");
        assert!(device.contains("reset_edl"));
        assert!(!device.contains("vbmeta_system_a"));
    }
}

#[test]
fn blank_gpt_slot_flags_use_the_adb_slot() {
    for slot in [Slot::A, Slot::B] {
        let mut device = Fake::new(Connection::Adb);
        device.adb.slot = Some(slot);
        device.gpt_slot = None;
        device.images.insert(
            format!("vbmeta_system{}", slot.suffix()),
            Some(avb(10, "TB520FU")),
        );
        let prepared = device.run().unwrap();
        assert_eq!(prepared.report.slot, slot);
        assert!(prepared.report.identity.unwrap().contains(slot.suffix()));
    }
}

#[test]
fn blank_gpt_slot_flags_block_an_edl_start() {
    let mut device = Fake::new(Connection::Edl);
    device.gpt_slot = None;
    assert!(device.run().is_err());
    assert!(device.contains("reset_edl"));
    assert!(!device.contains("vbmeta_system_a"));
}

#[test]
fn no_device_or_ambiguous_usb_never_reboots() {
    let mut device = Fake::new(Connection::None);
    assert!(matches!(device.run(), Err(PrepareError::NoDevice)));
    assert!(!device.contains("adb_reboot_edl"));
    device.fail.push("single");
    assert!(device.run().is_err());
    assert!(!device.contains("open_edl"));
    let mut device = Fake::new(Connection::Adb);
    device.fail.push("single");
    assert!(device.run().is_err());
    assert!(!device.contains("adb_identity"));
}

#[test]
fn edl_start_reuses_the_session_without_an_identity_record() {
    let mut device = Fake::new(Connection::Edl);
    device.gpt_slot = Some(Slot::B);
    device
        .images
        .insert("vbmeta_system_b".into(), Some(avb(19, "TB520FU")));
    let prepared = device.run().unwrap();
    assert_eq!(prepared.report.started_in, Connection::Edl);
    assert_eq!(prepared.report.slot, Slot::B);
    assert!(prepared.report.device_key.is_some());
    assert!(prepared.report.identity.is_none());
    assert_eq!(
        device.important(),
        [
            "detect",
            "open_edl",
            "gpt_slot",
            "vbmeta_system_a",
            "vbmeta_system_b"
        ]
    );
}

#[test]
fn edl_slot_uses_gpt_even_if_rollback_indices_suggest_the_other_slot() {
    for (a, b, expected) in [
        (Some(10), Some(10), Slot::A),
        (Some(11), Some(10), Slot::A),
        (Some(9), Some(10), Slot::A),
        (Some(10), None, Slot::A),
        (None, Some(10), Slot::B),
    ] {
        let mut device = Fake::new(Connection::Edl);
        device.gpt_slot = Some(expected);
        device.images.insert(
            "vbmeta_system_a".into(),
            a.map(|index| avb(index, "TB520FU")),
        );
        device.images.insert(
            "vbmeta_system_b".into(),
            b.map(|index| avb(index, "TB520FU")),
        );
        assert_eq!(device.run().unwrap().report.slot, expected);
    }
}

#[test]
fn inactive_slot_vbmeta_system_may_be_missing() {
    let mut device = Fake::new(Connection::Edl);
    device.gpt_slot = Some(Slot::B);
    device.images.insert("vbmeta_system_a".into(), None);
    assert!(device.run().is_ok());
}

#[test]
fn s1_never_reads_boot_or_compares_rollback_indices() {
    // vbmeta_system images far above the firmware's indices used to block
    // Keep at S1; the stored DeviceInfo values decide in TWRP now.
    for start in [Connection::Adb, Connection::Edl] {
        let mut device = Fake::new(start);
        for name in ["vbmeta_system_a", "vbmeta_system_b"] {
            device
                .images
                .insert(name.into(), Some(avb(u64::MAX, "TB520FU")));
        }
        let mut log = vec![];
        let prepared = prepare::prepare(&mut device, &firmware().loader, &mut |_| Ok(()), &mut log);
        assert!(prepared.is_ok(), "{start:?}");
        assert!(!device.contains("boot_a"));
        assert!(!device.contains("boot_b"));
        assert!(!log.iter().any(|line| line.contains("롤백")), "{log:?}");
    }
}

#[test]
fn edl_transport_error_is_never_treated_as_an_absent_or_invalid_slot() {
    for start in [Connection::Edl, Connection::Adb] {
        for failed in ["gpt_slot", "vbmeta_system_a", "vbmeta_system_b"] {
            let mut device = Fake::new(start);
            device.fail.push(failed);
            assert!(device.run().is_err());
            assert!(device.contains("reset_edl"), "{start:?} {failed}");
        }
    }
}

#[test]
fn edl_model_mismatch_resets_back_to_edl() {
    for start in [Connection::Edl, Connection::Adb] {
        let mut device = Fake::new(start);
        device
            .images
            .insert("vbmeta_system_a".into(), Some(avb(10, "TB323FU")));
        device.images.insert("vbmeta_system_b".into(), None);
        assert!(matches!(device.run(), Err(PrepareError::WrongModel(_))));
        assert!(device.contains("reset_edl"));
    }
}

#[test]
fn edl_model_prefers_a_matching_fingerprint_even_if_other_slot_differs() {
    let mut device = Fake::new(Connection::Edl);
    device
        .images
        .insert("vbmeta_system_b".into(), Some(avb(10, "TB323FU")));
    assert!(device.run().is_ok());
}

#[test]
fn reset_failure_keeps_the_original_probe_error() {
    let mut device = Fake::new(Connection::Edl);
    device
        .images
        .insert("vbmeta_system_a".into(), Some(avb(10, "TB323FU")));
    device.images.insert("vbmeta_system_b".into(), None);
    device.fail.push("reset_edl");
    let mut log = vec![];
    let result = prepare::prepare(
        &mut device,
        &firmware().loader,
        &mut |_| panic!("EDL must not save an ADB identity"),
        &mut log,
    );
    assert!(matches!(result, Err(PrepareError::WrongModel(_))));
    assert!(log.iter().any(|line| line.contains("EDL 복귀 실패")));
}

#[test]
fn edl_start_confirmation_is_required_before_open_and_cancellation_does_not_reset() {
    for accept in [false, true] {
        let mut d = Fake::new(Connection::Edl);
        let mut asked = 0;
        let result = prepare::prepare_with_confirmation(
            &mut d,
            &firmware().loader,
            &mut |_| Ok(()),
            &mut || {
                asked += 1;
                if accept { Ok(()) } else { Err("취소".into()) }
            },
            &mut vec![],
        );
        assert_eq!(asked, 1);
        assert_eq!(result.is_ok(), accept);
        assert_eq!(d.contains("open_edl"), accept);
        assert!(!d.contains("reset_edl"));
    }
    let mut d = Fake::new(Connection::Adb);
    prepare::prepare_with_confirmation(
        &mut d,
        &firmware().loader,
        &mut |_| Ok(()),
        &mut || panic!("only EDL starts ask"),
        &mut vec![],
    )
    .unwrap();
}

#[test]
fn gpt_active_slot_with_wrong_model_cannot_borrow_the_other_slots_identity() {
    for start in [Connection::Edl, Connection::Adb] {
        let mut d = Fake::new(start);
        d.gpt_slot = Some(Slot::B);
        d.adb.slot = Some(Slot::B);
        d.images
            .insert("vbmeta_system_b".into(), Some(avb(10, "TB323FU")));
        assert!(matches!(d.run(), Err(PrepareError::WrongModel(_))));
    }
}
