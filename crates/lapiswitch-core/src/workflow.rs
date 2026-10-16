//! Shared EDL, firmware and identity operations for the TWRP workflow.

use std::path::Path;

use ltbox_core::live;
use sha2::{Digest, Sha256};

use crate::backup::{self, VerifiedBackup};
use crate::country;
use crate::firmware::{FirmwareReport, MODEL};
use crate::gpt::{PrimaryGpt, SECTOR_SIZE};
use crate::stage1::StageIo;
use crate::state::{RunState, Stage, StateStore};
use crate::storage::{self, Partition};

pub mod native;

pub trait Clock {
    fn seconds(&self) -> u64;
    fn sleep_one_second(&mut self);
}
pub trait FinishIo: StageIo {
    fn probe_edl(&mut self) -> Result<Option<String>, String>;
    fn reboot_recovery(&mut self, log: &mut Vec<String>) -> Result<(), String>;
    fn open_edl(&mut self, loader: &Path, log: &mut Vec<String>) -> Result<(), String>;
    /// A bounded port-stabilization attempt may lose the enumerated device.
    /// False means no session was opened and the caller should keep watching.
    /// Actual handshake/configuration and selection errors remain errors.
    fn try_open_edl(&mut self, loader: &Path, log: &mut Vec<String>) -> Result<bool, String> {
        self.open_edl(loader, log)?;
        Ok(true)
    }
    fn flash_firmware(
        &mut self,
        firmware: &FirmwareReport,
        wipe: bool,
        log: &mut Vec<String>,
    ) -> Result<(), String>;
    fn set_active_a(&mut self, log: &mut Vec<String>) -> Result<(), String>;
    /// A reset handoff error is a warning after all writes have succeeded,
    /// matching LTBox reset_tolerant. It is not proof of a successful boot.
    fn final_reboot(&mut self, log: &mut Vec<String>) -> Option<String>;
}

fn read_primary<D: StageIo>(
    device: &mut D,
    lun: u8,
    work: &Path,
    tag: &str,
    log: &mut Vec<String>,
) -> Result<(Vec<u8>, PrimaryGpt), String> {
    let header = Partition {
        name: format!("header{lun}"),
        lun,
        start_sector: 0,
        num_sectors: 2,
    };
    let header_path = work.join(format!("{tag}_header{lun}.bin"));
    device.dump_at(&header, &header_path, log)?;
    let bytes = std::fs::read(&header_path).map_err(|error| error.to_string())?;
    if bytes.len() != 2 * SECTOR_SIZE {
        return Err("GPT 헤더 전송 길이가 다릅니다.".into());
    }
    let mut geometry = storage::primary_geometry(&bytes)?;
    geometry.lun = lun;
    let full = work.join(format!("{tag}_gpt{lun}.bin"));
    device.dump_at(&geometry, &full, log)?;
    let bytes = std::fs::read(&full).map_err(|error| error.to_string())?;
    let gpt = storage::validate_gpt(&bytes)?;
    Ok((bytes, gpt))
}

fn dump_bytes<D: StageIo>(
    device: &mut D,
    part: &Partition,
    path: &Path,
    log: &mut Vec<String>,
) -> Result<Vec<u8>, String> {
    device.dump_at(part, path, log)?;
    let data = std::fs::read(path).map_err(|error| error.to_string())?;
    if data.len() as u64 != part.size_bytes()? {
        return Err(format!("{} 전송 길이가 다릅니다.", part.name));
    }
    Ok(data)
}

fn original_gpt(backup: &VerifiedBackup) -> Result<Vec<u8>, String> {
    let bytes =
        std::fs::read(backup.file_path("gpt_main4.bin")?).map_err(|error| error.to_string())?;
    let entry = backup.entry("gpt_main4.bin")?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if bytes.len() as u64 != entry.size_bytes || hash != entry.sha256 {
        return Err("원본 GPT 백업이 변경되었습니다.".into());
    }
    storage::validate_gpt(&bytes)?;
    Ok(bytes)
}

fn confirm_model<D: StageIo>(device: &mut D, log: &mut Vec<String>) -> Result<(), String> {
    let a = device.read_avb("vbmeta_system_a", log)?;
    let b = device.read_avb("vbmeta_system_b", log)?;
    if [a, b]
        .into_iter()
        .flatten()
        .filter_map(|avb| avb.build_fingerprint())
        .any(|fp| ltbox_core::model::fingerprint_model_match(&fp, MODEL))
    {
        Ok(())
    } else {
        Err("S6 기기가 TB520FU로 확인되지 않았습니다.".into())
    }
}

pub(crate) fn confirm_identity<D: StageIo>(
    device: &mut D,
    backup: &VerifiedBackup,
    state: &mut RunState,
    store: &StateStore,
    work: &Path,
    log: &mut Vec<String>,
) -> Result<(), String> {
    let persist = storage::find_partition(&backup.manifest().snapshot, "persist", 0)?;
    let bytes = dump_bytes(device, persist, &work.join("identity_persist.img"), log)?;
    let identity = country::persist_fingerprint(&bytes);
    if let Some(expected) = &state.persist_fingerprint {
        // A failed S6 may leave vbmeta_system unparseable. A previously bound
        // persist fingerprint still identifies the same physical device.
        if expected != &identity {
            return Err(
                "저장된 작업과 기기의 persist 지문이 다릅니다. 올바른 기기를 연결하세요.".into(),
            );
        }
    } else {
        if state.stage == Stage::S6 {
            return Err("S6 기기 식별 기록이 누락되었습니다.".into());
        }
        confirm_model(device, log)?;
        let (live, _) = read_primary(device, 4, work, "identity", log)?;
        let original = original_gpt(backup)?;
        if storage::disk_guid(&live)? != storage::disk_guid(&original)? {
            return Err("S5 기기와 백업의 disk GUID가 다릅니다.".into());
        }
        state.persist_fingerprint = Some(identity);
        store.save(state)?;
    }
    Ok(())
}

pub(crate) fn restore_gpt_if_needed<D: StageIo>(
    device: &mut D,
    backup: &VerifiedBackup,
    work: &Path,
    log: &mut Vec<String>,
) -> Result<(), String> {
    let original = original_gpt(backup)?;
    let original_table = storage::validate_gpt(&original)?;
    // Restore the deletion targets (including active pvmfw) and retain all
    // other vbmeta/pvmfw variants. Require optional entries only if present.
    let required: Vec<String> = original_table
        .entries()
        .into_iter()
        .map(|entry| entry.name)
        .filter(|name| {
            matches!(name.as_str(), "vbmeta_a" | "vbmeta" | "vbmeta_b" | "pvmfw")
                || name.starts_with("pvmfw_")
        })
        .collect();
    let has_targets = |gpt: &PrimaryGpt| required.iter().all(|name| gpt.find(name).is_some());
    match read_primary(device, 4, work, "after_flash", log) {
        Ok((_, gpt)) if has_targets(&gpt) => return Ok(()),
        Ok(_) => live!(log, "S6: GPT 대상 항목이 없어 S3 원본을 복원합니다."),
        Err(error) => live!(log, "S6: GPT 확인 실패: {error}. S3 원본을 복원합니다."),
    }
    let geometry = storage::primary_geometry(&original)?;
    let path = work.join("restore_gpt_main4.bin");
    backup::write_new(&path, &original)?;
    device.write_at(&geometry, &path, log)?;
    let readback = dump_bytes(device, &geometry, &work.join("restored_gpt.bin"), log)?;
    if readback != original || !has_targets(&storage::validate_gpt(&readback)?) {
        return Err("S6 GPT 복원 검증 실패".into());
    }
    Ok(())
}

pub(crate) fn country_change<D: StageIo>(
    device: &mut D,
    code: &str,
    work: &Path,
    log: &mut Vec<String>,
) -> Vec<String> {
    let mut warnings = vec![];
    let mut staged = vec![];
    for (name, lun, field_only) in [("devinfo", 4, false), ("persist", 0, true)] {
        let result = (|| -> Result<Option<(Partition, Vec<u8>)>, String> {
            // Read GPT afresh: LTBox's cached GPT may predate rawprogram writes.
            let (_, gpt) = read_primary(device, lun, work, &format!("country_{name}"), log)?;
            let entry = gpt
                .find(name)
                .ok_or_else(|| format!("국가 코드 파티션 없음: {name}"))?;
            let part = Partition {
                name: name.into(),
                lun,
                start_sector: entry.first_lba,
                num_sectors: entry.last_lba - entry.first_lba + 1,
            };
            let bytes = dump_bytes(
                device,
                &part,
                &work.join(format!("{name}.current.img")),
                log,
            )?;
            let Some(old) = country::detect(&bytes, field_only) else {
                if name == "persist" {
                    return Ok(None);
                }
                return Err(format!("{name}에서 알려진 국가 코드를 찾지 못했습니다."));
            };
            let patched = country::patch(&bytes, old, code, field_only)?;
            if patched == bytes {
                return Ok(None);
            }
            Ok(Some((part, patched)))
        })();
        match result {
            Ok(Some(item)) => staged.push(item),
            Ok(None) => live!(log, "S6: {name} 국가 코드 변경 불필요"),
            Err(error) => warnings.push(error),
        }
    }
    for (part, bytes) in staged {
        let result = (|| -> Result<(), String> {
            let path = work.join(format!("{}.patched.img", part.name));
            backup::write_new(&path, &bytes)?;
            device.write_at(&part, &path, log)?;
            if dump_bytes(
                device,
                &part,
                &work.join(format!("{}.verify.img", part.name)),
                log,
            )? != bytes
            {
                return Err(format!("{} 국가 코드 쓰기 후 검증 실패", part.name));
            }
            Ok(())
        })();
        if let Err(error) = result {
            warnings.push(format!("{} 국가 코드 변경 실패: {error}", part.name));
        }
    }
    warnings
}
