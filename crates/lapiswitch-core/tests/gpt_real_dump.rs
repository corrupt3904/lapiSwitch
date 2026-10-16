//! Current GPT rule, byte-for-byte against tools/gpt_Modify_fixed.py on a local
//! device dump. These GUID-bearing files are ignored and never committed:
//! gpt_main4.bin and gpt_main4_modify_fixed{,_b}.py.bin. Generate with
//! python tools/gpt_Modify_fixed.py <dump> --slot <a|b> -o <reference>.
use lapiswitch_core::gpt::{PrimaryGpt, SECTOR_SIZE};
use std::path::PathBuf;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/local")
        .join(name)
}
#[test]
fn matches_current_python_rule_on_the_real_dump() {
    let Ok(dump) = std::fs::read(fixture("gpt_main4.bin")) else {
        eprintln!("skipped: tests/fixtures/local/gpt_main4.bin not present");
        return;
    };
    let gpt = PrimaryGpt::parse(&dump).expect("real dump parses with valid CRCs");
    lapiswitch_core::storage::validate_gpt(&dump).unwrap();
    lapiswitch_core::storage::disk_guid(&dump).unwrap();
    assert_eq!(gpt.header().primary_gpt_len(), 6 * SECTOR_SIZE);
    assert_eq!(dump.len(), gpt.header().primary_gpt_len());
    for (slot, reference) in [
        (
            lapiswitch_core::slot::Slot::A,
            "gpt_main4_modify_fixed.py.bin",
        ),
        (
            lapiswitch_core::slot::Slot::B,
            "gpt_main4_modify_fixed_b.py.bin",
        ),
    ] {
        let expected = std::fs::read(fixture(reference))
            .expect("generate current reference using tools/gpt_Modify_fixed.py --slot a|b");
        let actual = gpt.remove_region_vbmeta(slot).unwrap();
        assert!(actual == expected, "current Python reference differs");
        let modified = PrimaryGpt::parse(&actual).unwrap();
        assert!(modified.find("vbmeta_a").is_none());
        assert!(modified.find("vbmeta").is_none());
        assert!(modified.find(&format!("pvmfw{}", slot.suffix())).is_none());
        for name in [
            "vbmeta_b".into(),
            "pvmfw".into(),
            format!("pvmfw{}", slot.other().suffix()),
        ] {
            assert_eq!(modified.find(&name), gpt.find(&name));
        }
        for entry in modified.entries() {
            assert_eq!(gpt.find(&entry.name), Some(entry));
        }
    }
}

#[test]
fn named_unused_end_marker_from_a_real_scan_is_cross_checked() {
    let path = std::env::var_os("TB520_GPT_SCAN_DUMP")
        .map(PathBuf::from)
        .unwrap_or_else(|| fixture("gpt_main4.bin"));
    let Ok(dump) = std::fs::read(path) else {
        eprintln!("skipped: local GPT scan fixture not present");
        return;
    };
    let table = lapiswitch_core::storage::validate_gpt(&dump).unwrap();
    let header = table.header();
    let start = header.entries_lba as usize * SECTOR_SIZE;
    let mut checked = 0;
    for raw in
        dump[start..start + header.num_entries * header.entry_size].chunks_exact(header.entry_size)
    {
        let name = String::from_utf16_lossy(
            &raw[56..128]
                .chunks_exact(2)
                .map(|unit| u16::from_le_bytes(unit.try_into().unwrap()))
                .take_while(|unit| *unit != 0)
                .collect::<Vec<_>>(),
        );
        if name != "last_parti" || raw[..16].iter().any(|byte| *byte != 0) {
            continue;
        }
        let first = u64::from_le_bytes(raw[32..40].try_into().unwrap());
        let last = u64::from_le_bytes(raw[40..48].try_into().unwrap());
        assert!(table.find("last_parti").is_none());
        lapiswitch_core::storage::match_gpt_partition(
            &table,
            &lapiswitch_core::storage::Partition {
                name,
                lun: 4,
                start_sector: first,
                num_sectors: last - first + 1,
            },
        )
        .unwrap();
        checked += 1;
    }
    assert_eq!(checked, 1, "expected the reported named-unused end marker");
}
