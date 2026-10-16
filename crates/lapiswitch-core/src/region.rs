//! Supported target regions for the TWRP/DeviceInfo workflow.

/// Region the user converts the tablet to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Region {
    Prc,
    Row,
}
impl Region {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Prc => "PRC",
            Self::Row => "ROW",
        }
    }
}
