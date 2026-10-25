//! DeviceInfo field maps (from `devices/<MODEL>/deviceinfo.toml`) and typed,
//! exact-size value encoding. Every byte of the logical structure belongs to
//! exactly one field; the shared-buffer tail after it is never editable.
use std::collections::BTreeMap;

use super::SHARED_SIZE;
use crate::region::Region;

/// A device's DeviceInfo field layout, from `devices/<MODEL>/deviceinfo.toml`.
#[derive(Clone, Copy)]
pub struct Layout {
    model: &'static str,
    map: &'static lapiswitch_devices::DeviceInfoMap,
}

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
    /// "PRC" or "ROW", 3 bytes. No `hex:` input.
    Region,
    /// Printable ASCII, NUL padded. With `nul`, one terminating NUL is kept.
    Text { nul: bool },
    /// Raw bytes, entered as exactly `size` bytes of hex.
    Bytes,
}

/// Editing tier of a field: how much care a change needs.
pub use lapiswitch_devices::Class;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    pub offset: usize,
    pub size: usize,
    pub kind: Kind,
    pub class: Class,
    /// English meaning from the structure map; the app translates it.
    pub description: &'static str,
    /// `false`: a plan never changes these bytes.
    pub editable: bool,
    /// The only values a plan may write, as `display` shows them; empty
    /// accepts any value of the kind.
    pub choices: &'static [&'static str],
    /// A `uint` shown and entered in decimal only.
    pub decimal: bool,
    /// Most decimal digits a new value may have; 0 for no limit.
    pub max_digits: usize,
}

impl Field {
    pub fn range(&self) -> std::ops::Range<usize> {
        self.offset..self.offset + self.size
    }
}

/// Every device file has exactly 32 contiguous 8-byte rollback indices.
pub const ROLLBACK_COUNT: usize = 32;

impl Layout {
    /// `None` for a device without `deviceinfo.toml`.
    pub fn of(device: &'static lapiswitch_devices::Device) -> Option<Self> {
        device.deviceinfo.as_ref().map(|map| Self {
            model: device.model,
            map,
        })
    }
    /// The layout of a device in `devices/`, by model name.
    pub fn find(model: &str) -> Option<Self> {
        lapiswitch_devices::find(model).and_then(Self::of)
    }
    pub fn model(self) -> &'static str {
        self.model
    }
    pub fn logical_size(self) -> usize {
        self.map.logical_size
    }
    fn offset(self, key: &str) -> usize {
        self.map
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
        self.map
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
                editable: field.editable,
                choices: field.choices,
                decimal: field.decimal,
                max_digits: field.max_digits,
            })
            .collect()
    }

    pub fn field(self, key: &str) -> Result<Field, String> {
        self.fields()
            .into_iter()
            .find(|field| field.key == key)
            .ok_or_else(|| crate::msg!("core_map_unknown_field", key = key))
    }

    /// The read DeviceInfo must be a complete buffer with the expected magic.
    pub fn check_buffer(self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() != SHARED_SIZE || &bytes[..13] != b"ANDROID-BOOT!" {
            return Err(crate::msg!("core_map_buffer_invalid", model = self.model()));
        }
        Ok(())
    }

    /// PRC/ROW when the 3-byte region field holds one of them and the 13
    /// bytes of `region_fill` after it are all zero.
    pub fn region(self, bytes: &[u8]) -> Option<Region> {
        let start = self.region_offset();
        let fill = self.offset("region_fill");
        let region = decode_region(bytes.get(start..start + 3)?)?;
        bytes
            .get(fill..fill + 13)?
            .iter()
            .all(|b| *b == 0)
            .then_some(region)
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
        Kind::Flag | Kind::Uint if field.size <= 2 || field.decimal => {
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
        return Err(crate::msg!(
            "core_map_hex_exact",
            size = size,
            size2 = size * 2
        ));
    }
    Ok((0..size)
        .map(|i| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).expect("checked hex"))
        .collect())
}

fn parse_uint(text: &str, size: usize, decimal: bool) -> Result<u64, String> {
    let max = uint_max(size);
    let value = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(_) if decimal => return Err(crate::msg!("core_map_uint_decimal", max = max)),
        Some(digits) => u64::from_str_radix(digits, 16),
        None => text.parse::<u64>(),
    }
    .map_err(|_| {
        if decimal {
            crate::msg!("core_map_uint_decimal", max = max)
        } else {
            crate::msg!("core_map_uint_range", max = max)
        }
    })?;
    if value > max {
        return Err(crate::msg!(
            "core_map_uint_too_large",
            size = size,
            max = max
        ));
    }
    Ok(value)
}

/// Exactly `field.size` bytes for a user's input, or why it does not fit.
/// `hex:` (any case) followed by exactly `size` bytes is accepted for every
/// field except the region and decimal integers.
pub fn encode(field: &Field, input: &str) -> Result<Vec<u8>, String> {
    let text = input.trim();
    if let Some(raw) = text
        .get(..HEX_PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(HEX_PREFIX))
        .map(|_| &text[HEX_PREFIX.len()..])
    {
        return match field.kind {
            Kind::Region => Err(crate::msg!("core_map_region")),
            Kind::Uint if field.decimal => Err(crate::msg!(
                "core_map_uint_decimal",
                max = uint_max(field.size)
            )),
            _ => parse_hex(raw, field.size),
        };
    }
    let mut out = vec![0u8; field.size];
    match field.kind {
        Kind::Flag => match text {
            "0" | "1" => out[0] = text.parse().expect("checked"),
            _ => return Err(crate::msg!("core_map_flag")),
        },
        Kind::Uint => {
            if field.max_digits > 0 && text.len() > field.max_digits {
                return Err(crate::msg!(
                    "core_map_max_digits",
                    digits = field.max_digits
                ));
            }
            let value = parse_uint(text, field.size, field.decimal)?;
            out.copy_from_slice(&value.to_le_bytes()[..field.size]);
        }
        Kind::Region => {
            let region = match text.to_ascii_uppercase().as_str() {
                "PRC" => Region::Prc,
                "ROW" => Region::Row,
                _ => return Err(crate::msg!("core_map_region")),
            };
            out[..3].copy_from_slice(region.token().as_bytes());
        }
        Kind::Text { nul } => {
            let limit = if nul { field.size - 1 } else { field.size };
            if !printable(text.as_bytes()) {
                return Err(crate::msg!("core_map_ascii"));
            }
            if text.len() > limit {
                return Err(crate::msg!("core_map_text_limit", limit = limit));
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

/// The structural rules for any plan: same length, an unchanged
/// shared-buffer tail, read-only fields kept and choice fields changed only
/// to one of their choices. Other values are the user's choice.
pub fn check_plan(layout: Layout, before: &[u8], planned: &[u8]) -> Result<(), String> {
    layout.check_buffer(before)?;
    let logical = layout.logical_size();
    if planned.len() != SHARED_SIZE || planned[logical..] != before[logical..] {
        return Err(crate::msg!(
            "core_map_outside_logical",
            logical = format!("{:X}", logical)
        ));
    }
    for field in layout.fields() {
        if before[field.range()] == planned[field.range()] {
            continue;
        }
        if !field.editable {
            return Err(crate::msg!("core_map_read_only", key = field.key));
        }
        if !field.choices.is_empty() && !field.choices.contains(&display(&field, planned).as_str())
        {
            return Err(crate::msg!(
                "core_map_choice",
                key = field.key,
                choices = field.choices.join(", ")
            ));
        }
    }
    Ok(())
}

fn uint_max(size: usize) -> u64 {
    if size == 8 {
        u64::MAX
    } else {
        (1u64 << (size * 8)) - 1
    }
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
pub(crate) mod tests {
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
            encode(&f("rollback_index[2]"), "1785888000").unwrap(),
            0x6A727D00u64.to_le_bytes()
        );
        assert_eq!(
            encode(&f("user_public_key_length"), "4294967295").unwrap(),
            [0xFF; 4]
        );
        assert!(encode(&f("rollback_index[3]"), "18446744073709551615").is_err());
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
    fn read_only_fields_choices_and_decimal_rollbacks_are_enforced() {
        for layout in [tb520(), tb522()] {
            let before = sample(layout);
            let f = |key: &str| layout.field(key).unwrap();
            // Rollback indices are decimal only; the region takes no hex.
            assert_eq!(display(&f("rollback_index[1]"), &before), "1");
            assert!(encode(&f("rollback_index[1]"), "0x10").is_err());
            assert!(encode(&f("rollback_index[1]"), &format!("hex:{}", "00".repeat(8))).is_err());
            assert_eq!(
                encode(&f("rollback_index[1]"), "16").unwrap(),
                16u64.to_le_bytes()
            );
            // At most 10 digits.
            assert!(encode(&f("rollback_index[1]"), "9999999999").is_ok());
            assert!(encode(&f("rollback_index[1]"), "10000000000").is_err());
            assert!(encode(&f("region"), "hex:524F57").is_err());
            assert_eq!(encode(&f("region"), "row").unwrap(), b"ROW");
            let changed = |key: &str, byte: u8| {
                let mut planned = before.clone();
                planned[f(key).offset] = byte;
                check_plan(layout, &before, &planned)
            };
            for key in ["magic", "region_magic_1", "region_magic_2", "region_fill"] {
                assert!(changed(key, b'X').is_err(), "{layout:?} {key}");
            }
            assert!(changed("verity_mode", 3).is_ok());
            assert!(changed("verity_mode", 4).is_err());
            assert!(changed("fastboot_mode", 1).is_ok());
            assert!(changed("fastboot_mode", 2).is_err());
            assert!(changed("persistent[0].valid", 2).is_err());
            // A non-zero byte after the region token is not a region.
            let mut odd = before.clone();
            odd[f("region_fill").offset + 5] = 1;
            assert_eq!(layout.region(&odd), None);
        }
        for key in ["region_info_version", "deviceinfo_format_version"] {
            assert!(!tb522().field(key).unwrap().editable, "{key}");
        }
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
            let Some(layout) = Layout::of(device) else {
                continue;
            };
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
