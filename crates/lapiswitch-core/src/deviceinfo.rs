//! QSEECom DeviceInfo of the running model, independent of transport. Field
//! layouts live in `map`; plans are field edits applied to the live buffer.
pub mod map;
use crate::region::Region;
use serde::{Deserialize, Serialize};

pub const SHARED_SIZE: usize = 0x1000;
/// The model this build converts and edits. TB522FU's layout is defined in
/// `map` but not connected to a device flow yet.
pub const LAYOUT: map::Layout = map::Layout::Tb520fu;

/// Load the TA from the device's confirmed active slot. Firmware-specific
/// hashes/signatures do not determine DeviceInfo compatibility.
pub const fn keymaster_path(slot: crate::slot::Slot) -> &'static str {
    match slot {
        crate::slot::Slot::A => "/dev/block/bootdevice/by-name/keymaster_a",
        crate::slot::Slot::B => "/dev/block/bootdevice/by-name/keymaster_b",
    }
}

/// Rollback preset of a region conversion. New GUI conversions default to RESET.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RollbackMode {
    Reset,
    Keep,
}

/// PRC/ROW of a buffer with the DeviceInfo magic.
pub fn region(bytes: &[u8]) -> Result<Region, String> {
    LAYOUT.check_buffer(bytes)?;
    LAYOUT
        .region(bytes)
        .ok_or_else(|| "DeviceInfo 리전 필드가 PRC/ROW가 아닙니다.".into())
}

pub fn indices(bytes: &[u8]) -> [u64; map::ROLLBACK_COUNT] {
    LAYOUT.rollback_indices(bytes)
}

/// Set all 32 rollback indices to zero on top of existing edits.
pub fn with_rollback_reset(before: &[u8], edits: &map::Edits) -> Result<map::Edits, String> {
    let mut planned = map::apply(LAYOUT, before, edits)?;
    planned[map::ROLLBACK_START..map::ROLLBACK_START + map::ROLLBACK_COUNT * 8].fill(0);
    Ok(map::edits_between(LAYOUT, before, &planned))
}

/// Region conversion preset: the target region, optionally all rollback
/// indices reset. Every other field stays as read.
pub fn conversion_edits(
    before: &[u8],
    target: Region,
    mode: RollbackMode,
) -> Result<map::Edits, String> {
    region(before)?;
    let field = LAYOUT.field("region")?;
    let mut planned = before.to_vec();
    planned[field.range()].copy_from_slice(&map::encode(&field, target.token())?);
    let edits = map::edits_between(LAYOUT, before, &planned);
    match mode {
        RollbackMode::Keep => Ok(edits),
        RollbackMode::Reset => with_rollback_reset(before, &edits),
    }
}

/// Both readbacks must equal the full 4096-byte plan.
pub fn verify(planned: &[u8], same: &[u8], fresh_session: &[u8]) -> Result<(), String> {
    if planned.len() != SHARED_SIZE || same != planned || fresh_session != planned {
        return Err(
            "DeviceInfo 재읽기 전체 4096바이트 검증 실패. 재쓰지 않고 현재 상태를 확인하세요."
                .into(),
        );
    }
    Ok(())
}

/// Only BCB command[32], preserving the other bytes of the dumped partition.
pub fn recovery_misc(original: &[u8]) -> Result<Vec<u8>, String> {
    if original.len() < 32 {
        return Err("misc 덤프가 너무 짧습니다.".into());
    }
    let mut modified = original.to_vec();
    modified[..32].fill(0);
    modified[..13].copy_from_slice(b"boot-recovery");
    Ok(modified)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn sample(r: Region) -> Vec<u8> {
        let mut b: Vec<u8> = (0..SHARED_SIZE).map(|i| (i * 17 + 5) as u8).collect();
        b[..13].copy_from_slice(b"ANDROID-BOOT!");
        b[0xCC8..0xCD0].copy_from_slice(b"deadbeef");
        b[0xD20..0xD28].copy_from_slice(b"beefdead");
        b[0xD10..0xD20].fill(0);
        b[0xD10..0xD13].copy_from_slice(r.token().as_bytes());
        b
    }
    #[test]
    fn conversion_presets_change_only_region_and_optionally_all_rollback_indices() {
        for current in [Region::Prc, Region::Row] {
            for target in [Region::Prc, Region::Row] {
                for mode in [RollbackMode::Keep, RollbackMode::Reset] {
                    let b = sample(current);
                    let edits = conversion_edits(&b, target, mode).unwrap();
                    let p = map::apply(LAYOUT, &b, &edits).unwrap();
                    assert_eq!(region(&p).unwrap(), target);
                    if mode == RollbackMode::Reset {
                        assert_eq!(indices(&p), [0; 32]);
                    } else {
                        assert_eq!(indices(&p), indices(&b));
                    }
                    for (i, (a, c)) in b.iter().zip(&p).enumerate() {
                        let allowed = (0xD10..0xD20).contains(&i)
                            || (mode == RollbackMode::Reset && (0x898..0x998).contains(&i));
                        assert!(allowed || a == c, "{i:X}");
                    }
                    verify(&p, &p, &p).unwrap();
                    let mut bad = p.clone();
                    bad[SHARED_SIZE - 1] ^= 1;
                    assert!(verify(&p, &p, &bad).is_err());
                }
            }
        }
    }
    #[test]
    fn same_region_keep_is_empty_but_reset_still_clears_indices() {
        let b = sample(Region::Row);
        assert!(
            conversion_edits(&b, Region::Row, RollbackMode::Keep)
                .unwrap()
                .is_empty()
        );
        let reset = conversion_edits(&b, Region::Row, RollbackMode::Reset).unwrap();
        assert!(reset.keys().all(|key| key.starts_with("rollback_index[")));
        assert!(!reset.is_empty());
    }
    #[test]
    fn misc_only_changes_command() {
        let b = vec![0x7A; 1024];
        let p = recovery_misc(&b).unwrap();
        assert_eq!(&p[..13], b"boot-recovery");
        assert_eq!(&p[13..32], &[0; 19]);
        assert_eq!(&p[32..], &b[32..]);
        assert!(recovery_misc(&[0; 31]).is_err());
    }
}
