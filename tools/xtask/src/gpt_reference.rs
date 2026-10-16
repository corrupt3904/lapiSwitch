//! Independent byte-oriented GPT reference, replacing the old Python tool.
const SECTOR: usize = 4096;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn long(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
pub fn modify(data: &[u8], slot: &str) -> Result<Vec<u8>> {
    if !matches!(slot, "a" | "b") {
        return Err("Active slot must be a or b".into());
    }
    let header = data
        .get(SECTOR..)
        .filter(|h| h.len() >= 92)
        .ok_or("Truncated GPT header")?;
    if &header[..8] != b"EFI PART" {
        return Err("Invalid GPT signature".into());
    }
    let header_len = word(header, 12) as usize;
    if !(92..=SECTOR).contains(&header_len) || header_len > header.len() {
        return Err("Invalid GPT header length".into());
    }
    let mut header_copy = header[..header_len].to_vec();
    header_copy[16..20].fill(0);
    if crc32fast::hash(&header_copy) != word(header, 16) {
        return Err("Invalid GPT header CRC".into());
    }
    let count = word(header, 80) as usize;
    let width = word(header, 84) as usize;
    if width < 128 || count == 0 {
        return Err("Invalid GPT entry geometry".into());
    }
    let base = usize::try_from(long(header, 72))?
        .checked_mul(SECTOR)
        .ok_or("GPT offset overflow")?;
    let length = count.checked_mul(width).ok_or("GPT length overflow")?;
    let end = base.checked_add(length).ok_or("GPT range overflow")?;
    if base < 2 * SECTOR {
        return Err("GPT entries overlap the primary header".into());
    }
    let entries = data.get(base..end).ok_or("Truncated GPT array")?;
    if crc32fast::hash(entries) != word(header, 88) {
        return Err("Invalid GPT array CRC".into());
    }
    let pvmfw = format!("pvmfw_{slot}");
    let mut retained = Vec::new();
    let mut found = false;
    for entry in entries.chunks_exact(width) {
        if entry[..16].iter().all(|b| *b == 0) || long(entry, 32) == 0 || long(entry, 40) == 0 {
            continue;
        }
        let name = String::from_utf16_lossy(
            &entry[56..128]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|c| *c != 0)
                .collect::<Vec<_>>(),
        );
        if name == "vbmeta_a" {
            found = true;
        }
        if name != "vbmeta_a" && name != "vbmeta" && name != pvmfw {
            retained.extend_from_slice(entry);
        }
    }
    if !found {
        return Err("Required partition not found: vbmeta_a".into());
    }
    retained.resize(length, 0);
    let mut output = data.to_vec();
    output[base..end].copy_from_slice(&retained);
    output[SECTOR + 88..SECTOR + 92].copy_from_slice(&crc32fast::hash(&retained).to_le_bytes());
    output[SECTOR + 16..SECTOR + 20].fill(0);
    let crc = crc32fast::hash(&output[SECTOR..SECTOR + header_len]);
    output[SECTOR + 16..SECTOR + 20].copy_from_slice(&crc.to_le_bytes());
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bad_input_is_rejected_before_any_output_can_be_created() {
        assert!(modify(&[], "a").is_err());
        assert!(modify(&[0; 24576], "a").is_err());
        assert!(modify(&[0; 24576], "c").is_err());
    }
}
