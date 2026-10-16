//! S1/S1-E/S2 safety and ordering with an in-memory device. Never probes USB.

#[test]
fn reset_does_not_require_a_rollback_floor_but_still_requires_model_and_slot() {
    let mut d = Fake::new(Connection::Fastboot);
    d.vars.rollback_indices.clear();
    assert!(
        prepare::prepare_with_mode(
            &mut d,
            &firmware(),
            &mut |_| Ok(()),
            lapiswitch_core::deviceinfo::RollbackMode::Reset,
            &mut vec![]
        )
        .is_ok()
    );
    let mut bad = Fake::new(Connection::Fastboot);
    bad.vars.rollback_indices.clear();
    bad.vars.current_slot = None;
    assert!(
        prepare::prepare_with_mode(
            &mut bad,
            &firmware(),
            &mut |_| Ok(()),
            lapiswitch_core::deviceinfo::RollbackMode::Reset,
            &mut vec![]
        )
        .is_err()
    );
}

#[test]
fn reset_option_allows_lower_firmware_but_keep_still_blocks_it() {
    for mode in [
        lapiswitch_core::deviceinfo::RollbackMode::Reset,
        lapiswitch_core::deviceinfo::RollbackMode::Keep,
    ] {
        let mut d = Fake::new(Connection::Fastboot);
        d.vars.rollback_indices = HashMap::from([(2, 100), (3, 100)]);
        let result =
            prepare::prepare_with_mode(&mut d, &firmware(), &mut |_| Ok(()), mode, &mut vec![]);
        assert_eq!(
            result.is_ok(),
            mode == lapiswitch_core::deviceinfo::RollbackMode::Reset
        );
        if mode == lapiswitch_core::deviceinfo::RollbackMode::Keep {
            assert!(!d.contains("open_edl"));
        }
    }
}

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use lapiswitch_core::avb_info::{AvbSummary, KeyClass};
use lapiswitch_core::firmware::FirmwareReport;
use lapiswitch_core::prepare::{
    self, AdbIdentity, Arrival, Connection, DeviceIo, PrepareError, PreparedDevice,
};
use lapiswitch_core::region::Region;
use lapiswitch_core::rollback::{DeviceFloors, RollbackIndices};
use lapiswitch_core::slot::Slot;
use ltbox_device::fastboot::FastbootVars;

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
    edl_slot: Slot,
    vars: FastbootVars,
    adb: AdbIdentity,
    arrival: Arrival,
    images: HashMap<String, Option<AvbSummary>>,
    fail: Vec<&'static str>,
    calls: Vec<String>,
}

impl Fake {
    fn new(started_in: Connection) -> Self {
        Self {
            started_in,
            edl_slot: Slot::A,
            vars: FastbootVars {
                model: Some("TB520FU".into()), current_slot: Some("_a".into()),
                rollback_indices: HashMap::from([(1, 1), (2, 10), (3, 10)]),
                raw_getvar_all: "modelname:TB520FU\ncurrent-slot:a\nstored_rollback_index:2 = A\nstored_rollback_index:3 = A".into(),
                ..FastbootVars::default()
            },
            adb: AdbIdentity { model: Some("TB520FU".into()), slot: Some(Slot::A) },
            arrival: Arrival::Adb,
            images: HashMap::from([
                ("vbmeta_system_a".into(), Some(avb(10, "TB520FU"))),
                ("vbmeta_system_b".into(), Some(avb(9, "TB520FU"))),
                ("boot_a".into(), Some(avb(10, "TB520FU"))),
                ("boot_b".into(), Some(avb(9, "TB520FU"))),
            ]),
            fail: vec![], calls: vec![],
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

    fn run(&mut self) -> Result<PreparedDevice<usize>, PrepareError> {
        prepare::prepare(self, &firmware(), &mut |_| Ok(()), &mut vec![])
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
    fn adb_reboot(&mut self, target: &str) -> Result<(), String> {
        self.call(&format!("adb_reboot_{target}"))
    }
    fn wait_fastboot(&mut self, timeout: Duration) -> Result<(), String> {
        assert_eq!(timeout, Duration::from_secs(60));
        self.call("wait_fastboot")
    }
    fn fastboot_vars(&mut self) -> Result<FastbootVars, String> {
        self.call("fastboot_vars")?;
        Ok(self.vars.clone())
    }
    fn reboot_system(&mut self) -> Result<(), String> {
        self.call("reboot_system")
    }
    fn wait_adb_or_edl(&mut self) -> Result<Arrival, String> {
        self.call("wait_adb_or_edl")?;
        Ok(self.arrival)
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
    fn edl_active_slot(&mut self, _: &mut usize, _: &mut Vec<String>) -> Result<Slot, String> {
        self.call("gpt_slot")?;
        Ok(self.edl_slot)
    }
    fn reset_to_edl(&mut self, session: &mut usize, _: &mut Vec<String>) -> Result<(), String> {
        assert_eq!(*session, 42);
        self.call("reset_edl")
    }
}

#[test]
fn adb_flow_checks_then_saves_getvar_before_entering_edl() {
    let mut device = Fake::new(Connection::Adb);
    let mut saved = String::new();
    let prepared = prepare::prepare(
        &mut device,
        &firmware(),
        &mut |raw| {
            saved = raw.into();
            Ok(())
        },
        &mut vec![],
    )
    .unwrap();
    assert_eq!(prepared.report.slot, Slot::A);
    assert!(!prepared.report.slot_estimated);
    assert_eq!(prepared.session, 42);
    assert_eq!(prepared.report.getvar_all.as_deref(), Some(saved.as_str()));
    let important: Vec<_> = device
        .calls
        .iter()
        .filter(|name| name.as_str() != "single")
        .map(String::as_str)
        .collect();
    assert_eq!(
        important,
        [
            "detect",
            "adb_identity",
            "adb_reboot_bootloader",
            "wait_fastboot",
            "fastboot_vars",
            "reboot_system",
            "wait_adb_or_edl",
            "adb_reboot_edl",
            "wait_edl",
            "open_edl"
        ]
    );
}

#[test]
fn bootloader_response_failure_is_accepted_if_fastboot_arrives() {
    let mut device = Fake::new(Connection::Adb);
    device.fail.push("adb_reboot_bootloader");
    assert!(device.run().is_ok());
}

#[test]
fn downgrade_error_and_comparison_logs_use_exact_decimal_rollback_indices() {
    let mut device = Fake::new(Connection::Fastboot);
    device.vars.rollback_indices = HashMap::from([(2, 1736035200), (3, 1736035200)]);
    let mut package = firmware();
    package.indices = RollbackIndices {
        boot: 1730764800,
        vbmeta_system: 1730764800,
    };
    let mut log = vec![];
    let error = prepare::prepare(&mut device, &package, &mut |_| Ok(()), &mut log)
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "이 펌웨어는 기기보다 롤백인덱스가 낮습니다. 롤백인덱스가 같거나 더 높은 펌웨어를 선택하세요. boot: 펌웨어 1730764800 < 기기 1736035200 / vbmeta_system: 펌웨어 1730764800 < 기기 1736035200"
    );
    assert!(
        log.iter()
            .any(|line| line == "롤백 비교 boot: 펌웨어 1730764800, 기기 1736035200")
    );
    assert!(
        log.iter()
            .any(|line| line == "롤백 비교 vbmeta_system: 펌웨어 1730764800, 기기 1736035200")
    );
    assert!(!device.contains("open_edl"));
}

#[test]
fn fastboot_start_skips_adb_identity_and_bootloader_reboot() {
    let mut device = Fake::new(Connection::Fastboot);
    assert!(device.run().is_ok());
    assert!(!device.contains("adb_identity"));
    assert!(!device.contains("adb_reboot_bootloader"));
}

#[test]
fn downgrade_keeps_getvar_and_reboots_without_entering_edl() {
    let mut device = Fake::new(Connection::Fastboot);
    device.vars.rollback_indices.insert(3, 30);
    let mut saved = false;
    let error = prepare::prepare(
        &mut device,
        &firmware(),
        &mut |_| {
            saved = true;
            Ok(())
        },
        &mut vec![],
    )
    .err()
    .unwrap();
    assert!(matches!(error, PrepareError::Downgrade { .. }));
    assert!(saved);
    assert!(device.contains("reboot_system"));
    assert!(!device.contains("wait_adb_or_edl"));
    assert!(!device.contains("open_edl"));
}

#[test]
fn empty_or_unreadable_fastboot_floors_never_pass_as_stock() {
    for empty_raw in [false, true] {
        let mut device = Fake::new(Connection::Fastboot);
        if empty_raw {
            device.vars.raw_getvar_all.clear();
        } else {
            device.vars.rollback_indices.clear();
        }
        assert!(matches!(device.run(), Err(PrepareError::MissingRollback)));
        assert!(!device.contains("open_edl"));
    }
}

#[test]
fn actual_zero_one_floors_are_allowed() {
    let mut device = Fake::new(Connection::Fastboot);
    device.vars.rollback_indices = HashMap::from([(0, 0), (1, 1), (2, 0), (3, 0)]);
    assert_eq!(device.run().unwrap().report.floors, DeviceFloors::None);
}

#[test]
fn unclassified_floor_is_conservative() {
    let mut device = Fake::new(Connection::Fastboot);
    device.vars.rollback_indices.insert(4, 21);
    assert!(matches!(device.run(), Err(PrepareError::Downgrade { .. })));
    assert!(!device.contains("open_edl"));
}

#[test]
fn adb_and_fastboot_slot_disagreement_blocks() {
    let mut device = Fake::new(Connection::Adb);
    device.vars.current_slot = Some("b".into());
    assert!(matches!(
        device.run(),
        Err(PrepareError::SlotMismatch { .. })
    ));
    assert!(!device.contains("open_edl"));
}

#[test]
fn missing_or_invalid_fastboot_slot_blocks() {
    for slot in [None, Some("c".into())] {
        let mut device = Fake::new(Connection::Fastboot);
        device.vars.current_slot = slot;
        assert!(matches!(device.run(), Err(PrepareError::MissingSlot)));
    }
}

#[test]
fn incorrect_adb_model_is_rejected_before_bootloader_transition() {
    let mut device = Fake::new(Connection::Adb);
    device.adb.model = Some("TB520FUX".into());
    assert!(matches!(device.run(), Err(PrepareError::WrongModel(_))));
    assert!(!device.contains("adb_reboot_bootloader"));
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
fn fastboot_model_must_be_the_dedicated_modelname() {
    let mut device = Fake::new(Connection::Fastboot);
    device.vars.model = Some("TB323FU".into());
    device.vars.product = Some("TB520FU".into());
    assert!(matches!(device.run(), Err(PrepareError::WrongModel(_))));
    assert!(!device.contains("open_edl"));
}

#[test]
fn save_failure_prevents_edl_transition() {
    let mut device = Fake::new(Connection::Fastboot);
    let result = prepare::prepare(
        &mut device,
        &firmware(),
        &mut |_| Err("disk full".into()),
        &mut vec![],
    );
    assert!(matches!(result, Err(PrepareError::SaveGetvar(_))));
    assert!(!device.contains("wait_adb_or_edl"));
}

#[test]
fn fastboot_timeout_stops_before_s2() {
    let mut device = Fake::new(Connection::Adb);
    device.fail.push("wait_fastboot");
    assert!(
        device
            .run()
            .err()
            .unwrap()
            .to_string()
            .contains("만약 기기가 Bootloader 모드라면 수동으로 재부팅한 뒤 다시 시도해 주세요.")
    );
    assert!(!device.contains("fastboot_vars"));
    assert!(!device.contains("open_edl"));
}

#[test]
fn edl_arriving_before_android_skips_adb_reboot() {
    let mut device = Fake::new(Connection::Fastboot);
    device.arrival = Arrival::Edl;
    assert!(device.run().is_ok());
    assert!(!device.contains("adb_reboot_edl"));
    assert!(device.contains("wait_edl"));
}

#[test]
fn manual_edl_can_recover_transition_failures() {
    for failed in ["reboot_system", "wait_adb_or_edl", "adb_reboot_edl"] {
        let mut device = Fake::new(Connection::Fastboot);
        device.fail.push(failed);
        assert!(device.run().is_ok(), "{failed}");
        assert!(device.contains("wait_edl"));
    }
}

#[test]
fn edl_timeout_and_handshake_failure_surface_without_writes() {
    for failed in ["wait_edl", "open_edl"] {
        let mut device = Fake::new(Connection::Fastboot);
        device.fail.push(failed);
        assert!(device.run().is_err());
        assert!(!device.contains("boot_a"));
    }
}

#[test]
fn no_device_or_ambiguous_usb_never_reboots() {
    let mut device = Fake::new(Connection::None);
    assert!(matches!(device.run(), Err(PrepareError::NoDevice)));
    assert!(!device.contains("reboot_system"));
    device.fail.push("single");
    assert!(device.run().is_err());
    assert!(!device.contains("open_edl"));
}

#[test]
fn edl_start_reuses_the_session_and_takes_component_maximum() {
    let mut device = Fake::new(Connection::Edl);
    device.edl_slot = Slot::B;
    device
        .images
        .insert("vbmeta_system_b".into(), Some(avb(19, "TB520FU")));
    device
        .images
        .insert("boot_a".into(), Some(avb(20, "TB520FU")));
    let prepared = device.run().unwrap();
    assert_eq!(prepared.report.slot, Slot::B);
    assert!(!prepared.report.slot_estimated);
    assert_eq!(
        prepared.report.floors,
        DeviceFloors::PerLocation(RollbackIndices {
            boot: 20,
            vbmeta_system: 19
        })
    );
    assert!(prepared.report.getvar_all.is_none());
    assert_eq!(
        device
            .calls
            .iter()
            .filter(|name| name.as_str() == "open_edl")
            .count(),
        1
    );
    assert!(!device.contains("reboot_system"));
    assert!(!device.contains("wait_edl"));
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
        device.edl_slot = expected;
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
fn edl_missing_images_can_be_valid_on_different_slots() {
    let mut device = Fake::new(Connection::Edl);
    device.edl_slot = Slot::B;
    device.images.insert("vbmeta_system_a".into(), None);
    device.images.insert("boot_b".into(), None);
    assert!(device.run().is_ok());
}

#[test]
fn edl_transport_error_is_never_treated_as_an_absent_or_invalid_slot() {
    for failed in [
        "gpt_slot",
        "vbmeta_system_a",
        "vbmeta_system_b",
        "boot_a",
        "boot_b",
    ] {
        let mut device = Fake::new(Connection::Edl);
        device.fail.push(failed);
        assert!(device.run().is_err());
        assert!(device.contains("reset_edl"));
        assert!(!device.contains("reboot_system"));
    }
}

#[test]
fn edl_mismatch_missing_boot_and_downgrade_reset_back_to_edl() {
    for failure in ["model", "boot", "downgrade"] {
        let mut device = Fake::new(Connection::Edl);
        match failure {
            "model" => {
                device
                    .images
                    .insert("vbmeta_system_a".into(), Some(avb(10, "TB323FU")));
                device.images.insert("vbmeta_system_b".into(), None);
            }
            "boot" => {
                device.images.insert("boot_a".into(), None);
                device.images.insert("boot_b".into(), None);
            }
            _ => {
                device
                    .images
                    .insert("boot_b".into(), Some(avb(21, "TB520FU")));
            }
        }
        assert!(device.run().is_err(), "{failure}");
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
fn reset_failure_keeps_the_original_downgrade_error() {
    let mut device = Fake::new(Connection::Edl);
    device
        .images
        .insert("boot_a".into(), Some(avb(21, "TB520FU")));
    device.fail.push("reset_edl");
    let mut log = vec![];
    let result = prepare::prepare(
        &mut device,
        &firmware(),
        &mut |_| panic!("EDL must not save getvar"),
        &mut log,
    );
    assert!(matches!(result, Err(PrepareError::Downgrade { .. })));
    assert!(log.iter().any(|line| line.contains("EDL 복귀 실패")));
}

#[test]
fn edl_start_confirmation_is_required_before_open_and_cancellation_does_not_reset() {
    for accept in [false, true] {
        let mut d = Fake::new(Connection::Edl);
        let mut asked = 0;
        let result = prepare::prepare_with_confirmation(
            &mut d,
            &firmware(),
            &mut |_| Ok(()),
            lapiswitch_core::deviceinfo::RollbackMode::Reset,
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
    let mut d = Fake::new(Connection::Fastboot);
    prepare::prepare_with_confirmation(
        &mut d,
        &firmware(),
        &mut |_| Ok(()),
        lapiswitch_core::deviceinfo::RollbackMode::Reset,
        &mut || panic!("only EDL starts ask"),
        &mut vec![],
    )
    .unwrap();
}
#[test]
fn gpt_active_slot_with_wrong_model_cannot_borrow_the_other_slots_identity() {
    let mut d = Fake::new(Connection::Edl);
    d.edl_slot = Slot::B;
    d.images
        .insert("vbmeta_system_b".into(), Some(avb(10, "TB323FU")));
    assert!(matches!(d.run(), Err(PrepareError::WrongModel(_))));
}
