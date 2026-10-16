//! Device firmware admission at S3, from the active slot's AVB fingerprint.
//! This does not restrict the selected installation package or original restore.
use crate::region::Region;

pub fn check(fingerprint: &str) -> Result<String, String> {
    let unknown = || {
        "활성 슬롯의 vbmeta_system에서 기기 펌웨어 버전·리전을 확정할 수 없습니다. 작업을 시작하지 않습니다.".to_owned()
    };
    // Android fingerprints: brand/product/device:release/build/incremental:type/tags.
    // Never mistake the Android release, build ID or build date for ROM version.
    let mut fields = fingerprint.split(':');
    fields.next().ok_or_else(unknown)?;
    let build = fields.next().ok_or_else(unknown)?;
    let mut build = build.split('/');
    let _android = build.next().ok_or_else(unknown)?;
    let _id = build.next().ok_or_else(unknown)?;
    let incremental = build.next().ok_or_else(unknown)?;
    if build.next().is_some() {
        return Err(unknown());
    }
    let (prefix, region) = incremental.rsplit_once('_').ok_or_else(unknown)?;
    let region = match region {
        "PRC" => Region::Prc,
        "ROW" => Region::Row,
        _ => return Err(unknown()),
    };
    let mut tokens = prefix.split('_');
    let family = tokens.next().ok_or_else(unknown)?;
    let raw = tokens.next().ok_or_else(unknown)?;
    if let Some(date) = tokens.next()
        && (!matches!(date.len(), 6 | 8) || !date.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(unknown());
    }
    if tokens.next().is_some() {
        return Err(unknown());
    }
    let numbers: Vec<u32> = raw
        .split('.')
        .map(|p| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err(unknown());
            }
            p.parse().map_err(|_| unknown())
        })
        .collect::<Result<_, _>>()?;
    let version: [u32; 4] = numbers.try_into().map_err(|_| unknown())?;
    let limit = match (region, family) {
        (Region::Prc, "ZUXOS") => Some(([1, 5, 10, 186], "ZUXOS_1.5.10.186")),
        // The user explicitly permits PRC's preceding ZUI firmware family.
        (Region::Prc, "ZUI") => None,
        (Region::Row, "ZUI") => Some(([18, 5, 10, 37], "ZUI_18.5.10.037")),
        _ => return Err(unknown()),
    };
    let display = format!("{} {family}_{raw}", region.token());
    if let Some((ceiling, label)) = limit
        && version > ceiling
    {
        return Err(format!(
            "현재 기기 펌웨어 {display}는 지원 상한 {label}을 초과합니다. 해당 버전에서는 리전 변환을 시작할 수 없습니다."
        ));
    }
    Ok(display)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fp(version: &str) -> String {
        format!(
            "qti/TB520FU/TB520FU:16/BQ2A.250610.001-BP2A.250605.031.A3/{version}:user/release-keys"
        )
    }
    #[test]
    fn ceilings_are_numeric_inclusive_and_apply_to_device_region() {
        for v in [
            "ZUXOS_1.5.10.185_260408_PRC",
            "ZUXOS_1.5.10.186_260408_PRC",
            "ZUXOS_1.5.9.999_260408_PRC",
            "ZUI_18.5.10.036_260408_ROW",
            "ZUI_18.5.10.037_260408_ROW",
            "ZUI_17.5.10.309_260528_ROW",
            "ZUI_16.1.10.027_241129_ROW",
            "ZUI_16.1.10.027_241129_PRC",
            "ZUI_17.5.10.309_260528_PRC",
        ] {
            assert!(check(&fp(v)).is_ok(), "{v}");
        }
        for v in [
            "ZUXOS_1.5.10.187_260408_PRC",
            "ZUXOS_1.6.0.0_260408_PRC",
            "ZUI_18.5.10.038_260408_ROW",
            "ZUI_19.1.0.0_260408_ROW",
        ] {
            assert!(check(&fp(v)).unwrap_err().contains("초과"), "{v}");
        }
    }
    #[test]
    fn unidentified_or_conflicting_versions_never_pass() {
        for v in [
            "ROW",
            "build_ROW",
            "ZUXOS_1.5.10.186_260408_ROW",
            "ZUI_18.5.10_260408_ROW",
            "ZUI_18.5.x.037_260408_ROW",
            "ZUI_-18.5.10.037_260408_ROW",
            "ZUI_18.5.10.037_evil_ROW",
            "ZUI_18.5.10.037_260408_PRC_ROW",
        ] {
            assert!(check(&fp(v)).is_err(), "{v}");
        }
        assert!(check("qti/TB520FU/TB520FU:16/ZUI_18.5.10.037_ROW:user/release-keys").is_err());
    }
}
