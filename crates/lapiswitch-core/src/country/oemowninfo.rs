//! A country code in a fixed field of a dedicated partition (`[country]
//! method = "oemowninfo"` in device.toml), e.g. TB522FU's LinuxLoader field at
//! +0xE4 of `oemowninfo`. Only the four ASCII country bytes may change; never
//! search or replace anywhere else.
use crate::storage::Partition;
pub use lapiswitch_devices::OemOwnInfo;

pub fn validate_partition(field: &OemOwnInfo, part: &Partition) -> Result<(), String> {
    if part.name != field.partition
        || part.lun != field.lun
        || part.size_bytes()? != field.size as u64
    {
        return Err(crate::msg!(
            "core_oemowninfo_unsupported_layout",
            partition = field.partition
        ));
    }
    Ok(())
}

pub fn detect<'a>(field: &OemOwnInfo, data: &'a [u8]) -> Result<&'a str, String> {
    if data.len() != field.size {
        return Err(crate::msg!(
            "core_oemowninfo_length_mismatch",
            partition = field.partition,
            size = field.size
        ));
    }
    let bytes = &data[field.offset..field.offset + field.field_size];
    let code =
        std::str::from_utf8(&bytes[..2]).map_err(|_| crate::msg!("core_oemowninfo_code_format"))?;
    if !super::KNOWN_CODES.contains(&code)
        || !matches!(&bytes[2..4], b"XX" | b"XE")
        || bytes[4..].iter().any(|b| *b != 0)
    {
        return Err(crate::msg!(
            "core_oemowninfo_field_unexpected",
            partition = field.partition,
            offset = format!("{:X}", field.offset),
            field_size = field.field_size
        ));
    }
    Ok(code)
}

pub fn plan(field: &OemOwnInfo, before: &[u8], code: &str) -> Result<Vec<u8>, String> {
    detect(field, before)?;
    let code = super::validate(code)?;
    let suffix = if super::EU_CODES.contains(&code.as_str()) {
        "XE"
    } else {
        "XX"
    };
    let mut planned = before.to_vec();
    planned[field.offset..field.offset + 4].copy_from_slice(&super::pattern(&code, suffix));
    verify_plan(field, before, &planned)?;
    Ok(planned)
}

pub fn verify_plan(field: &OemOwnInfo, before: &[u8], planned: &[u8]) -> Result<(), String> {
    detect(field, before)?;
    detect(field, planned)?;
    let at = field.offset;
    if before[..at] != planned[..at] || before[at + 4..] != planned[at + 4..] {
        return Err(crate::msg!(
            "core_oemowninfo_outside_changed",
            partition = field.partition
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field() -> OemOwnInfo {
        match crate::device::tb522fu().country() {
            lapiswitch_devices::Country::Oemowninfo(field) => field,
            other => panic!("{other:?}"),
        }
    }
    fn image() -> Vec<u8> {
        let f = field();
        let mut b = vec![0xA7; f.size];
        b[f.offset..f.offset + f.field_size].fill(0);
        b[f.offset..f.offset + 4].copy_from_slice(b"CNXX");
        // Deliberately put another matching string outside the field.
        b[0x900..0x904].copy_from_slice(b"CNXX");
        b
    }
    #[test]
    fn fixed_field_preserves_every_other_byte_including_decoys_and_padding() {
        let f = field();
        let before = image();
        for (code, expected) in [("KR", b"KRXX"), ("DE", b"DEXE"), ("CN", b"CNXX")] {
            let after = plan(&f, &before, code).unwrap();
            assert_eq!(&after[f.offset..f.offset + 4], expected);
            assert_eq!(&after[0x900..0x904], b"CNXX");
            assert_eq!(&after[..f.offset], &before[..f.offset]);
            assert_eq!(&after[f.offset + 4..], &before[f.offset + 4..]);
        }
    }
    #[test]
    fn malformed_or_unrecognized_images_and_unrelated_changes_are_rejected() {
        let f = field();
        assert!(plan(&f, &[], "KR").is_err());
        assert!(plan(&f, &image()[..f.size - 1], "KR").is_err());
        assert!(plan(&f, &image(), "ZZ").is_err());
        for at in [
            f.offset,
            f.offset + 2,
            f.offset + 4,
            f.offset + f.field_size - 1,
        ] {
            let mut bad = image();
            bad[at] = 0xFF;
            assert!(plan(&f, &bad, "KR").is_err());
        }
        let before = image();
        let mut bad = plan(&f, &before, "KR").unwrap();
        bad[0x104] ^= 1;
        assert!(verify_plan(&f, &before, &bad).is_err());
    }
}
