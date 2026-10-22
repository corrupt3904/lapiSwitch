//! The device the workflow runs for: the folder in `devices/` with
//! `flow = true`. Model names, firmware rules and the pinned TWRP come from
//! its `device.toml`; nothing here is specific to one model.
use crate::region::Region;
use lapiswitch_devices::{Device, Flow, RegionCode};

pub fn current() -> &'static Device {
    lapiswitch_devices::flow_device()
}

pub fn model() -> &'static str {
    current().model
}

/// TWRP's `ro.product.device` and an ADB/EDL model name may be the codename.
pub fn codename() -> &'static str {
    current()
        .codename
        .expect("the build requires a codename for the flow device")
}

pub fn flow() -> &'static Flow {
    current().flow()
}

pub fn region(code: RegionCode) -> Region {
    match code {
        RegionCode::Prc => Region::Prc,
        RegionCode::Row => Region::Row,
    }
}
