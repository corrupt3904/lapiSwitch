//! Primary GPT of UFS LUN 4 (`gpt_main4.bin`): validate and remove `vbmeta_a`
//! (required), `vbmeta` and the active slot's `pvmfw_X` (if present).
//!
//! All retained raw entries, including the other slot's pvmfw, bare pvmfw
//! and vbmeta_b, are preserved. A separate Rust reference converter lives
//! in tools/xtask for byte-for-byte fixture verification.

use crate::slot::Slot;

/// Missing fields in pre-policy manifests/states identify the old algorithm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GptPolicy {
    #[default]
    LegacyActiveSlot,
    FixedVbmetaA,
    FixedVbmetaAPvmfwA,
    FixedVbmetaAActivePvmfw,
}
pub const CURRENT_POLICY: GptPolicy = GptPolicy::FixedVbmetaAActivePvmfw;

/// UFS logical sector size on the TB520FU.
pub const SECTOR_SIZE: usize = 4096;
/// The GPT header sits in LBA 1.
const HEADER_OFFSET: usize = SECTOR_SIZE;
const SIGNATURE: &[u8; 8] = b"EFI PART";
const MIN_HEADER_SIZE: usize = 92;
const MIN_ENTRY_SIZE: usize = 128;
/// UTF-16LE partition name field inside an entry.
const NAME_RANGE: std::ops::Range<usize> = 56..128;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GptError {
    #[error("GPT blob is {len} bytes; the header needs at least {needed}")]
    TooShortForHeader { len: usize, needed: usize },
    #[error("EFI PART signature not found at offset {HEADER_OFFSET:#x}")]
    BadSignature,
    #[error("unexpected GPT header size {0}")]
    BadHeaderSize(u32),
    #[error("unexpected GPT entry size {0}")]
    BadEntrySize(u32),
    #[error("GPT header CRC {stored:#010x} does not match computed {computed:#010x}")]
    HeaderCrc { stored: u32, computed: u32 },
    #[error("GPT blob is {len} bytes; the partition entry array ends at {needed}")]
    TooShortForEntries { len: usize, needed: usize },
    #[error("GPT entry array CRC {stored:#010x} does not match computed {computed:#010x}")]
    EntriesCrc { stored: u32, computed: u32 },
    #[error("partition(s) not found in GPT: {}", .0.join(", "))]
    MissingPartitions(Vec<String>),
    #[error("modified GPT changed a retained partition entry")]
    RetainedEntriesChanged,
}

/// Header fields this tool reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GptHeader {
    pub header_size: usize,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub entries_lba: u64,
    pub num_entries: usize,
    pub entry_size: usize,
}

impl GptHeader {
    fn entries_offset(&self) -> usize {
        self.entries_lba as usize * SECTOR_SIZE
    }

    fn entries_len(&self) -> usize {
        self.num_entries * self.entry_size
    }

    /// Bytes from LBA 0 up to the first usable LBA: the whole primary GPT as
    /// Lenovo packages ship it (`gpt_main4.bin`, 6 sectors = 24 KiB).
    pub fn primary_gpt_len(&self) -> usize {
        self.first_usable_lba as usize * SECTOR_SIZE
    }
}

/// One used partition entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GptEntry {
    pub name: String,
    pub first_lba: u64,
    pub last_lba: u64,
    pub attributes: u64,
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn u64_at(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

/// Parse and validate the header in LBA 1 of `data`. `data` only needs to
/// reach the end of the header, so S3 can read the dump size from the first
/// two sectors.
pub fn parse_header(data: &[u8]) -> Result<GptHeader, GptError> {
    let needed = HEADER_OFFSET + MIN_HEADER_SIZE;
    if data.len() < needed {
        return Err(GptError::TooShortForHeader {
            len: data.len(),
            needed,
        });
    }
    let h = &data[HEADER_OFFSET..];
    if &h[..8] != SIGNATURE {
        return Err(GptError::BadSignature);
    }
    let header_size = u32_at(h, 12);
    if (header_size as usize) < MIN_HEADER_SIZE || header_size as usize > SECTOR_SIZE {
        return Err(GptError::BadHeaderSize(header_size));
    }
    let header_size = header_size as usize;
    if data.len() < HEADER_OFFSET + header_size {
        return Err(GptError::TooShortForHeader {
            len: data.len(),
            needed: HEADER_OFFSET + header_size,
        });
    }
    let stored = u32_at(h, 16);
    let computed = header_crc(&h[..header_size]);
    if stored != computed {
        return Err(GptError::HeaderCrc { stored, computed });
    }
    let entry_size = u32_at(h, 84);
    if (entry_size as usize) < MIN_ENTRY_SIZE {
        return Err(GptError::BadEntrySize(entry_size));
    }
    Ok(GptHeader {
        header_size,
        first_usable_lba: u64_at(h, 40),
        last_usable_lba: u64_at(h, 48),
        entries_lba: u64_at(h, 72),
        num_entries: u32_at(h, 80) as usize,
        entry_size: entry_size as usize,
    })
}

/// CRC32 of a header with its own CRC field (offset 16) taken as zero.
fn header_crc(header: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&header[..16]);
    hasher.update(&[0; 4]);
    hasher.update(&header[20..]);
    hasher.finalize()
}

/// A validated primary GPT blob.
#[derive(Debug, Clone)]
pub struct PrimaryGpt {
    bytes: Vec<u8>,
    header: GptHeader,
}

/// Slot-control flags are Qualcomm/Lapis flags, not Android misc metadata.
pub fn active_slot_from_attributes(a: u64, b: u64) -> Result<Slot, String> {
    // LinuxLoader initializes slot A on a virgin table. Only accept the
    // unambiguous all-zero slot-control state, never arbitrary invalid flags.
    if ((a | b) >> 48) & 0xff == 0 {
        return Ok(Slot::A);
    }
    let mut selected = None;
    let mut priority = 0;
    for (slot, attrs) in [(Slot::A, a), (Slot::B, b)] {
        let value = (attrs >> 48) & 3;
        if attrs & (1 << 50) != 0 && value > priority {
            selected = Some((slot, attrs));
            priority = value;
        }
    }
    let (slot, attrs) = selected.ok_or("GPT 슬롯 속성에서 활성 슬롯을 확인할 수 없습니다.")?;
    if attrs & (1 << 55) != 0 || (attrs & (1 << 54) == 0 && (attrs >> 51) & 7 == 0) {
        return Err("GPT의 활성 슬롯이 부팅 불가 또는 재시도 소진 상태입니다. 기기를 복구한 뒤 다시 시도하세요.".into());
    }
    Ok(slot)
}

/// A zero type GUID or a zero LBA range is an unused entry.
fn entry_used(entry: &[u8]) -> bool {
    entry[..16].iter().any(|&b| b != 0) && !(u64_at(entry, 32) == 0 && u64_at(entry, 40) == 0)
}

fn entry_name(entry: &[u8]) -> String {
    let units: Vec<u16> = entry[NAME_RANGE]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}

impl PrimaryGpt {
    /// Validate the header and entry-array CRCs. A dump that fails here is
    /// never modified or written back.
    pub fn parse(bytes: &[u8]) -> Result<Self, GptError> {
        let header = parse_header(bytes)?;
        let needed = header.entries_offset() + header.entries_len();
        if bytes.len() < needed {
            return Err(GptError::TooShortForEntries {
                len: bytes.len(),
                needed,
            });
        }
        let stored = u32_at(&bytes[HEADER_OFFSET..], 88);
        let computed = crc32fast::hash(&bytes[header.entries_offset()..needed]);
        if stored != computed {
            return Err(GptError::EntriesCrc { stored, computed });
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            header,
        })
    }

    pub fn header(&self) -> &GptHeader {
        &self.header
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn raw_entries(&self) -> impl Iterator<Item = &[u8]> {
        let start = self.header.entries_offset();
        self.bytes[start..start + self.header.entries_len()].chunks_exact(self.header.entry_size)
    }

    /// Used entries in table order.
    pub fn entries(&self) -> Vec<GptEntry> {
        self.raw_entries()
            .filter(|entry| entry_used(entry))
            .map(|entry| GptEntry {
                name: entry_name(entry),
                first_lba: u64_at(entry, 32),
                last_lba: u64_at(entry, 40),
                attributes: u64_at(entry, 48),
            })
            .collect()
    }

    pub fn find(&self, name: &str) -> Option<GptEntry> {
        self.entries().into_iter().find(|entry| entry.name == name)
    }

    /// Match Lapis LinuxLoader GetActiveSlot: active bit 50, priority 48..49,
    /// scanning A before B (ties retain A). Never infer from AVB timestamps.
    pub fn active_slot(&self) -> Result<Slot, String> {
        let entries = self.entries();
        let attrs = |name: &str| -> Result<u64, String> {
            let mut found = entries.iter().filter(|entry| entry.name == name);
            let entry = found
                .next()
                .ok_or_else(|| format!("GPT에 {name}가 없습니다."))?;
            if found.next().is_some() {
                return Err(format!("GPT에 {name}가 중복되었습니다."));
            }
            Ok(entry.attributes)
        };
        active_slot_from_attributes(attrs("boot_a")?, attrs("boot_b")?)
    }

    /// LTBox/gptman scans named unused entries too. Lenovo's zero-type-GUID
    /// `last_parti` describes the free tail, not a used partition. Expose only
    /// this strictly bounded marker for scan cross-checks; `find`, writes and
    /// GPT modification must continue to operate on used entries exclusively.
    pub(crate) fn scanned_end_marker(&self) -> Option<GptEntry> {
        let mut markers = self
            .raw_entries()
            .filter(|entry| !entry_used(entry) && entry_name(entry) == "last_parti");
        let raw = markers.next()?;
        if markers.next().is_some() {
            return None;
        }
        let first_lba = u64_at(raw, 32);
        let last_lba = u64_at(raw, 40);
        if first_lba < self.header.first_usable_lba
            || first_lba > last_lba
            || last_lba != self.header.last_usable_lba
            || self
                .entries()
                .iter()
                .any(|entry| entry.last_lba >= first_lba)
        {
            return None;
        }
        Some(GptEntry {
            name: "last_parti".into(),
            first_lba,
            last_lba,
            attributes: u64_at(raw, 48),
        })
    }

    /// Remove `vbmeta_a` regardless of slot, plus optional `vbmeta` and active
    /// `pvmfw_X`. Retain all other raw entries, including inactive pvmfw.
    pub fn remove_region_vbmeta(&self, slot: Slot) -> Result<Vec<u8>, GptError> {
        if self.find("vbmeta_a").is_none() {
            return Err(GptError::MissingPartitions(vec!["vbmeta_a".into()]));
        }

        let header = &self.header;
        let active_pvmfw = format!("pvmfw{}", slot.suffix());
        let mut packed = vec![0u8; header.entries_len()];
        let kept: Vec<&[u8]> = self
            .raw_entries()
            .filter(|entry| {
                let name = entry_name(entry);
                entry_used(entry)
                    && !matches!(name.as_str(), "vbmeta_a" | "vbmeta")
                    && name != active_pvmfw
            })
            .collect();
        for (slot_index, entry) in kept.iter().enumerate() {
            let at = slot_index * header.entry_size;
            packed[at..at + header.entry_size].copy_from_slice(entry);
        }

        let mut out = self.bytes.clone();
        let start = header.entries_offset();
        out[start..start + packed.len()].copy_from_slice(&packed);
        let array_crc = crc32fast::hash(&packed);
        out[HEADER_OFFSET + 88..HEADER_OFFSET + 92].copy_from_slice(&array_crc.to_le_bytes());
        let header_crc = header_crc(&out[HEADER_OFFSET..HEADER_OFFSET + header.header_size]);
        out[HEADER_OFFSET + 16..HEADER_OFFSET + 20].copy_from_slice(&header_crc.to_le_bytes());

        // Check complete raw retained entries: GUIDs, LBAs, attributes and names.
        let modified = Self::parse(&out)?;
        if modified
            .raw_entries()
            .filter(|entry| entry_used(entry))
            .collect::<Vec<_>>()
            != kept
        {
            return Err(GptError::RetainedEntriesChanged);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRIES_LBA: u64 = 2;
    const NUM_ENTRIES: usize = 96;

    /// A 6-sector primary GPT shaped like the TB520FU LUN 4 dump: protective
    /// MBR, header in LBA 1, 96 × 128-byte entries from LBA 2.
    fn build_gpt(names: &[&str]) -> Vec<u8> {
        let mut data = vec![0u8; 6 * SECTOR_SIZE];
        data[0x1c2] = 0xee;
        data[510] = 0x55;
        data[511] = 0xaa;
        let base = ENTRIES_LBA as usize * SECTOR_SIZE;
        for (index, name) in names.iter().enumerate() {
            let entry = &mut data[base + index * 128..base + (index + 1) * 128];
            entry[..16].copy_from_slice(&[0x11; 16]);
            entry[16..32].copy_from_slice(&[index as u8 + 1; 16]);
            let first = 100 + index as u64 * 10;
            entry[32..40].copy_from_slice(&first.to_le_bytes());
            entry[40..48].copy_from_slice(&(first + 9).to_le_bytes());
            for (unit, code) in name.encode_utf16().enumerate() {
                entry[56 + unit * 2..58 + unit * 2].copy_from_slice(&code.to_le_bytes());
            }
        }
        let h = HEADER_OFFSET;
        data[h..h + 8].copy_from_slice(SIGNATURE);
        data[h + 8..h + 12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        data[h + 12..h + 16].copy_from_slice(&92u32.to_le_bytes());
        data[h + 24..h + 32].copy_from_slice(&1u64.to_le_bytes());
        data[h + 40..h + 48].copy_from_slice(&6u64.to_le_bytes());
        data[h + 48..h + 56].copy_from_slice(&1000u64.to_le_bytes());
        data[h + 72..h + 80].copy_from_slice(&ENTRIES_LBA.to_le_bytes());
        data[h + 80..h + 84].copy_from_slice(&(NUM_ENTRIES as u32).to_le_bytes());
        data[h + 84..h + 88].copy_from_slice(&128u32.to_le_bytes());
        let array = crc32fast::hash(&data[base..base + NUM_ENTRIES * 128]);
        data[h + 88..h + 92].copy_from_slice(&array.to_le_bytes());
        let crc = header_crc(&data[h..h + 92]);
        data[h + 16..h + 20].copy_from_slice(&crc.to_le_bytes());
        data
    }

    const LAYOUT: &[&str] = &[
        "boot_a", "vbmeta_a", "pvmfw_a", "boot_b", "vbmeta_b", "pvmfw_b", "devinfo",
    ];

    fn names(gpt: &PrimaryGpt) -> Vec<String> {
        gpt.entries().into_iter().map(|entry| entry.name).collect()
    }

    #[test]
    fn parses_the_header_and_entries() {
        let gpt = PrimaryGpt::parse(&build_gpt(LAYOUT)).unwrap();
        assert_eq!(gpt.header().primary_gpt_len(), 6 * SECTOR_SIZE);
        assert_eq!(names(&gpt), LAYOUT);
        assert_eq!(gpt.find("boot_b").unwrap().first_lba, 130);
    }

    #[test]
    fn removes_exact_targets_and_preserves_all_other_raw_entries() {
        let layout = [
            "boot_a",
            "vbmeta_a",
            "pvmfw_a",
            "vbmeta",
            "boot_b",
            "vbmeta_b",
            "pvmfw_b",
            "pvmfw",
            "pvmfw_a_extra",
            "devinfo",
            "vbmeta_system_a",
            "vbmeta_a_extra",
        ];
        let original = PrimaryGpt::parse(&build_gpt(&layout)).unwrap();
        for slot in [Slot::A, Slot::B] {
            let active_pvmfw = format!("pvmfw{}", slot.suffix());
            let out = original.remove_region_vbmeta(slot).unwrap();
            let modified = PrimaryGpt::parse(&out).unwrap();
            let expected: Vec<&str> = layout
                .into_iter()
                .filter(|name| {
                    !matches!(*name, "vbmeta_a" | "vbmeta") && *name != active_pvmfw.as_str()
                })
                .collect();
            assert_eq!(names(&modified), expected);
            for entry in modified.entries() {
                assert_eq!(original.find(&entry.name), Some(entry));
            }
            assert_eq!(
                original
                    .raw_entries()
                    .filter(|entry| entry_used(entry)
                        && !matches!(entry_name(entry).as_str(), "vbmeta_a" | "vbmeta")
                        && entry_name(entry) != active_pvmfw)
                    .collect::<Vec<_>>(),
                modified
                    .raw_entries()
                    .filter(|entry| entry_used(entry))
                    .collect::<Vec<_>>()
            );
            let base = ENTRIES_LBA as usize * SECTOR_SIZE;
            assert!(
                out[base + expected.len() * 128..base + NUM_ENTRIES * 128]
                    .iter()
                    .all(|b| *b == 0)
            );
            let mut restored = out.clone();
            restored[base..base + NUM_ENTRIES * 128]
                .copy_from_slice(&original.as_bytes()[base..base + NUM_ENTRIES * 128]);
            for offset in [HEADER_OFFSET + 16, HEADER_OFFSET + 88] {
                restored[offset..offset + 4]
                    .copy_from_slice(&original.as_bytes()[offset..offset + 4]);
            }
            assert_eq!(restored, original.as_bytes());
            assert_eq!(out.len(), original.as_bytes().len());
        }
    }

    #[test]
    fn active_pvmfw_is_optional_and_other_slot_and_bare_variants_are_preserved() {
        for slot in [Slot::A, Slot::B] {
            for layout in [
                vec!["boot_a", "vbmeta_a", "boot_b"],
                vec!["boot_a", "vbmeta_a", "pvmfw_a", "pvmfw"],
                LAYOUT.to_vec(),
            ] {
                let original = PrimaryGpt::parse(&build_gpt(&layout)).unwrap();
                let modified =
                    PrimaryGpt::parse(&original.remove_region_vbmeta(slot).unwrap()).unwrap();
                let removed_pvmfw = format!("pvmfw{}", slot.suffix());
                assert!(modified.find("vbmeta_a").is_none());
                assert!(modified.find(&removed_pvmfw).is_none());
                let removed = 1 + usize::from(original.find(&removed_pvmfw).is_some());
                assert_eq!(modified.entries().len() + removed, original.entries().len());
                for name in [
                    format!("pvmfw{}", slot.other().suffix()),
                    "pvmfw".into(),
                    "vbmeta_b".into(),
                ] {
                    assert_eq!(modified.find(&name), original.find(&name));
                }
            }
        }
    }

    #[test]
    fn header_length_check_only_needs_the_first_two_sectors() {
        let data = build_gpt(LAYOUT);
        let header = parse_header(&data[..2 * SECTOR_SIZE]).unwrap();
        assert_eq!(header.primary_gpt_len(), 6 * SECTOR_SIZE);
    }

    #[test]
    fn rejects_damaged_or_incomplete_dumps() {
        let good = build_gpt(LAYOUT);

        let mut bad = good.clone();
        bad[HEADER_OFFSET] = b'X';
        assert_eq!(PrimaryGpt::parse(&bad).unwrap_err(), GptError::BadSignature);

        let mut bad = good.clone();
        bad[HEADER_OFFSET + 40] ^= 1;
        assert!(matches!(
            PrimaryGpt::parse(&bad),
            Err(GptError::HeaderCrc { .. })
        ));

        let mut bad = good.clone();
        bad[ENTRIES_LBA as usize * SECTOR_SIZE + 60] ^= 1;
        assert!(matches!(
            PrimaryGpt::parse(&bad),
            Err(GptError::EntriesCrc { .. })
        ));

        assert!(matches!(
            PrimaryGpt::parse(&good[..3 * SECTOR_SIZE]),
            Err(GptError::TooShortForEntries { .. })
        ));
        assert!(matches!(
            PrimaryGpt::parse(&good[..100]),
            Err(GptError::TooShortForHeader { .. })
        ));
    }

    #[test]
    fn refuses_when_a_target_entry_is_missing() {
        let gpt =
            PrimaryGpt::parse(&build_gpt(&["boot_a", "vbmeta", "vbmeta_b", "pvmfw_a"])).unwrap();
        assert_eq!(
            gpt.remove_region_vbmeta(Slot::A).unwrap_err(),
            GptError::MissingPartitions(vec!["vbmeta_a".into()])
        );
    }

    #[test]
    fn modifying_twice_fails_instead_of_dropping_more() {
        let gpt = PrimaryGpt::parse(&build_gpt(LAYOUT)).unwrap();
        let once = PrimaryGpt::parse(&gpt.remove_region_vbmeta(Slot::A).unwrap()).unwrap();
        assert!(matches!(
            once.remove_region_vbmeta(Slot::A),
            Err(GptError::MissingPartitions(_))
        ));
    }
}

#[cfg(test)]
mod slot_tests {
    use super::*;
    fn flags(priority: u64) -> u64 {
        (priority << 48) | (1 << 50) | (7 << 51)
    }
    #[test]
    fn lapis_slot_control_matches_priority_active_and_a_first_tie() {
        assert_eq!(
            active_slot_from_attributes(flags(3), flags(2)).unwrap(),
            Slot::A
        );
        assert_eq!(
            active_slot_from_attributes(flags(2), flags(3)).unwrap(),
            Slot::B
        );
        assert_eq!(
            active_slot_from_attributes(flags(3), flags(3)).unwrap(),
            Slot::A
        );
        assert_eq!(active_slot_from_attributes(0, flags(1)).unwrap(), Slot::B);
        assert_eq!(active_slot_from_attributes(0, 0).unwrap(), Slot::A);
        assert_eq!(
            active_slot_from_attributes(flags(3) & !(1 << 50), flags(1)).unwrap(),
            Slot::B
        );
        assert_eq!(
            active_slot_from_attributes(flags(3) & !(7 << 51) | (1 << 54), 0).unwrap(),
            Slot::A
        );
    }
    #[test]
    fn unusable_or_unidentifiable_slot_flags_are_rejected() {
        assert!(active_slot_from_attributes(flags(3) | (1 << 55), flags(2)).is_err());
        assert!(active_slot_from_attributes(flags(3) & !(7 << 51), 0).is_err());
        assert!(active_slot_from_attributes(1 << 48, 0).is_err());
        assert!(active_slot_from_attributes(flags(0), 0).is_err());
    }
}
