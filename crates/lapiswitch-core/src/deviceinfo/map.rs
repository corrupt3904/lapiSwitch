//! DeviceInfo field maps (TB520FU / TB522FU structure maps) and typed,
//! exact-size value encoding. Every byte of the logical structure belongs to
//! exactly one field; the shared-buffer tail after it is never editable.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::SHARED_SIZE;
use crate::region::Region;

/// Field layouts of the supported models' DeviceInfo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    #[serde(rename = "TB520FU")]
    Tb520fu,
    #[serde(rename = "TB522FU")]
    Tb522fu,
}

/// How a field's bytes are shown and entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One byte, 0 or 1.
    Flag,
    /// Little-endian unsigned integer of the field's size (1, 2, 4 or 8).
    Uint,
    /// "PRC" or "ROW", NUL padded.
    Region,
    /// Printable ASCII, NUL padded. With `nul`, one terminating NUL is kept.
    Text { nul: bool },
    /// Raw bytes, entered as exactly `size` bytes of hex.
    Bytes,
}

/// Editing tier shown in the UI. Every tier is editable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Region, rollback indices and plain options.
    Basic,
    /// Version strings, boot-mode bytes and reserved areas.
    Advanced,
    /// Structure markers and security state: changes can prevent booting,
    /// change the lock/verity state or invalidate device-unique data.
    Protected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    pub offset: usize,
    pub size: usize,
    pub kind: Kind,
    pub class: Class,
    /// Korean meaning from the structure map.
    pub description: &'static str,
}

impl Field {
    pub fn range(&self) -> std::ops::Range<usize> {
        self.offset..self.offset + self.size
    }
}

const PERSISTENT_START: usize = 0x0998;
const PERSISTENT_SLOT: usize = 0x5E;
pub const ROLLBACK_START: usize = 0x0898;
pub const ROLLBACK_COUNT: usize = 32;

impl Layout {
    pub const fn model(self) -> &'static str {
        match self {
            Self::Tb520fu => "TB520FU",
            Self::Tb522fu => "TB522FU",
        }
    }
    pub const fn logical_size(self) -> usize {
        match self {
            Self::Tb520fu => 0x0EA8,
            Self::Tb522fu => 0x0D20,
        }
    }
    pub const fn region_offset(self) -> usize {
        match self {
            Self::Tb520fu => 0x0D10,
            Self::Tb522fu => 0x0CD4,
        }
    }

    /// The whole logical structure in offset order, without gaps.
    pub fn fields(self) -> Vec<Field> {
        use Class::*;
        use Kind::*;
        let mut fields = vec![];
        let mut add = |key: String, offset, size, kind, class, description| {
            fields.push(Field {
                key,
                offset,
                size,
                kind,
                class,
                description,
            })
        };
        add(
            "magic".into(),
            0x0000,
            0x0D,
            Text { nul: false },
            Protected,
            "DeviceInfo 매직",
        );
        add(
            "is_unlocked".into(),
            0x000D,
            1,
            Uint,
            Protected,
            "부트로더 잠금 해제 상태",
        );
        add(
            "is_unlock_critical".into(),
            0x000E,
            1,
            Uint,
            Protected,
            "critical 잠금 해제 상태",
        );
        add(
            "charger_screen_enabled".into(),
            0x000F,
            1,
            Flag,
            Basic,
            "충전 화면 사용",
        );
        add(
            "bootloader_version".into(),
            0x0010,
            0x40,
            Text { nul: true },
            Advanced,
            "부트로더 버전 문자열",
        );
        add(
            "radio_version".into(),
            0x0050,
            0x40,
            Text { nul: true },
            Advanced,
            "라디오/베이스밴드 버전 문자열",
        );
        add(
            "verity_mode".into(),
            0x0090,
            1,
            Uint,
            Protected,
            "verity 모드",
        );
        add(
            "reserved_0091".into(),
            0x0091,
            3,
            Bytes,
            Advanced,
            "정렬/예약",
        );
        add(
            "user_public_key_length".into(),
            0x0094,
            4,
            Uint,
            Protected,
            "사용자 공개키 길이",
        );
        add(
            "user_public_key".into(),
            0x0098,
            0x800,
            Bytes,
            Protected,
            "사용자 공개키",
        );
        for n in 0..ROLLBACK_COUNT {
            add(
                format!("rollback_index[{n}]"),
                ROLLBACK_START + n * 8,
                8,
                Uint,
                Basic,
                "롤백인덱스",
            );
        }
        for n in 0..8 {
            let base = PERSISTENT_START + n * PERSISTENT_SLOT;
            add(
                format!("persistent[{n}].valid"),
                base,
                2,
                Uint,
                Protected,
                "AVB persistent value 사용 여부",
            );
            add(
                format!("persistent[{n}].name_length"),
                base + 2,
                2,
                Uint,
                Protected,
                "AVB persistent value 이름 길이",
            );
            add(
                format!("persistent[{n}].value_length"),
                base + 4,
                2,
                Uint,
                Protected,
                "AVB persistent value 값 길이",
            );
            add(
                format!("persistent[{n}].name"),
                base + 6,
                0x38,
                Text { nul: false },
                Protected,
                "AVB persistent value 이름",
            );
            add(
                format!("persistent[{n}].value"),
                base + 0x3E,
                0x20,
                Bytes,
                Protected,
                "AVB persistent value 값",
            );
        }
        add(
            "golden_snapshot".into(),
            0x0C88,
            8,
            Uint,
            Advanced,
            "golden snapshot (추정)",
        );
        add(
            "audio_framework".into(),
            0x0C90,
            0x10,
            Bytes,
            Advanced,
            "audio framework 버퍼",
        );
        add(
            "fdr_frs_uds_flag".into(),
            0x0CA0,
            1,
            Uint,
            Protected,
            "FDR/FRS-UDS 상태",
        );
        add(
            "reserved_0CA1".into(),
            0x0CA1,
            3,
            Bytes,
            Advanced,
            "정렬/예약",
        );
        add(
            "frs_uds_cache_length".into(),
            0x0CA4,
            4,
            Uint,
            Protected,
            "FRS/UDS 캐시 길이",
        );
        add(
            "frs_uds_cache".into(),
            0x0CA8,
            0x20,
            Bytes,
            Protected,
            "FRS/UDS 캐시 (기기 고유)",
        );
        add(
            "region_magic_1".into(),
            0x0CC8,
            8,
            Text { nul: false },
            Protected,
            "리전 레코드 매직 1",
        );
        match self {
            Self::Tb520fu => {
                add(
                    "oem_reserved_1".into(),
                    0x0CD0,
                    0x40,
                    Bytes,
                    Advanced,
                    "OEM 예약 (미확인)",
                );
                add("region".into(), 0x0D10, 0x10, Region, Basic, "리전");
                add(
                    "region_magic_2".into(),
                    0x0D20,
                    8,
                    Text { nul: false },
                    Protected,
                    "리전 레코드 매직 2",
                );
                add(
                    "region_padding".into(),
                    0x0D28,
                    8,
                    Bytes,
                    Advanced,
                    "리전 후미 (미확인)",
                );
                add(
                    "oem_reserved_2".into(),
                    0x0D30,
                    0x170,
                    Bytes,
                    Advanced,
                    "OEM 예약 (미확인)",
                );
                add(
                    "fastboot_mode".into(),
                    0x0EA0,
                    1,
                    Uint,
                    Advanced,
                    "fastboot 부팅 모드 플래그",
                );
                add(
                    "recovery_mode".into(),
                    0x0EA1,
                    1,
                    Uint,
                    Advanced,
                    "recovery 부팅 모드 플래그",
                );
                add(
                    "factory_mode".into(),
                    0x0EA2,
                    1,
                    Uint,
                    Advanced,
                    "factory 부팅 모드 플래그",
                );
                add(
                    "reserved_end".into(),
                    0x0EA3,
                    5,
                    Bytes,
                    Advanced,
                    "후미 예약",
                );
            }
            Self::Tb522fu => {
                add(
                    "region_info_version".into(),
                    0x0CD0,
                    4,
                    Uint,
                    Protected,
                    "리전 레코드 버전",
                );
                add("region".into(), 0x0CD4, 0x10, Region, Basic, "리전");
                add(
                    "region_magic_2".into(),
                    0x0CE4,
                    8,
                    Text { nul: false },
                    Protected,
                    "리전 레코드 매직 2",
                );
                add(
                    "region_reserved".into(),
                    0x0CEC,
                    0x28,
                    Bytes,
                    Advanced,
                    "리전 레코드 후미 (미확인)",
                );
                add(
                    "fastboot_mode".into(),
                    0x0D14,
                    1,
                    Uint,
                    Advanced,
                    "fastboot 부팅 모드 플래그",
                );
                add(
                    "recovery_mode".into(),
                    0x0D15,
                    1,
                    Uint,
                    Advanced,
                    "recovery 부팅 모드 플래그",
                );
                add(
                    "factory_mode".into(),
                    0x0D16,
                    1,
                    Uint,
                    Advanced,
                    "factory 부팅 모드 플래그",
                );
                add(
                    "deviceinfo_format_version".into(),
                    0x0D17,
                    4,
                    Text { nul: true },
                    Protected,
                    "DeviceInfo 형식 버전",
                );
                add(
                    "reserved_end".into(),
                    0x0D1B,
                    5,
                    Bytes,
                    Advanced,
                    "후미 예약",
                );
            }
        }
        fields
    }

    pub fn field(self, key: &str) -> Result<Field, String> {
        self.fields()
            .into_iter()
            .find(|field| field.key == key)
            .ok_or_else(|| format!("알 수 없는 DeviceInfo 필드: {key}"))
    }

    /// The read DeviceInfo must be a complete buffer with the expected magic.
    pub fn check_buffer(self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() != SHARED_SIZE || &bytes[..13] != b"ANDROID-BOOT!" {
            return Err(format!(
                "{} DeviceInfo 길이·매직이 올바르지 않습니다.",
                self.model()
            ));
        }
        Ok(())
    }

    /// PRC/ROW when the region field holds one of them, NUL padded.
    pub fn region(self, bytes: &[u8]) -> Option<Region> {
        let field = &bytes.get(self.region_offset()..self.region_offset() + 0x10)?;
        decode_region(field)
    }

    pub fn rollback_indices(self, bytes: &[u8]) -> [u64; ROLLBACK_COUNT] {
        std::array::from_fn(|n| {
            let at = ROLLBACK_START + n * 8;
            u64::from_le_bytes(bytes[at..at + 8].try_into().expect("fixed size"))
        })
    }
}

fn decode_region(field: &[u8]) -> Option<Region> {
    let region = match &field[..3] {
        b"PRC" => Region::Prc,
        b"ROW" => Region::Row,
        _ => return None,
    };
    field[3..].iter().all(|b| *b == 0).then_some(region)
}

fn printable(text: &[u8]) -> bool {
    text.iter().all(|b| (0x20..=0x7E).contains(b))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

const HEX_PREFIX: &str = "hex:";

/// Canonical text of a field's current bytes; `encode` of it round-trips.
pub fn display(field: &Field, bytes: &[u8]) -> String {
    let value = &bytes[field.range()];
    match field.kind {
        Kind::Flag | Kind::Uint if field.size <= 2 => {
            let mut raw = [0u8; 8];
            raw[..field.size].copy_from_slice(value);
            let number = u64::from_le_bytes(raw);
            if field.kind == Kind::Flag && number > 1 {
                format!("{HEX_PREFIX}{}", hex(value))
            } else {
                number.to_string()
            }
        }
        Kind::Uint => {
            let mut raw = [0u8; 8];
            raw[..field.size].copy_from_slice(value);
            format!("0x{:X}", u64::from_le_bytes(raw))
        }
        Kind::Flag => unreachable!("flags are one byte"),
        Kind::Region => match decode_region(value) {
            Some(region) => region.token().into(),
            None => format!("{HEX_PREFIX}{}", hex(value)),
        },
        Kind::Text { nul } => {
            let end = value.iter().position(|b| *b == 0).unwrap_or(value.len());
            let (text, rest) = value.split_at(end);
            let fits = !nul || text.len() < value.len();
            if printable(text) && fits && rest.iter().all(|b| *b == 0) {
                String::from_utf8(text.to_vec()).expect("printable ASCII")
            } else {
                format!("{HEX_PREFIX}{}", hex(value))
            }
        }
        Kind::Bytes => hex(value),
    }
}

fn parse_hex(text: &str, size: usize) -> Result<Vec<u8>, String> {
    let digits: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() != size * 2 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "16진수 {size}바이트({}자리)를 정확히 입력하세요.",
            size * 2
        ));
    }
    Ok((0..size)
        .map(|i| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).expect("checked hex"))
        .collect())
}

fn parse_uint(text: &str, size: usize) -> Result<u64, String> {
    let max = if size == 8 {
        u64::MAX
    } else {
        (1u64 << (size * 8)) - 1
    };
    let value = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(digits) => u64::from_str_radix(digits, 16),
        None => text.parse::<u64>(),
    }
    .map_err(|_| format!("0 ~ {max} 범위의 정수(10진수 또는 0x16진수)를 입력하세요."))?;
    if value > max {
        return Err(format!(
            "{size}바이트 정수의 최대값({max}, 0x{max:X})을 넘습니다."
        ));
    }
    Ok(value)
}

/// Exactly `field.size` bytes for a user's input, or why it does not fit.
/// `hex:` followed by exactly `size` bytes is accepted for every field.
pub fn encode(field: &Field, input: &str) -> Result<Vec<u8>, String> {
    let text = input.trim();
    if let Some(raw) = text.strip_prefix(HEX_PREFIX) {
        return parse_hex(raw, field.size);
    }
    let mut out = vec![0u8; field.size];
    match field.kind {
        Kind::Flag => match text {
            "0" | "1" => out[0] = text.parse().expect("checked"),
            _ => return Err("0 또는 1을 입력하세요.".into()),
        },
        Kind::Uint => {
            let value = parse_uint(text, field.size)?;
            out.copy_from_slice(&value.to_le_bytes()[..field.size]);
        }
        Kind::Region => {
            let region = match text.to_ascii_uppercase().as_str() {
                "PRC" => Region::Prc,
                "ROW" => Region::Row,
                _ => return Err("PRC 또는 ROW를 입력하세요.".into()),
            };
            out[..3].copy_from_slice(region.token().as_bytes());
        }
        Kind::Text { nul } => {
            let limit = if nul { field.size - 1 } else { field.size };
            if !printable(text.as_bytes()) {
                return Err("영문·숫자·기호(ASCII)만 입력할 수 있습니다.".into());
            }
            if text.len() > limit {
                return Err(format!("최대 {limit}자까지 입력할 수 있습니다."));
            }
            out[..text.len()].copy_from_slice(text.as_bytes());
        }
        Kind::Bytes => return parse_hex(text, field.size),
    }
    Ok(out)
}

/// Field key → entered value. Only changed fields are stored, canonically.
pub type Edits = BTreeMap<String, String>;

/// Apply edits to the read buffer. Bytes outside the edited fields, and the
/// shared tail after the logical structure, stay exactly as read.
pub fn apply(layout: Layout, before: &[u8], edits: &Edits) -> Result<Vec<u8>, String> {
    layout.check_buffer(before)?;
    let mut planned = before.to_vec();
    for (key, value) in edits {
        let field = layout.field(key)?;
        let bytes = encode(&field, value).map_err(|error| format!("{key}: {error}"))?;
        planned[field.range()].copy_from_slice(&bytes);
    }
    check_plan(layout, before, &planned)?;
    Ok(planned)
}

/// The only structural rule for any plan: same length and an unchanged
/// shared-buffer tail. Field values are the user's choice.
pub fn check_plan(layout: Layout, before: &[u8], planned: &[u8]) -> Result<(), String> {
    layout.check_buffer(before)?;
    let logical = layout.logical_size();
    if planned.len() != SHARED_SIZE || planned[logical..] != before[logical..] {
        return Err(format!(
            "DeviceInfo 논리 크기(0x{logical:X}) 밖의 공유 버퍼는 바꿀 수 없습니다."
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub field: Field,
    pub before: String,
    pub after: String,
}

/// Changed fields in offset order.
pub fn diff(layout: Layout, before: &[u8], planned: &[u8]) -> Vec<Change> {
    layout
        .fields()
        .into_iter()
        .filter(|field| before[field.range()] != planned[field.range()])
        .map(|field| Change {
            before: display(&field, before),
            after: display(&field, planned),
            field,
        })
        .collect()
}

/// Canonical edits that reproduce `planned` from `before`.
pub fn edits_between(layout: Layout, before: &[u8], planned: &[u8]) -> Edits {
    diff(layout, before, planned)
        .into_iter()
        .map(|change| (change.field.key, change.after))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic buffer shaped like the structure maps (no real dump data).
    pub(crate) fn sample(layout: Layout) -> Vec<u8> {
        let mut b = vec![0u8; SHARED_SIZE];
        b[..13].copy_from_slice(b"ANDROID-BOOT!");
        b[0xCC8..0xCD0].copy_from_slice(b"deadbeef");
        let r = layout.region_offset();
        b[r..r + 3].copy_from_slice(b"PRC");
        b[r + 0x10..r + 0x18].copy_from_slice(b"beefdead");
        b[ROLLBACK_START + 8] = 1;
        b[ROLLBACK_START + 16..ROLLBACK_START + 20].copy_from_slice(&0x6A727D00u32.to_le_bytes());
        b[0xCA4] = 0x20;
        b[0xCA8..0xCC8].fill(0xA5);
        if layout == Layout::Tb522fu {
            b[0xCD0] = 1;
            b[0xD17..0xD1B].copy_from_slice(b"1.0\0");
        }
        b
    }

    #[test]
    fn both_maps_cover_the_logical_structure_exactly_once() {
        for layout in [Layout::Tb520fu, Layout::Tb522fu] {
            let mut next = 0;
            for field in layout.fields() {
                assert_eq!(field.offset, next, "{layout:?} {}", field.key);
                assert!(field.size > 0);
                next = field.range().end;
            }
            assert_eq!(next, layout.logical_size(), "{layout:?}");
            let fields = layout.fields();
            let keys: std::collections::BTreeSet<_> = fields.iter().map(|f| &f.key).collect();
            assert_eq!(keys.len(), fields.len(), "{layout:?} duplicate keys");
            // Offsets from the structure maps.
            let at = |key: &str| layout.field(key).unwrap().offset;
            assert_eq!(at("rollback_index[0]"), 0x898);
            assert_eq!(at("rollback_index[31]"), 0x990);
            assert_eq!(at("persistent[7].value"), 0x0C2A + 0x3E);
            assert_eq!(at("frs_uds_cache"), 0x0CA8);
            assert_eq!(at("region"), layout.region_offset());
        }
        let at = |layout: Layout, key: &str| layout.field(key).unwrap().offset;
        assert_eq!(at(Layout::Tb520fu, "factory_mode"), 0x0EA2);
        assert_eq!(at(Layout::Tb522fu, "factory_mode"), 0x0D16);
        assert_eq!(at(Layout::Tb522fu, "deviceinfo_format_version"), 0x0D17);
    }

    #[test]
    fn every_field_round_trips_its_canonical_display() {
        for layout in [Layout::Tb520fu, Layout::Tb522fu] {
            let b = sample(layout);
            for field in layout.fields() {
                let shown = display(&field, &b);
                assert_eq!(
                    encode(&field, &shown).unwrap(),
                    &b[field.range()],
                    "{}",
                    field.key
                );
            }
            assert!(apply(layout, &b, &Edits::new()).unwrap() == b);
        }
    }

    #[test]
    fn inputs_must_fit_the_field_type_and_size_exactly() {
        let layout = Layout::Tb520fu;
        let f = |key: &str| layout.field(key).unwrap();
        // Flags and integers.
        assert_eq!(encode(&f("charger_screen_enabled"), "1").unwrap(), [1]);
        assert!(encode(&f("charger_screen_enabled"), "2").is_err());
        assert_eq!(encode(&f("is_unlocked"), "255").unwrap(), [255]);
        assert!(encode(&f("is_unlocked"), "256").is_err());
        assert!(encode(&f("persistent[0].valid"), "65536").is_err());
        assert_eq!(
            encode(&f("rollback_index[2]"), "0x6A727D00").unwrap(),
            0x6A727D00u64.to_le_bytes()
        );
        assert_eq!(
            encode(&f("rollback_index[3]"), "18446744073709551615").unwrap(),
            [0xFF; 8]
        );
        assert!(encode(&f("rollback_index[3]"), "18446744073709551616").is_err());
        assert!(encode(&f("rollback_index[3]"), "-1").is_err());
        assert!(encode(&f("user_public_key_length"), "0x100000000").is_err());
        // Region.
        let region = encode(&f("region"), "row").unwrap();
        assert_eq!(&region[..3], b"ROW");
        assert!(region[3..].iter().all(|b| *b == 0));
        assert!(encode(&f("region"), "KOR").is_err());
        // Text keeps the terminating NUL only where the map requires it.
        assert!(encode(&f("bootloader_version"), &"a".repeat(63)).is_ok());
        assert!(encode(&f("bootloader_version"), &"a".repeat(64)).is_err());
        assert!(encode(&f("magic"), "ANDROID-BOOT!").is_ok());
        assert!(encode(&f("magic"), "ANDROID-BOOT!!").is_err());
        assert!(encode(&f("radio_version"), "한글").is_err());
        // Raw bytes need exactly the field size.
        assert_eq!(encode(&f("reserved_0091"), "01 02 03").unwrap(), [1, 2, 3]);
        assert!(encode(&f("reserved_0091"), "0102").is_err());
        assert!(encode(&f("reserved_0091"), "01020304").is_err());
        assert!(encode(&f("frs_uds_cache"), "zz").is_err());
        assert_eq!(encode(&f("verity_mode"), "hex:FF").unwrap(), [0xFF]);
        assert!(encode(&f("verity_mode"), "hex:FFFF").is_err());
    }

    #[test]
    fn apply_changes_only_edited_fields_and_never_the_shared_tail() {
        for layout in [Layout::Tb520fu, Layout::Tb522fu] {
            let mut before = sample(layout);
            before[layout.logical_size()..].fill(0x33);
            let edits = Edits::from([
                ("region".into(), "ROW".into()),
                ("rollback_index[2]".into(), "0".into()),
                ("is_unlocked".into(), "1".into()),
                ("frs_uds_cache".into(), "00".repeat(32)),
            ]);
            let planned = apply(layout, &before, &edits).unwrap();
            let changes = diff(layout, &before, &planned);
            let keys: Vec<_> = changes.iter().map(|c| c.field.key.as_str()).collect();
            assert_eq!(
                keys,
                [
                    "is_unlocked",
                    "rollback_index[2]",
                    "frs_uds_cache",
                    "region"
                ]
            );
            for (i, (a, b)) in before.iter().zip(&planned).enumerate() {
                if a != b {
                    assert!(
                        changes.iter().any(|c| c.field.range().contains(&i)),
                        "{i:X}"
                    );
                }
            }
            assert_eq!(
                planned[layout.logical_size()..],
                before[layout.logical_size()..]
            );
            assert_eq!(layout.region(&planned), Some(Region::Row));
            // Canonical edits rebuild exactly the same plan.
            let canonical = edits_between(layout, &before, &planned);
            assert_eq!(apply(layout, &before, &canonical).unwrap(), planned);
            let mut tail = planned.clone();
            tail[SHARED_SIZE - 1] ^= 1;
            assert!(check_plan(layout, &before, &tail).is_err());
        }
        let b = sample(Layout::Tb520fu);
        let unknown = Edits::from([("nonexistent".into(), "1".into())]);
        assert!(apply(Layout::Tb520fu, &b, &unknown).is_err());
        let mut no_magic = b.clone();
        no_magic[0] = b'X';
        assert!(apply(Layout::Tb520fu, &no_magic, &Edits::new()).is_err());
    }

    #[test]
    fn layouts_place_region_and_tail_fields_differently() {
        let b520 = sample(Layout::Tb520fu);
        let b522 = sample(Layout::Tb522fu);
        assert_eq!(Layout::Tb520fu.region(&b520), Some(Region::Prc));
        assert_eq!(Layout::Tb522fu.region(&b522), Some(Region::Prc));
        assert_eq!(Layout::Tb520fu.region(&b522), None);
        let f = Layout::Tb522fu.field("deviceinfo_format_version").unwrap();
        assert_eq!(display(&f, &b522), "1.0");
        assert_eq!(
            display(
                &Layout::Tb522fu.field("region_info_version").unwrap(),
                &b522
            ),
            "0x1"
        );
    }

    /// Real dumps stay outside the repository; set the env vars to check them.
    #[test]
    #[ignore = "Set LAPISWITCH_DEVICEINFO_TB520FU / LAPISWITCH_DEVICEINFO_TB522FU to local dumps."]
    fn real_dumps_match_the_structure_maps() {
        for (layout, var) in [
            (Layout::Tb520fu, "LAPISWITCH_DEVICEINFO_TB520FU"),
            (Layout::Tb522fu, "LAPISWITCH_DEVICEINFO_TB522FU"),
        ] {
            let Some(path) = std::env::var_os(var) else {
                continue;
            };
            let bytes = std::fs::read(path).unwrap();
            layout.check_buffer(&bytes).unwrap();
            assert!(layout.region(&bytes).is_some(), "{layout:?}");
            let magic = |key: &str| display(&layout.field(key).unwrap(), &bytes);
            assert_eq!(magic("region_magic_1"), "deadbeef");
            assert_eq!(magic("region_magic_2"), "beefdead");
            for field in layout.fields() {
                let shown = display(&field, &bytes);
                assert_eq!(encode(&field, &shown).unwrap(), &bytes[field.range()]);
            }
            assert!(bytes[layout.logical_size()..].iter().all(|b| *b == 0));
        }
    }
}
