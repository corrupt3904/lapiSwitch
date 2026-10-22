//! DeviceInfo field maps (from `devices/<MODEL>/deviceinfo.toml`) and typed,
//! exact-size value encoding. Every byte of the logical structure belongs to
//! exactly one field; the shared-buffer tail after it is never editable.
use std::collections::BTreeMap;

use super::SHARED_SIZE;
use crate::region::Region;

/// A device's DeviceInfo field layout, from `devices/<MODEL>/deviceinfo.toml`.
#[derive(Clone, Copy)]
pub struct Layout(&'static lapiswitch_devices::Device);

impl PartialEq for Layout {
    fn eq(&self, other: &Self) -> bool {
        self.model() == other.model()
    }
}
impl Eq for Layout {}
impl std::fmt::Debug for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.model())
    }
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

/// Editing tier of a field. Every tier is editable.
pub use lapiswitch_devices::Class;

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

/// Every device file has exactly 32 contiguous 8-byte rollback indices.
pub const ROLLBACK_COUNT: usize = 32;

impl Layout {
    pub fn of(device: &'static lapiswitch_devices::Device) -> Self {
        Self(device)
    }
    /// The layout of a device in `devices/`, by model name.
    pub fn find(model: &str) -> Option<Self> {
        lapiswitch_devices::find(model).map(Self)
    }
    pub fn model(self) -> &'static str {
        self.0.model
    }
    pub fn logical_size(self) -> usize {
        self.0.deviceinfo.logical_size
    }
    fn offset(self, key: &str) -> usize {
        self.0
            .deviceinfo
            .fields
            .iter()
            .find(|field| field.key == key)
            .map(|field| field.offset)
            .expect("required field, checked at build time")
    }
    pub fn region_offset(self) -> usize {
        self.offset("region")
    }
    pub fn rollback_start(self) -> usize {
        self.offset("rollback_index[0]")
    }

    /// The whole logical structure in offset order, without gaps.
    pub fn fields(self) -> Vec<Field> {
        use lapiswitch_devices::Kind as K;
        self.0
            .deviceinfo
            .fields
            .iter()
            .map(|field| Field {
                key: field.key.to_owned(),
                offset: field.offset,
                size: field.size,
                kind: match field.kind {
                    K::Flag => Kind::Flag,
                    K::Uint => Kind::Uint,
                    K::Region => Kind::Region,
                    K::Text => Kind::Text { nul: false },
                    K::TextNul => Kind::Text { nul: true },
                    K::Bytes => Kind::Bytes,
                },
                class: field.class,
                description: field.description,
            })
            .collect()
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
        let start = self.rollback_start();
        std::array::from_fn(|n| {
            let at = start + n * 8;
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

    fn tb520() -> Layout {
        Layout::find("TB520FU").unwrap()
    }
    fn tb522() -> Layout {
        Layout::find("TB522FU").unwrap()
    }

    /// Synthetic buffer shaped like the structure maps (no real dump data).
    pub(crate) fn sample(layout: Layout) -> Vec<u8> {
        let mut b = vec![0u8; SHARED_SIZE];
        b[..13].copy_from_slice(b"ANDROID-BOOT!");
        b[0xCC8..0xCD0].copy_from_slice(b"deadbeef");
        let r = layout.region_offset();
        b[r..r + 3].copy_from_slice(b"PRC");
        b[r + 0x10..r + 0x18].copy_from_slice(b"beefdead");
        let rollback = layout.rollback_start();
        b[rollback + 8] = 1;
        b[rollback + 16..rollback + 20].copy_from_slice(&0x6A727D00u32.to_le_bytes());
        b[0xCA4] = 0x20;
        b[0xCA8..0xCC8].fill(0xA5);
        if layout == tb522() {
            b[0xCD0] = 1;
            b[0xD17..0xD1B].copy_from_slice(b"1.0\0");
        }
        b
    }

    #[test]
    fn both_maps_cover_the_logical_structure_exactly_once() {
        for layout in [tb520(), tb522()] {
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
        assert_eq!(at(tb520(), "factory_mode"), 0x0EA2);
        assert_eq!(at(tb522(), "factory_mode"), 0x0D16);
        assert_eq!(at(tb522(), "deviceinfo_format_version"), 0x0D17);
    }

    #[test]
    fn every_field_round_trips_its_canonical_display() {
        for layout in [tb520(), tb522()] {
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
        let layout = tb520();
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
        for layout in [tb520(), tb522()] {
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
        let b = sample(tb520());
        let unknown = Edits::from([("nonexistent".into(), "1".into())]);
        assert!(apply(tb520(), &b, &unknown).is_err());
        let mut no_magic = b.clone();
        no_magic[0] = b'X';
        assert!(apply(tb520(), &no_magic, &Edits::new()).is_err());
    }

    #[test]
    fn layouts_place_region_and_tail_fields_differently() {
        let b520 = sample(tb520());
        let b522 = sample(tb522());
        assert_eq!(tb520().region(&b520), Some(Region::Prc));
        assert_eq!(tb522().region(&b522), Some(Region::Prc));
        assert_eq!(tb520().region(&b522), None);
        let f = tb522().field("deviceinfo_format_version").unwrap();
        assert_eq!(display(&f, &b522), "1.0");
        assert_eq!(
            display(&tb522().field("region_info_version").unwrap(), &b522),
            "0x1"
        );
    }

    /// Real dumps stay outside the repository; set the env vars to check them.
    #[test]
    #[ignore = "Set LAPISWITCH_DEVICEINFO_<MODEL> (e.g. LAPISWITCH_DEVICEINFO_TB520FU) to local dumps."]
    fn real_dumps_match_the_structure_maps() {
        for device in lapiswitch_devices::DEVICES {
            let layout = Layout::of(device);
            let var = format!("LAPISWITCH_DEVICEINFO_{}", layout.model());
            let Some(path) = std::env::var_os(&var) else {
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
