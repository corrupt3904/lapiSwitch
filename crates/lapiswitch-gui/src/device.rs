//! LTBox dashboard probes. Detailed probes own USB only while the workflow is
//! idle; passive probes enumerate descriptors without opening/claiming a port.
use ltbox_device::{adb::AdbManager, edl, fastboot::FastbootDevice};
use nusb::MaybeFuture;
use std::time::{Duration, Instant};

pub const INTERVAL: Duration = Duration::from_secs(3);
pub const MAX_AGE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    None,
    Adb,
    Recovery,
    Unauthorized,
    Sideload,
    ServerBlocking,
    Fastboot,
    Fastbootd,
    Edl,
    Usb,
}
impl Mode {
    pub fn reachable(self) -> bool {
        matches!(
            self,
            Self::Adb | Self::Recovery | Self::Fastboot | Self::Fastbootd | Self::Edl
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub mode: Mode,
    pub model: String,
    pub name: String,
    pub android: String,
    pub slot: String,
    pub firmware: String,
    pub ram: String,
    pub storage: String,
    pub serial: String,
    pub error: Option<String>,
    pub cached: bool,
}
impl Snapshot {
    pub fn merge(&mut self, mut next: Self) {
        // Never carry another device's identity through a disconnect or swap.
        let same = next.mode != Mode::None
            && next.error.is_none()
            && (next.serial.is_empty() || self.serial.is_empty() || self.serial == next.serial)
            && (next.model.is_empty() || self.model.is_empty() || self.model == next.model);
        if same {
            for (old, new) in [
                (&self.model, &mut next.model),
                (&self.name, &mut next.name),
                (&self.slot, &mut next.slot),
                (&self.firmware, &mut next.firmware),
                (&self.ram, &mut next.ram),
                (&self.storage, &mut next.storage),
                (&self.serial, &mut next.serial),
            ] {
                if new.is_empty() && !old.is_empty() {
                    *new = old.clone();
                    next.cached = true;
                }
            }
        }
        *self = next;
    }
}

#[derive(Debug)]
pub struct Monitor {
    pub snapshot: Snapshot,
    pub checked: Option<Instant>,
    pub in_flight: Option<u64>,
    pub next: Instant,
    sequence: u64,
}
impl Default for Monitor {
    fn default() -> Self {
        Self {
            snapshot: Snapshot::default(),
            checked: None,
            in_flight: None,
            next: Instant::now(),
            sequence: 0,
        }
    }
}
impl Monitor {
    /// The supported device the connected model names, if any. EDL reports
    /// no model; the device is then identified when the run starts.
    pub fn supported(&self) -> Option<lapiswitch_core::device::Device> {
        lapiswitch_core::device::Device::identify(&self.snapshot.model)
    }
    pub fn unsupported_model(&self) -> bool {
        let model = self.snapshot.model.trim();
        !model.is_empty() && !lapiswitch_core::prepare::supported_model(model)
    }
    /// Conversion starts from Android ADB or EDL. Fastboot is shown as a
    /// connection only; preparation never uses it.
    pub fn fastboot(&self) -> bool {
        matches!(self.snapshot.mode, Mode::Fastboot | Mode::Fastbootd)
    }
    pub fn startable(&self) -> bool {
        self.reachable()
            && !self.fastboot()
            && !self.unsupported_model()
            && (self.snapshot.mode == Mode::Edl
                || lapiswitch_core::prepare::supported_model(&self.snapshot.model))
    }
    pub fn reachable(&self) -> bool {
        self.snapshot.error.is_none()
            && self.snapshot.mode.reachable()
            && self.checked.is_some_and(|time| time.elapsed() < MAX_AGE)
    }
    pub fn begin(&mut self) -> Option<u64> {
        if self.in_flight.is_some() {
            return None;
        }
        self.sequence += 1;
        self.in_flight = Some(self.sequence);
        self.next = Instant::now() + INTERVAL;
        Some(self.sequence)
    }
    pub fn finish(&mut self, id: u64, result: Result<Snapshot, String>) -> bool {
        if self.in_flight != Some(id) {
            return false;
        }
        self.in_flight = None;
        self.next = Instant::now() + INTERVAL;
        self.snapshot.merge(result.unwrap_or_else(|error| Snapshot {
            error: Some(error),
            ..Snapshot::default()
        }));
        self.checked = Some(Instant::now());
        true
    }
}

pub fn probe(passive: bool) -> Result<Snapshot, String> {
    if passive {
        return passive_probe();
    }
    ltbox_device::selection::ensure_single_usb_target().map_err(|e| e.to_string())?;
    let mut adb = AdbManager::new();
    // LTBox falls through to Fastboot/EDL for offline/inaccessible ADB.
    let state = adb.check_device_state().ok().flatten();
    let mode = match state {
        Some("adb_server_blocking") => Some(Mode::ServerBlocking),
        Some("unauthorized" | "authorizing") => Some(Mode::Unauthorized),
        Some("sideload") => Some(Mode::Sideload),
        Some("device" | "recovery") => Some(Mode::Adb),
        _ => None,
    };
    if let Some(mode) = mode {
        let mut result = Snapshot {
            mode,
            serial: adb.serial().unwrap_or_default().into(),
            ..Snapshot::default()
        };
        if mode != Mode::Adb {
            return Ok(result);
        }
        let model = adb.get_model().ok().flatten().unwrap_or_default();
        if model.is_empty() {
            result.mode = Mode::Unauthorized;
            return Ok(result);
        }
        result.mode =
            if model.to_ascii_lowercase().starts_with("twrp_") || state == Some("recovery") {
                Mode::Recovery
            } else {
                Mode::Adb
            };
        result.model = if model.to_ascii_lowercase().starts_with("twrp_") {
            model[5..].into()
        } else {
            model
        };
        result.android = prop(&mut adb, "ro.build.version.release");
        result.slot = adb.get_slot_suffix().ok().flatten().unwrap_or_default();
        result.firmware = trim_build(&prop(&mut adb, "ro.build.display.id"));
        let board = prop(&mut adb, "ro.boot.hwboardid");
        if let Some(caps) = regex::Regex::new(r"(\d+)\+(\d+)")
            .expect("RAM pattern")
            .captures(&board)
        {
            result.ram = format!("{} GB", &caps[1]);
            result.storage = format!("{} GB", &caps[2]);
        }
        result.name = [
            "ro.vendor.config.lgsi.en.market_name",
            "ro.vendor.config.lgsi.market_name",
            "ro.config.lgsi.market_name",
            "ro.vendor.config.lgsi.kirby_en",
        ]
        .into_iter()
        .map(|key| prop(&mut adb, key))
        .find(|v| !v.is_empty())
        .unwrap_or_default();
        return Ok(result);
    }
    drop(adb);
    match FastbootDevice::open() {
        Ok(mut fb) => {
            let mode = if fb.is_userspace() {
                Mode::Fastbootd
            } else {
                Mode::Fastboot
            };
            let vars = fb.get_all_vars().map_err(|e| e.to_string())?;
            Ok(Snapshot {
                mode,
                model: vars.model.unwrap_or_default(),
                name: vars.product.unwrap_or_default(),
                slot: vars.current_slot.unwrap_or_default(),
                firmware: trim_build(&vars.build_display_id.unwrap_or_default()),
                ram: vars.ram_gb.unwrap_or_default(),
                storage: vars.storage_gb.unwrap_or_default(),
                serial: vars.serialno.unwrap_or_default(),
                ..Snapshot::default()
            })
        }
        Err(error) => {
            if edl::check_device() {
                return Ok(Snapshot {
                    mode: Mode::Edl,
                    ..Snapshot::default()
                });
            }
            if matches!(error, ltbox_device::fastboot::FastbootError::DeviceNotFound) {
                Ok(Snapshot::default())
            } else {
                Err(error.to_string())
            }
        }
    }
}
fn prop(adb: &mut AdbManager, key: &str) -> String {
    adb.shell(&format!("getprop {key}"))
        .unwrap_or_default()
        .trim()
        .into()
}
fn trim_build(value: &str) -> String {
    ["_ZUXOS_", "_ZUI_"]
        .iter()
        .find_map(|marker| value.find(marker).map(|i| value[i + 1..].into()))
        .unwrap_or_else(|| value.into())
}

fn passive_probe() -> Result<Snapshot, String> {
    let devices = nusb::list_devices().wait().map_err(|e| e.to_string())?;
    let mut modes = vec![];
    for device in devices {
        let mode = if device.vendor_id() == 0x05c6 && device.product_id() == 0x9008 {
            Some(Mode::Edl)
        } else if device
            .interfaces()
            .any(|i| i.class() == 0xff && i.subclass() == 0x42 && i.protocol() == 3)
        {
            Some(Mode::Fastboot)
        } else if device
            .interfaces()
            .any(|i| i.class() == 0xff && i.subclass() == 0x42 && i.protocol() == 1)
        {
            Some(Mode::Usb)
        } else {
            None
        };
        if let Some(mode) = mode {
            modes.push(mode);
        }
    }
    if modes.len() > 1 {
        return Err("Multiple Android/EDL USB devices found. Disconnect other devices.".into());
    }
    // The Windows Qualcomm kernel binding can expose only its COM metadata.
    let mode = modes.first().copied().unwrap_or_else(|| {
        if edl::check_device() {
            Mode::Edl
        } else {
            Mode::None
        }
    });
    Ok(Snapshot {
        mode,
        cached: true,
        ..Snapshot::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_gate_uses_core_aliases_and_keeps_unidentified_edl_for_preflight() {
        let mut monitor = Monitor {
            checked: Some(Instant::now()),
            ..Monitor::default()
        };
        for mode in [
            Mode::Adb,
            Mode::Recovery,
            Mode::Fastboot,
            Mode::Fastbootd,
            Mode::Edl,
        ] {
            monitor.snapshot.mode = mode;
            let fastboot = matches!(mode, Mode::Fastboot | Mode::Fastbootd);
            for model in ["TB520FU", "tb520fu", "lapis"] {
                monitor.snapshot.model = model.into();
                assert_eq!(monitor.startable(), !fastboot, "{mode:?}");
                assert_eq!(monitor.fastboot(), fastboot);
            }
            for model in ["TB321FU", "TB520FUX", "TB320FC"] {
                monitor.snapshot.model = model.into();
                assert!(monitor.unsupported_model());
                assert!(!monitor.startable());
            }
            monitor.snapshot.model.clear();
            assert_eq!(monitor.startable(), mode == Mode::Edl);
        }
        monitor.snapshot.model = "TB321FU".into();
        monitor.snapshot.merge(Snapshot {
            mode: Mode::Edl,
            ..Snapshot::default()
        });
        assert!(!monitor.startable());
    }
    #[test]
    fn disconnect_and_serial_swap_clear_identity() {
        let mut s = Snapshot {
            mode: Mode::Adb,
            model: "TB520FU".into(),
            serial: "one".into(),
            firmware: "old".into(),
            ..Snapshot::default()
        };
        s.merge(Snapshot {
            mode: Mode::Edl,
            ..Snapshot::default()
        });
        assert_eq!(s.model, "TB520FU");
        assert!(s.cached);
        s.merge(Snapshot {
            mode: Mode::Adb,
            serial: "two".into(),
            ..Snapshot::default()
        });
        assert!(s.firmware.is_empty());
        assert!(s.model.is_empty());
        s.merge(Snapshot::default());
        assert!(s.serial.is_empty());
    }
    #[test]
    fn polls_are_exclusive_and_stale_results_cannot_unlock_usb() {
        let mut m = Monitor::default();
        let id = m.begin().unwrap();
        assert!(m.begin().is_none());
        assert!(!m.finish(id + 1, Ok(Snapshot::default())));
        assert_eq!(m.in_flight, Some(id));
        assert!(m.finish(
            id,
            Ok(Snapshot {
                mode: Mode::Adb,
                ..Snapshot::default()
            })
        ));
        assert!(m.reachable());
        m.checked = Some(Instant::now() - MAX_AGE);
        assert!(!m.reachable());
        let id = m.begin().unwrap();
        m.finish(id, Err("probe failed".into()));
        assert!(!m.reachable());
    }
    #[test]
    fn unauthenticated_modes_never_allow_start() {
        for mode in [
            Mode::None,
            Mode::Unauthorized,
            Mode::Sideload,
            Mode::ServerBlocking,
            Mode::Usb,
        ] {
            assert!(!mode.reachable());
        }
    }
}
