#![allow(dead_code)] // Shared fixtures expose paths used by different integration-test binaries.
//! In-memory UFS device: partial writes, corrupt readbacks, backup tampering
//! and recovery faults. No USB APIs or real device dumps are used.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use lapiswitch_core::avb_info::{AvbSummary, KeyClass};
use lapiswitch_core::backup::{BackupBuilder, TransactionStage, VerifiedBackup};
use lapiswitch_core::firmware::FirmwareReport;
use lapiswitch_core::gpt::SECTOR_SIZE;
use lapiswitch_core::prepare::{Connection, PrepareReport};
use lapiswitch_core::region::Region;
use lapiswitch_core::rollback::{DeviceFloors, RollbackIndices};
use lapiswitch_core::slot::Slot;
use lapiswitch_core::stage1::{self, ResetMode, StageIo};
use lapiswitch_core::storage::Partition;

pub(crate) fn part(name: &str, lun: u8, start_sector: u64, num_sectors: u64) -> Partition {
    Partition {
        name: name.into(),
        lun,
        start_sector,
        num_sectors,
    }
}

pub(crate) fn gpt(parts: &[Partition]) -> Vec<u8> {
    let mut data = vec![0u8; 6 * SECTOR_SIZE];
    data[510..512].copy_from_slice(&[0x55, 0xaa]);
    for (i, p) in parts.iter().filter(|part| part.lun == 4).enumerate() {
        let at = 2 * SECTOR_SIZE + i * 128;
        data[at..at + 16].fill(0x11);
        data[at + 16..at + 32].fill(i as u8 + 1);
        data[at + 32..at + 40].copy_from_slice(&p.start_sector.to_le_bytes());
        data[at + 40..at + 48].copy_from_slice(&(p.start_sector + p.num_sectors - 1).to_le_bytes());
        for (j, code) in p.name.encode_utf16().enumerate() {
            data[at + 56 + j * 2..at + 58 + j * 2].copy_from_slice(&code.to_le_bytes());
        }
    }
    let h = SECTOR_SIZE;
    data[h..h + 8].copy_from_slice(b"EFI PART");
    data[h + 8..h + 12].copy_from_slice(&0x10000u32.to_le_bytes());
    data[h + 12..h + 16].copy_from_slice(&92u32.to_le_bytes());
    data[h + 24..h + 32].copy_from_slice(&1u64.to_le_bytes());
    data[h + 32..h + 40].copy_from_slice(&2000u64.to_le_bytes());
    data[h + 40..h + 48].copy_from_slice(&6u64.to_le_bytes());
    data[h + 48..h + 56].copy_from_slice(&1999u64.to_le_bytes());
    data[h + 56..h + 72].fill(0x42);
    data[h + 72..h + 80].copy_from_slice(&2u64.to_le_bytes());
    data[h + 80..h + 84].copy_from_slice(&96u32.to_le_bytes());
    data[h + 84..h + 88].copy_from_slice(&128u32.to_le_bytes());
    let crc = crc32fast::hash(&data[2 * SECTOR_SIZE..5 * SECTOR_SIZE]);
    data[h + 88..h + 92].copy_from_slice(&crc.to_le_bytes());
    refresh_header(&mut data);
    data
}

pub(crate) fn refresh_header(data: &mut [u8]) {
    data[SECTOR_SIZE + 16..SECTOR_SIZE + 20].fill(0);
    let crc = crc32fast::hash(&data[SECTOR_SIZE..SECTOR_SIZE + 92]);
    data[SECTOR_SIZE + 16..SECTOR_SIZE + 20].copy_from_slice(&crc.to_le_bytes());
}

pub(crate) fn firmware() -> FirmwareReport {
    FirmwareReport {
        image_dir: "firmware/image".into(),
        loader: "loader.melf".into(),
        region: Region::Row,
        fingerprint: "qti/TB520FU/lapis:16/test_ROW:user/release-keys".into(),
        key_class: KeyClass::Lenovo,
        indices: RollbackIndices {
            boot: 20,
            vbmeta_system: 20,
        },
        raw_xmls: vec![],
        patch_xmls: vec![],
        decrypted: 0,
        missing_images: vec![],
        lun4_gpt_in_package: true,
    }
}

pub(crate) fn prepared(slot: Slot, mode: Connection) -> PrepareReport {
    PrepareReport {
        started_in: mode,
        slot,
        slot_estimated: false,
        floors: DeviceFloors::None,
        device_key: None,
        getvar_all: (mode != Connection::Edl).then(|| "modelname:TB520FU\nsynthetic getvar".into()),
    }
}

pub(crate) struct Fake {
    pub(crate) disk: Vec<Vec<u8>>,
    pub(crate) snapshot: Vec<Partition>,
    pub(crate) original_gpt: Vec<u8>,
    pub(crate) writes: Vec<Partition>,
    pub(crate) fail_writes: HashSet<usize>,
    pub(crate) corrupt_writes: HashSet<usize>,
    pub(crate) fail_dump: Option<String>,
    pub(crate) truncated_dump: bool,
    pub(crate) fail_absent: bool,
    pub(crate) bad_model: bool,
    pub(crate) fingerprints: std::collections::HashMap<String, String>,
    pub(crate) moved: bool,
    pub(crate) resets: Vec<ResetMode>,
    pub(crate) fail_reset: bool,
    pub(crate) journal: Option<PathBuf>,
    pub(crate) journal_stages: Vec<TransactionStage>,
    pub(crate) tamper_on_write: Option<PathBuf>,
}

impl Fake {
    pub(crate) fn new(boot_sectors: u64, proinfo: bool) -> Self {
        let mut snapshot = vec![
            part("boot_a", 4, 100, boot_sectors),
            part("vbmeta_a", 4, 700, 1),
            part("pvmfw_a", 4, 701, 1),
            part("boot_b", 4, 800, boot_sectors),
            part("vbmeta_b", 4, 1400, 1),
            part("pvmfw_b", 4, 1401, 1),
            part("devinfo", 4, 1500, 2),
            part("persist", 0, 50, 2),
        ];
        if proinfo {
            snapshot.push(part("proinfo", 0, 60, 2));
        }
        let original_gpt = gpt(&snapshot);
        let mut disk = vec![vec![0x33; 100 * SECTOR_SIZE]; 6];
        disk[4] = vec![0x77; 2001 * SECTOR_SIZE];
        disk[4][..original_gpt.len()].copy_from_slice(&original_gpt);
        Self {
            disk,
            snapshot,
            original_gpt,
            writes: vec![],
            fail_writes: HashSet::new(),
            corrupt_writes: HashSet::new(),
            fail_dump: None,
            truncated_dump: false,
            fail_absent: false,
            bad_model: false,
            fingerprints: Default::default(),
            moved: false,
            resets: vec![],
            fail_reset: false,
            journal: None,
            journal_stages: vec![],
            tamper_on_write: None,
        }
    }

    pub(crate) fn backup(&mut self, output: &Path, slot: Slot) -> VerifiedBackup {
        let builder = BackupBuilder::create(output).unwrap();
        self.journal = Some(builder.path().to_path_buf());
        stage1::take_backup(
            self,
            &prepared(slot, Connection::Fastboot),
            &firmware(),
            builder,
            &mut vec![],
        )
        .unwrap()
    }

    pub(crate) fn bytes(&self, name: &str) -> &[u8] {
        let p = self.snapshot.iter().find(|part| part.name == name).unwrap();
        &self.disk[p.lun as usize][p.start_sector as usize * SECTOR_SIZE
            ..(p.start_sector + p.num_sectors) as usize * SECTOR_SIZE]
    }

    pub(crate) fn assert_original(&self, slot: Slot) {
        assert_eq!(
            &self.disk[4][..self.original_gpt.len()],
            self.original_gpt.as_slice()
        );
        assert!(
            self.bytes(&format!("boot{}", slot.suffix()))
                .iter()
                .all(|byte| *byte == 0x77)
        );
    }
}

impl StageIo for Fake {
    fn ensure_single_target(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn read_avb(
        &mut self,
        partition: &str,
        _: &mut Vec<String>,
    ) -> Result<Option<AvbSummary>, String> {
        Ok(Some(AvbSummary {
            rollback_index: 10,
            rollback_index_location: 2,
            public_key_sha1: None,
            props: vec![(
                "com.android.build.system.fingerprint".into(),
                self.fingerprints
                    .get(partition)
                    .cloned()
                    .unwrap_or_else(|| {
                        format!(
                            "qti/{0}/{0}:16/BUILD/ZUI_17.5.10.309_260528_ROW:user/release-keys",
                            if self.bad_model { "TB323FU" } else { "TB520FU" }
                        )
                    })
                    .into_bytes(),
            )],
        }))
    }
    fn scan(&mut self, _: &mut Vec<String>) -> Result<Vec<Partition>, String> {
        Ok(self.snapshot.clone())
    }
    fn validate_snapshot(&mut self, _: &[Partition]) -> Result<(), String> {
        if self.moved {
            Err("layout changed".into())
        } else {
            Ok(())
        }
    }
    fn confirm_proinfo_absent(&mut self, _: &mut Vec<String>) -> Result<(), String> {
        if self.fail_absent {
            Err("proinfo transfer timeout".into())
        } else {
            Ok(())
        }
    }
    fn dump_at(&mut self, p: &Partition, output: &Path, _: &mut Vec<String>) -> Result<(), String> {
        if self.fail_dump.as_deref() == Some(p.name.as_str()) {
            return Err("read failed".into());
        }
        let start = p.start_sector as usize * SECTOR_SIZE;
        let mut bytes = self.disk[p.lun as usize][start..start + p.size_bytes()? as usize].to_vec();
        if self.truncated_dump {
            bytes.pop();
        }
        std::fs::write(output, bytes).map_err(|error| error.to_string())
    }
    fn write_at(&mut self, p: &Partition, image: &Path, _: &mut Vec<String>) -> Result<(), String> {
        if let Some(dir) = &self.journal {
            self.journal_stages.push(
                VerifiedBackup::open(dir)
                    .and_then(|backup| backup.transaction_stage())
                    .unwrap_or(TransactionStage::RecoveryRequired),
            );
        }
        let bytes = std::fs::read(image).map_err(|error| error.to_string())?;
        assert!(bytes.len() as u64 <= p.size_bytes()?);
        self.writes.push(p.clone());
        let n = self.writes.len();
        let start = p.start_sector as usize * SECTOR_SIZE;
        let count = if self.fail_writes.contains(&n) {
            bytes.len() / 2
        } else {
            bytes.len()
        };
        self.disk[p.lun as usize][start..start + count].copy_from_slice(&bytes[..count]);
        if self.corrupt_writes.contains(&n) {
            self.disk[p.lun as usize][start] ^= 1;
        }
        if let Some(path) = self.tamper_on_write.take() {
            std::fs::write(path, b"backup changed after first write").unwrap();
        }
        if self.fail_writes.contains(&n) {
            Err("partial write failure".into())
        } else {
            Ok(())
        }
    }
    fn reset(&mut self, mode: ResetMode, _: &mut Vec<String>) -> Result<(), String> {
        self.resets.push(mode);
        if self.fail_reset {
            Err("reset failed".into())
        } else {
            Ok(())
        }
    }
}
