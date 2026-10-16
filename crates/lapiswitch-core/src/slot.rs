//! A/B slot of the device.

/// Active slot, as read from fastboot `current-slot` or inferred over EDL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    A,
    B,
}

impl Slot {
    /// Partition-name suffix (`_a` / `_b`).
    pub const fn suffix(self) -> &'static str {
        match self {
            Self::A => "_a",
            Self::B => "_b",
        }
    }

    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    /// Parse `a`, `b`, `_a` or `_b` (any case).
    pub fn parse(value: &str) -> Option<Self> {
        match value
            .trim()
            .trim_start_matches('_')
            .to_ascii_lowercase()
            .as_str()
        {
            "a" => Some(Self::A),
            "b" => Some(Self::B),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fastboot_and_suffix_forms() {
        assert_eq!(Slot::parse("a"), Some(Slot::A));
        assert_eq!(Slot::parse("_B"), Some(Slot::B));
        assert_eq!(Slot::parse(" b "), Some(Slot::B));
        assert_eq!(Slot::parse("c"), None);
        assert_eq!(Slot::A.other().suffix(), "_b");
    }
}
