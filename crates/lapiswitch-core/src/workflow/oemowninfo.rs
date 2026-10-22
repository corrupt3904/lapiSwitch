//! Country change in a fixed partition field (`[country] method =
//! "oemowninfo"`), with durable write evidence separate from the immutable S3
//! original. Never retries a write whose result cannot be established.
use super::{dump_bytes, read_primary};
use crate::{
    backup::{self, VerifiedBackup},
    country::oemowninfo::{self, OemOwnInfo},
    stage1::StageIo,
    storage::Partition,
};
use ltbox_core::live;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u8,
    code: String,
    partition: Partition,
    original_sha256: String,
    before_sha256: String,
    planned_sha256: String,
    verified: bool,
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn keep(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.try_exists().map_err(|e| e.to_string())? {
        if std::fs::read(path).map_err(|e| e.to_string())? != bytes {
            return Err(format!("국가코드 증거 파일이 다릅니다: {}", path.display()));
        }
        Ok(())
    } else {
        backup::write_new(path, bytes)
    }
}

pub(crate) fn change(
    device: &mut impl StageIo,
    backup: &VerifiedBackup,
    field: &OemOwnInfo,
    code: &str,
    work: &Path,
    log: &mut Vec<String>,
) -> Result<(), String> {
    backup.verify()?;
    let code = crate::country::validate(code)?;
    let name = field.partition;
    let original = backup.entry(&format!("{name}.img"))?;
    let expected = original
        .partition
        .as_ref()
        .ok_or_else(|| format!("{name} 백업 좌표 누락"))?;
    let (_, gpt) = read_primary(device, field.lun, work, &format!("country_{name}"), log)?;
    let entry = gpt
        .find(name)
        .ok_or_else(|| format!("기기에 {name} 파티션이 없습니다."))?;
    let part = Partition {
        name: name.into(),
        lun: field.lun,
        start_sector: entry.first_lba,
        num_sectors: entry.last_lba - entry.first_lba + 1,
    };
    oemowninfo::validate_partition(field, &part)?;
    if &part != expected {
        return Err(format!("현재 {name} 좌표가 원본 백업과 다릅니다."));
    }
    device.ensure_single_target()?;
    let current = dump_bytes(
        device,
        &part,
        &work.join(format!("{name}.current.img")),
        log,
    )?;
    let second = dump_bytes(device, &part, &work.join(format!("{name}.second.img")), log)?;
    if current != second {
        return Err(format!("{name} 두 번 읽기 결과가 달라 쓰지 않았습니다."));
    }
    oemowninfo::detect(field, &current)?;

    let dir = backup.work_dir()?;
    let journal = dir.join(format!("{name}.country.json"));
    let before_file = dir.join(format!("{name}.country.before.img"));
    let planned_file = dir.join(format!("{name}.country.planned.img"));
    let mut intent;
    let planned;
    if journal.try_exists().map_err(|e| e.to_string())? {
        intent = backup::read_json::<Intent>(&journal)?;
        let before = std::fs::read(&before_file).map_err(|e| e.to_string())?;
        planned = std::fs::read(&planned_file).map_err(|e| e.to_string())?;
        if intent.version != 1
            || intent.code != code
            || intent.partition != part
            || intent.original_sha256 != original.sha256
            || intent.before_sha256 != hash(&before)
            || intent.planned_sha256 != hash(&planned)
            || oemowninfo::plan(field, &before, &code)? != planned
        {
            return Err(format!("{name} 국가코드 쓰기 기록/파일 검증 실패"));
        }
        if current != planned && (intent.verified || current != before) {
            return Err(format!(
                "{name} 쓰기 결과를 확정할 수 없습니다. 자동 재쓰기 없이 백업과 기록을 보존합니다."
            ));
        }
    } else {
        planned = oemowninfo::plan(field, &current, &code)?;
        keep(&before_file, &current)?;
        keep(&planned_file, &planned)?;
        intent = Intent {
            version: 1,
            code: code.clone(),
            partition: part.clone(),
            original_sha256: original.sha256.clone(),
            before_sha256: hash(&current),
            planned_sha256: hash(&planned),
            verified: false,
        };
        backup::write_json_atomic(&journal, &intent)?;
    }
    if current != planned {
        // Recheck the actual file consumed by EDL after the durable intent.
        if std::fs::read(&planned_file).map_err(|e| e.to_string())? != planned {
            return Err(format!("{name} 예정 파일이 쓰기 전에 변경되었습니다."));
        }
        live!(
            log,
            "국가코드: {} → {code}. {name}의 0x{:X}~0x{:X}만 변경합니다.",
            oemowninfo::detect(field, &current)?,
            field.offset,
            field.offset + 3
        );
        device.write_at(&part, &planned_file, log).map_err(|e| {
            format!("{name} 국가코드 쓰기 결과 불명: {e}. 백업과 기록을 보존합니다.")
        })?;
    } else {
        live!(
            log,
            "국가코드가 목표값 {code}와 같습니다. 쓰기를 생략하고 전체를 검증합니다."
        );
    }
    let verified = dump_bytes(device, &part, &work.join(format!("{name}.verify.img")), log)?;
    if verified != planned {
        return Err(format!(
            "{name} 국가코드 쓰기 후 전체 재읽기 검증 실패. 백업과 기록을 보존합니다."
        ));
    }
    keep(&dir.join(format!("{name}.country.verified.img")), &verified)?;
    intent.verified = true;
    backup::write_json_atomic(&journal, &intent)?;
    live!(
        log,
        "국가코드 {code} 및 {name} 전체 {}바이트 재읽기 검증 완료.",
        verified.len()
    );
    Ok(())
}
