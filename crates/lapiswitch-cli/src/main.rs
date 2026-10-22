//! Development CLI: runs the region-change steps one at a time on a real
//! device. This CLI is intended for development and diagnostics.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
mod status;

use lapiswitch_core::firmware::check_firmware;
use lapiswitch_core::gpt::PrimaryGpt;
use lapiswitch_core::prepare::{native::NativeDevice, prepare};
use lapiswitch_core::region::Region;
use lapiswitch_core::slot::Slot;

const USAGE: &str = "\
usage: lapiswitch-cli <command>

commands:
  gpt-info <gpt_main4.bin>                 validate a LUN 4 primary GPT and list its entries
  gpt-modify <gpt_main4.bin> --slot <a|b> [-o <out>]
                                           remove vbmeta_a, plus vbmeta and active pvmfw if present
  decrypt-x <file.x> <out.xml>             decrypt one Lenovo .x file (LTBox crypto)
  check <firmware folder> --region <prc|row>
                                           S0: validate a firmware folder (decrypts .x in place, like LTBox)
  prepare <firmware folder> --region <prc|row> --output <new folder>
                                           S0-S2: check the ADB device, reboot to EDL (no partition writes)
  status [--state <file>]                  inspect state without connecting to a device";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = execute(&args);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn read_gpt(path: &Path) -> Result<PrimaryGpt, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    PrimaryGpt::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn gpt_info(args: &[String]) -> Result<(), String> {
    let [path] = args else {
        return Err(USAGE.to_string());
    };
    let gpt = read_gpt(Path::new(path))?;
    let header = gpt.header();
    println!(
        "header ok: {} entries × {} bytes from LBA {}, first usable LBA {} ({} bytes)",
        header.num_entries,
        header.entry_size,
        header.entries_lba,
        header.first_usable_lba,
        header.primary_gpt_len()
    );
    for (index, entry) in gpt.entries().iter().enumerate() {
        println!(
            "{index:3} {:<24} {:>10}..{:<10} attr {:#018x}",
            entry.name, entry.first_lba, entry.last_lba, entry.attributes
        );
    }
    Ok(())
}

fn parse_gpt_modify_args(args: &[String]) -> Result<(PathBuf, Slot, Option<PathBuf>), String> {
    let mut slot = None;
    let mut input = None;
    let mut output = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-o" | "--output" if output.is_none() => {
                output = Some(PathBuf::from(rest.next().ok_or(USAGE)?))
            }
            "--slot" if slot.is_none() => {
                slot = Some(
                    Slot::parse(rest.next().ok_or(USAGE)?)
                        .ok_or("활성 슬롯은 a 또는 b여야 합니다.")?,
                );
            }
            _ if !arg.starts_with('-') && input.is_none() => input = Some(PathBuf::from(arg)),
            _ => return Err(USAGE.into()),
        }
    }
    Ok((
        input.ok_or(USAGE)?,
        slot.ok_or("GPT 수정에는 활성 슬롯을 --slot a 또는 --slot b로 지정해야 합니다.")?,
        output,
    ))
}

fn gpt_modify(args: &[String]) -> Result<(), String> {
    let (input, slot, output) = parse_gpt_modify_args(args)?;
    let gpt = read_gpt(&input)?;
    let modified = gpt
        .remove_region_vbmeta(slot)
        .map_err(|error| format!("{}: {error}", input.display()))?;
    let output = output.unwrap_or_else(|| {
        let stem = input.file_stem().unwrap_or_default().to_string_lossy();
        input.with_file_name(format!("{stem}_modify.bin"))
    });
    std::fs::write(&output, &modified).map_err(|error| format!("{}: {error}", output.display()))?;
    println!("written: {}", output.display());
    Ok(())
}

fn decrypt_x(args: &[String]) -> Result<(), String> {
    let [input, output] = args else {
        return Err(USAGE.to_string());
    };
    let size = ltbox_core::crypto::decrypt_file(Path::new(input), Path::new(output))
        .map_err(|error| format!("{input}: {error}"))?;
    println!("{output}: {size} bytes");
    Ok(())
}

fn check(args: &[String]) -> Result<(), String> {
    let [folder, flag, region] = args else {
        return Err(USAGE.to_string());
    };
    if flag != "--region" {
        return Err(USAGE.to_string());
    }
    let target = match region.to_ascii_lowercase().as_str() {
        "prc" => Region::Prc,
        "row" => Region::Row,
        _ => return Err(format!("bad region `{region}`")),
    };
    let report = check_firmware(Path::new(folder), target).map_err(|error| error.to_string())?;
    println!("image folder : {}", report.image_dir.display());
    println!("loader       : {}", report.loader.display());
    println!("region       : {}", report.region.token());
    println!("fingerprint  : {}", report.fingerprint);
    println!("signing key  : {:?}", report.key_class);
    println!(
        "rollback     : boot {}, vbmeta_system {}",
        report.indices.boot, report.indices.vbmeta_system
    );
    println!("decrypted .x : {}", report.decrypted);
    let names = |paths: &[PathBuf]| {
        paths
            .iter()
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("rawprogram   : {}", names(&report.raw_xmls));
    println!("patch        : {}", names(&report.patch_xmls));
    println!(
        "LUN 4 GPT    : {}",
        if report.lun4_gpt_in_package {
            "written by the package"
        } else {
            "placeholder (restore from backup in S6)"
        }
    );
    for missing in &report.missing_images {
        println!(
            "warning      : {} {} -> {} missing (skipped, as in LTBox)",
            missing.xml, missing.label, missing.filename
        );
    }
    println!("result       : OK");
    Ok(())
}

/// --output must be new so device transcripts cannot overwrite older runs.
fn prepare_device(args: &[String]) -> Result<(), String> {
    let (folder, region, output) = parse_prepare_args(args)?;
    let firmware = check_firmware(&folder, region).map_err(|error| error.to_string())?;
    std::fs::create_dir(&output).map_err(|error| {
        format!(
            "{}: {error} (새 출력 폴더를 지정해 주세요)",
            output.display()
        )
    })?;
    let mut device = NativeDevice::new(&output)?;
    let mut log = vec![];
    let mut save_identity = |raw: &str| -> Result<(), String> {
        use std::io::Write;
        let path = output.join(lapiswitch_core::backup::IDENTITY_NAME);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        file.write_all(raw.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|error| error.to_string())
    };
    println!(
        "기기 확인과 EDL 전환을 시작합니다. 기록 폴더: {}",
        output.display()
    );
    let result = prepare(
        &mut device,
        Some(firmware.device),
        &firmware.loader,
        &mut save_identity,
        &mut log,
    )
    .map_err(|error| error.to_string())
    .and_then(|mut prepared| {
        // This standalone command ends here. Full/stage1 will retain the
        // session for S3; explicitly reset here so failure is reported.
        prepared
            .session
            .reset_to_edl(&mut log)
            .map_err(|error| error.to_string())?;
        Ok(prepared.report)
    });
    if let Err(error) = &result {
        log.push(format!("준비 실패: {error}"));
    }
    let log_result = std::fs::write(output.join("prepare.log"), log.join("\n"))
        .map_err(|error| error.to_string());
    // Keep the preparation failure visible even if saving the log also fails.
    match result {
        Ok(report) => {
            println!("활성 슬롯: {}", report.slot.suffix());
            println!("기록 폴더: {}", output.display());
            println!("준비 완료. GPT·boot·펌웨어는 쓰지 않았으며, 기기를 EDL로 돌려놓았습니다.");
            log_result
        }
        Err(error) => {
            if let Err(log_error) = log_result {
                eprintln!("로그 저장 실패: {log_error}");
            }
            Err(error)
        }
    }
}

fn parse_prepare_args(args: &[String]) -> Result<(PathBuf, Region, PathBuf), String> {
    let Some(folder) = args.first().filter(|arg| !arg.starts_with('-')) else {
        return Err(USAGE.into());
    };
    let mut region = None;
    let mut output = None;
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or(USAGE)?;
        match flag.as_str() {
            "--region" if region.is_none() => {
                region = Some(match value.to_ascii_lowercase().as_str() {
                    "prc" => Region::Prc,
                    "row" => Region::Row,
                    _ => return Err(format!("bad region `{value}`")),
                })
            }
            "--output" if output.is_none() => output = Some(PathBuf::from(value)),
            _ => return Err(USAGE.into()),
        }
    }
    Ok((
        PathBuf::from(folder),
        region.ok_or(USAGE)?,
        output.ok_or(USAGE)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(input: &[&str]) -> Vec<String> {
        input.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn prepare_requires_region_and_a_new_output_location() {
        let parsed = parse_prepare_args(&args(&[
            "C:/firmware",
            "--output",
            "C:/backup/run",
            "--region",
            "ROW",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            ("C:/firmware".into(), Region::Row, "C:/backup/run".into())
        );
        for invalid in [
            vec!["C:/firmware", "--region", "row"],
            vec!["C:/firmware", "--output", "C:/backup"],
            vec![
                "C:/firmware",
                "--region",
                "row",
                "--output",
                "C:/backup",
                "--force",
                "yes",
            ],
            vec![
                "C:/firmware",
                "--region",
                "row",
                "--region",
                "prc",
                "--output",
                "C:/backup",
            ],
        ] {
            assert!(parse_prepare_args(&args(&invalid)).is_err());
        }
    }
}

#[cfg(test)]
mod gpt_args_tests {
    use super::*;
    fn args(input: &[&str]) -> Vec<String> {
        input.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn gpt_modify_requires_active_slot_and_rejects_ambiguous_arguments() {
        for (slot, expected) in [("a", Slot::A), ("b", Slot::B)] {
            assert_eq!(
                parse_gpt_modify_args(&args(&["dump.bin", "--slot", slot])).unwrap(),
                ("dump.bin".into(), expected, None)
            );
            assert_eq!(
                parse_gpt_modify_args(&args(&["--slot", slot, "dump.bin", "-o", "modified.bin"]))
                    .unwrap(),
                ("dump.bin".into(), expected, Some("modified.bin".into()))
            );
        }
        for invalid in [
            vec![],
            vec!["dump.bin"],
            vec!["dump.bin", "--slot"],
            vec!["dump.bin", "--slot", "c"],
            vec!["dump.bin", "--slot", "a", "--slot", "b"],
            vec!["dump.bin", "--slot", "a", "--unknown"],
        ] {
            assert!(parse_gpt_modify_args(&args(&invalid)).is_err());
        }
    }
}

fn execute(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("gpt-info") => gpt_info(&args[1..]),
        Some("gpt-modify") => gpt_modify(&args[1..]),
        Some("decrypt-x") => decrypt_x(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("prepare") => prepare_device(&args[1..]),
        Some("status") => status::status(&args[1..]),
        _ => Err(USAGE.to_string()),
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn removed_and_unknown_commands_only_print_usage() {
        for command in ["restore", "assets", "stage1", "full", "resume", "unknown"] {
            let error = execute(&[command.into(), "nonexistent".into()]).unwrap_err();
            assert_eq!(error, USAGE);
        }
    }
}
