//! Frozen UFS coordinates shared by backup and recovery. Never look up a
//! partition by its GPT entry number after modifying the table.

use serde::{Deserialize, Serialize};

use crate::gpt::{self, PrimaryGpt, SECTOR_SIZE};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    pub name: String,
    pub lun: u8,
    pub start_sector: u64,
    pub num_sectors: u64,
}

impl Partition {
    pub fn size_bytes(&self) -> Result<u64, String> {
        self.start_sector
            .checked_add(self.num_sectors)
            .filter(|_| self.lun <= 5 && self.num_sectors > 0)
            .ok_or_else(|| format!("잘못된 파티션 좌표: {}", self.name))?;
        self.num_sectors
            .checked_mul(SECTOR_SIZE as u64)
            .ok_or_else(|| format!("파티션 크기 범위 초과: {}", self.name))
    }

    pub fn prefix(&self, bytes: u64) -> Result<Self, String> {
        if bytes == 0 || bytes > self.size_bytes()? {
            return Err(format!("이미지가 파티션보다 큽니다: {}", self.name));
        }
        Ok(Self {
            num_sectors: bytes.div_ceil(SECTOR_SIZE as u64),
            ..self.clone()
        })
    }
}

pub fn find_partition<'a>(
    snapshot: &'a [Partition],
    name: &str,
    lun: u8,
) -> Result<&'a Partition, String> {
    let mut found = snapshot.iter().filter(|part| part.name == name);
    let part = found.next().ok_or_else(|| format!("파티션 없음: {name}"))?;
    if found.next().is_some() || part.lun != lun {
        return Err(format!("파티션 위치가 모호하거나 LUN이 다릅니다: {name}"));
    }
    part.size_bytes()?;
    Ok(part)
}

/// Bound untrusted header arithmetic before calling the existing GPT parser.
/// TB520FU uses six sectors; cap a primary GPT at 1 MiB before any disk read.
pub fn primary_geometry(header: &[u8]) -> Result<Partition, String> {
    let h = gpt::parse_header(header).map_err(|error| error.to_string())?;
    let end = h
        .entries_lba
        .checked_mul(SECTOR_SIZE as u64)
        .and_then(|start| {
            (h.num_entries as u64)
                .checked_mul(h.entry_size as u64)
                .and_then(|len| start.checked_add(len))
        })
        .ok_or("GPT 배열 범위 초과")?;
    let bytes = h
        .first_usable_lba
        .checked_mul(SECTOR_SIZE as u64)
        .ok_or("GPT 크기 범위 초과")?;
    if !(2..=256).contains(&h.first_usable_lba)
        || h.entries_lba < 2
        || h.num_entries == 0
        || h.entry_size % 128 != 0
        || end > bytes
        || h.last_usable_lba < h.first_usable_lba
    {
        return Err("지원하지 않거나 손상된 primary GPT 배치입니다.".into());
    }
    Ok(Partition {
        name: "gpt_main4".into(),
        lun: 4,
        start_sector: 0,
        num_sectors: h.first_usable_lba,
    })
}

pub fn validate_gpt(bytes: &[u8]) -> Result<PrimaryGpt, String> {
    let geometry = primary_geometry(bytes)?;
    if bytes.len() as u64 != geometry.size_bytes()? {
        return Err("GPT 백업 길이가 헤더와 다릅니다.".into());
    }
    let gpt = PrimaryGpt::parse(bytes).map_err(|error| error.to_string())?;
    let mut entries = gpt.entries();
    entries.sort_by_key(|entry| entry.first_lba);
    let mut names = std::collections::HashSet::new();
    let mut previous_end = None;
    for entry in entries {
        if !names.insert(entry.name.clone())
            || entry.first_lba < gpt.header().first_usable_lba
            || entry.last_lba > gpt.header().last_usable_lba
            || entry.last_lba < entry.first_lba
            || previous_end.is_some_and(|end| entry.first_lba <= end)
        {
            return Err(format!(
                "GPT 파티션 범위가 잘못되었거나 겹칩니다: {}",
                entry.name
            ));
        }
        previous_end = Some(entry.last_lba);
    }
    Ok(gpt)
}

pub fn match_gpt_partition(gpt: &PrimaryGpt, part: &Partition) -> Result<(), String> {
    let entry = gpt
        .find(&part.name)
        // The scanner includes Lenovo's named unused end marker. Validate its
        // exact coordinates without promoting arbitrary unused rows to real
        // partitions or changing the GPT edit/preservation policy.
        .or_else(|| {
            (part.name == "last_parti")
                .then(|| gpt.scanned_end_marker())
                .flatten()
        })
        .ok_or_else(|| format!("원본 GPT에 {}가 없습니다.", part.name))?;
    let sectors = entry
        .last_lba
        .checked_sub(entry.first_lba)
        .and_then(|n| n.checked_add(1));
    if part.lun != 4 || part.start_sector != entry.first_lba || Some(part.num_sectors) != sectors {
        return Err(format!("GPT와 고정 좌표가 다릅니다: {}", part.name));
    }
    Ok(())
}

/// The disk GUID survives our GPT edit. It is checked on a new restore session
/// before any writes; a different device/firmware GUID requires manual review.
pub fn disk_guid(header: &[u8]) -> Result<&[u8], String> {
    let guid = header
        .get(SECTOR_SIZE + 56..SECTOR_SIZE + 72)
        .ok_or("GPT 헤더가 짧습니다.")?;
    if guid.iter().all(|byte| *byte == 0) {
        return Err("GPT disk GUID가 비어 있습니다.".into());
    }
    Ok(guid)
}
