//! TB520FU QSEECom DeviceInfo layout, independent of transport.
use crate::region::Region;
use serde::{Deserialize, Serialize};

pub const SHARED_SIZE: usize = 0x1000;
pub const LOGICAL_SIZE: usize = 0xEA8;
pub const REGION_OFFSET: usize = 0xD10;
pub const ROLLBACK_START: usize = 0x898;
pub const ROLLBACK_END: usize = 0x998;
/// Load the TA from the device's confirmed active slot. Firmware-specific
/// hashes/signatures do not determine DeviceInfo compatibility.
pub const fn keymaster_path(slot: crate::slot::Slot) -> &'static str {
    match slot {
        crate::slot::Slot::A => "/dev/block/bootdevice/by-name/keymaster_a",
        crate::slot::Slot::B => "/dev/block/bootdevice/by-name/keymaster_b",
    }
}

/// Recorded explicitly in every run. New GUI conversions default to RESET.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RollbackMode {
    Reset,
    Keep,
}

pub fn region(bytes: &[u8]) -> Result<Region, String> {
    if bytes.len() != SHARED_SIZE
        || &bytes[..13] != b"ANDROID-BOOT!"
        || &bytes[0xCC8..0xCD0] != b"deadbeef"
        || &bytes[0xD20..0xD28] != b"beefdead"
        || bytes[REGION_OFFSET + 3] != 0
    {
        return Err("DeviceInfo 길이·매직·리전 형식이 지원 프로파일과 다릅니다.".into());
    }
    match &bytes[REGION_OFFSET..REGION_OFFSET + 3] {
        b"PRC" => Ok(Region::Prc),
        b"ROW" => Ok(Region::Row),
        _ => Err("DeviceInfo에 알려지지 않은 리전이 있습니다.".into()),
    }
}

pub fn indices(bytes: &[u8]) -> Result<[u64; 32], String> {
    region(bytes)?;
    Ok(std::array::from_fn(|i| {
        u64::from_le_bytes(
            bytes[ROLLBACK_START + i * 8..ROLLBACK_START + (i + 1) * 8]
                .try_into()
                .expect("fixed range"),
        )
    }))
}

pub fn plan(before: &[u8], target: Region, mode: RollbackMode) -> Result<Vec<u8>, String> {
    region(before)?;
    let mut planned = before.to_vec();
    planned[REGION_OFFSET..REGION_OFFSET + 3].copy_from_slice(target.token().as_bytes());
    if mode == RollbackMode::Reset {
        planned[ROLLBACK_START..ROLLBACK_END].fill(0);
    }
    validate_plan(before, &planned, target, mode)?;
    Ok(planned)
}

pub fn validate_plan(
    before: &[u8],
    planned: &[u8],
    target: Region,
    mode: RollbackMode,
) -> Result<(), String> {
    region(before)?;
    if region(planned)? != target {
        return Err("예정 리전이 선택과 다릅니다.".into());
    }
    for i in 0..SHARED_SIZE {
        let allowed = (REGION_OFFSET..REGION_OFFSET + 3).contains(&i)
            || (mode == RollbackMode::Reset && (ROLLBACK_START..ROLLBACK_END).contains(&i));
        if !allowed && before[i] != planned[i] {
            return Err(format!("DeviceInfo 보호 영역 변경: 0x{i:X}"));
        }
    }
    if mode == RollbackMode::Reset
        && planned[ROLLBACK_START..ROLLBACK_END]
            .iter()
            .any(|b| *b != 0)
    {
        return Err("롤백인덱스 32개가 모두 0으로 계획되지 않았습니다.".into());
    }
    Ok(())
}

pub fn verify(planned: &[u8], same: &[u8], fresh_session: &[u8]) -> Result<(), String> {
    region(planned)?;
    if same != planned || fresh_session != planned {
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
        b[REGION_OFFSET..REGION_OFFSET + 3].copy_from_slice(r.token().as_bytes());
        b[REGION_OFFSET + 3] = 0;
        b
    }
    #[test]
    fn both_regions_modes_and_every_protected_byte() {
        for current in [Region::Prc, Region::Row] {
            for target in [Region::Prc, Region::Row] {
                for mode in [RollbackMode::Keep, RollbackMode::Reset] {
                    let b = sample(current);
                    let p = plan(&b, target, mode).unwrap();
                    if mode == RollbackMode::Reset {
                        assert_eq!(indices(&p).unwrap(), [0; 32]);
                    }
                    for i in 0..SHARED_SIZE {
                        if (REGION_OFFSET..REGION_OFFSET + 3).contains(&i)
                            || (mode == RollbackMode::Reset
                                && (ROLLBACK_START..ROLLBACK_END).contains(&i))
                        {
                            continue;
                        }
                        let mut corrupt = p.clone();
                        corrupt[i] ^= 1;
                        assert!(validate_plan(&b, &corrupt, target, mode).is_err(), "{i:X}");
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
    fn same_region_still_resets_indices() {
        let b = sample(Region::Row);
        assert_eq!(plan(&b, Region::Row, RollbackMode::Keep).unwrap(), b);
        assert_ne!(plan(&b, Region::Row, RollbackMode::Reset).unwrap(), b);
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
