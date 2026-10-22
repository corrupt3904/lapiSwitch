//! TB520FU country patching ported from LTBox ltbox-patch/region.rs.
//! devinfo uses the original raw-match rule; persist only edits actual fields
//! and leaves captured log text unchanged. No proinfo or vendor_boot changes.

use sha2::{Digest, Sha256};

pub mod oemowninfo;

pub const KNOWN_CODES: &[&str] = &[
    "CN", "KR", "JP", "US", "GB", "DE", "FR", "IT", "ES", "NL", "AT", "BE", "BG", "HR", "CY", "CZ",
    "DK", "EE", "FI", "GR", "HU", "IE", "LV", "LT", "LU", "MT", "PL", "PT", "RO", "SK", "SI", "SE",
    "AU", "CA", "IN", "RU", "BR", "MX", "SA", "AE", "WW",
];
pub const EU_CODES: &[&str] = &[
    "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE", "IT", "LV",
    "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
];

pub fn validate(code: &str) -> Result<String, String> {
    let code = code.to_ascii_uppercase();
    if KNOWN_CODES.contains(&code.as_str()) {
        Ok(code)
    } else {
        Err(format!("지원하지 않는 국가 코드: {code}"))
    }
}

fn field(data: &[u8], i: usize) -> bool {
    (i.is_multiple_of(4096) || data[i - 1] == 0) && (i + 4 == data.len() || data[i + 4] == 0)
}

pub(crate) fn pattern(code: &str, suffix: &str) -> [u8; 4] {
    [
        code.as_bytes()[0],
        code.as_bytes()[1],
        suffix.as_bytes()[0],
        suffix.as_bytes()[1],
    ]
}

pub fn detect(data: &[u8], field_only: bool) -> Option<&'static str> {
    for code in KNOWN_CODES {
        for suffix in ["XE", "XX"] {
            let wanted = pattern(code, suffix);
            if data
                .windows(4)
                .enumerate()
                .any(|(i, bytes)| bytes == wanted && (!field_only || field(data, i)))
            {
                return Some(code);
            }
        }
    }
    None
}

/// Same-code matches still count as handled, as in LTBox. The suffix for EU
/// targets is XE and for others XX, accepting either suffix on the old code.
pub fn patch(data: &[u8], old: &str, new: &str, field_only: bool) -> Result<Vec<u8>, String> {
    let old = validate(old)?;
    let new = validate(new)?;
    let target = pattern(
        &new,
        if EU_CODES.contains(&new.as_str()) {
            "XE"
        } else {
            "XX"
        },
    );
    let mut out = data.to_vec();
    let mut i = 0;
    while i + 4 <= out.len() {
        let matches = out[i..i + 4] == pattern(&old, "XE") || out[i..i + 4] == pattern(&old, "XX");
        if matches && (!field_only || field(&out, i)) {
            out[i..i + 4].copy_from_slice(&target);
            i += 4;
        } else {
            i += 1;
        }
    }
    Ok(out)
}

/// A post-Region persist fingerprint stable across our own country edits.
/// Only known, real country fields are normalized; logs and all other bytes
/// remain part of the identity. This is never used to identify a model.
pub fn persist_fingerprint(data: &[u8]) -> String {
    let mut normalized = data.to_vec();
    for i in 0..data.len().saturating_sub(3) {
        let bytes = &data[i..i + 4];
        if (&bytes[2..] != b"XE" && &bytes[2..] != b"XX") || !field(data, i) {
            continue;
        }
        if KNOWN_CODES
            .iter()
            .any(|code| bytes == pattern(code, "XE") || bytes == pattern(code, "XX"))
        {
            normalized[i..i + 4].copy_from_slice(b"ZZXX");
        }
    }
    Sha256::digest(&normalized)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persist_edits_real_fields_and_preserves_log_decoys() {
        let data = b"xx Update country code CNXX\0junk\0CNXX\0\0";
        assert_eq!(detect(data, true), Some("CN"));
        let patched = patch(data, "CN", "KR", true).unwrap();
        assert!(
            patched
                .windows(b"Update country code CNXX".len())
                .any(|part| part == b"Update country code CNXX")
        );
        assert!(patched.windows(6).any(|part| part == b"\0KRXX\0"));
        assert_eq!(persist_fingerprint(data), persist_fingerprint(&patched));
    }
    #[test]
    fn log_with_zero_slack_is_not_a_country_field() {
        let data = b"xx Update country code CNXX\0\0";
        assert_eq!(detect(data, true), None);
        assert_eq!(patch(data, "CN", "KR", true).unwrap(), data);
        assert_eq!(detect(data, false), Some("CN"));
    }
    #[test]
    fn block_aligned_country_field_may_have_binary_prefix() {
        let mut data = vec![0xab; 4096];
        data.extend_from_slice(b"CNXX\0\0");
        assert_eq!(detect(&data, true), Some("CN"));
        assert_eq!(
            &patch(&data, "CN", "KR", true).unwrap()[4096..4100],
            b"KRXX"
        );
    }
    #[test]
    fn eu_suffix_is_written_from_either_stock_suffix() {
        for input in [b"\0CNXX\0", b"\0CNXE\0"] {
            assert_eq!(patch(input, "CN", "DE", false).unwrap(), b"\0DEXE\0");
        }
        assert_eq!(patch(b"FRXX", "FR", "US", false).unwrap(), b"USXX");
    }
    #[test]
    fn fingerprint_keeps_non_country_bytes_and_log_text() {
        let a = b"\0CNXX\0device-a";
        let b = b"\0KRXX\0device-a";
        let c = b"\0KRXX\0device-b";
        assert_eq!(persist_fingerprint(a), persist_fingerprint(b));
        assert_ne!(persist_fingerprint(a), persist_fingerprint(c));
        assert_ne!(
            persist_fingerprint(b"log CNXX\0"),
            persist_fingerprint(b"log KRXX\0")
        );
    }
    #[test]
    fn invalid_codes_are_rejected_and_short_images_are_safe() {
        for code in ["K", "KOR", "ZZ", "../"] {
            assert!(validate(code).is_err());
        }
        assert_eq!(validate("kr").unwrap(), "KR");
        for data in [b"".as_slice(), b"C", b"CNX"] {
            assert_eq!(detect(data, true), None);
            persist_fingerprint(data);
        }
    }
}
