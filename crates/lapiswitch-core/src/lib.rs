//! `lapiswitch-core` — the TB520FU PRC ↔ ROW region-change sequence, shared by
//! the GUI and the development CLI.

pub mod avb_info;
pub mod backup;
pub mod country;
pub mod device;
pub mod device_version;
pub mod firmware;
pub mod gpt;
pub mod i18n;
pub mod prepare;
pub mod region;
pub mod rollback;
pub mod slot;
pub mod stage1;
pub mod state;
pub mod storage;
pub mod workflow;

pub mod deviceinfo;
pub mod recovery_asset;
pub mod recovery_flow;

pub mod vendor_boot;
