//! M5 adapter over the same EdlSession returned by M4. Vendor code is unchanged.

use std::path::Path;

use ltbox_device::edl::{EdlError, EdlSession, GptPartitionInfo};

use super::{ResetMode, StageIo};
use crate::avb_info::AvbSummary;
use crate::prepare::{DeviceIo, native::NativeDevice};
use crate::storage::Partition;

pub struct NativeStage {
    device: NativeDevice,
    session: EdlSession,
}

impl NativeStage {
    pub fn new(device: NativeDevice, session: EdlSession) -> Self {
        Self { device, session }
    }

    pub(crate) fn into_parts(self) -> (NativeDevice, EdlSession) {
        (self.device, self.session)
    }

    pub(crate) fn session_mut(&mut self) -> &mut EdlSession {
        &mut self.session
    }
}

impl StageIo for NativeStage {
    fn ensure_single_target(&mut self) -> Result<(), String> {
        self.device.ensure_single_target()
    }

    fn read_avb(
        &mut self,
        partition: &str,
        log: &mut Vec<String>,
    ) -> Result<Option<AvbSummary>, String> {
        self.device.read_avb(&mut self.session, partition, log)
    }

    fn scan(&mut self, log: &mut Vec<String>) -> Result<Vec<Partition>, String> {
        self.session
            .scan_partitions(0..=5, log)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|part| {
                let row = Partition {
                    name: part.name,
                    lun: part.lun,
                    start_sector: part.start_sector,
                    num_sectors: part.num_sectors,
                };
                if row.size_bytes()? != part.size_bytes {
                    return Err("기기의 논리 섹터 크기가 4096바이트가 아닙니다.".into());
                }
                Ok(row)
            })
            .collect()
    }

    fn validate_snapshot(&mut self, snapshot: &[Partition]) -> Result<(), String> {
        let rows = snapshot
            .iter()
            .map(|part| {
                Ok(GptPartitionInfo {
                    name: part.name.clone(),
                    lun: part.lun,
                    start_sector: part.start_sector,
                    num_sectors: part.num_sectors,
                    size_bytes: part.size_bytes()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.session
            .validate_partition_snapshot(&rows)
            .map_err(|error| error.to_string())
    }

    fn confirm_proinfo_absent(&mut self, log: &mut Vec<String>) -> Result<(), String> {
        let work =
            tempfile::tempdir_in(self.device.scratch_dir()).map_err(|error| error.to_string())?;
        match self
            .session
            .dump_partition("proinfo", &work.path().join("proinfo.img"), 0, 0, log)
        {
            Err(EdlError::PartitionNotFound(_)) => Ok(()),
            Ok(()) => {
                Err("GPT 스캔에 없던 proinfo가 발견되었습니다. 스캔부터 다시 진행하세요.".into())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    fn dump_at(
        &mut self,
        part: &Partition,
        output: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        part.size_bytes()?;
        let sectors = usize::try_from(part.num_sectors).map_err(|error| error.to_string())?;
        self.session
            .dump_partition_at(
                &part.name,
                output,
                part.lun,
                part.start_sector,
                sectors,
                log,
            )
            .map_err(|error| error.to_string())
    }

    fn write_at(
        &mut self,
        part: &Partition,
        image: &Path,
        log: &mut Vec<String>,
    ) -> Result<(), String> {
        part.size_bytes()?;
        self.session
            .flash_partition_at(
                &part.name,
                image,
                part.lun,
                &part.start_sector.to_string(),
                part.num_sectors,
                log,
            )
            .map_err(|error| error.to_string())
    }

    fn reset(&mut self, mode: ResetMode, log: &mut Vec<String>) -> Result<(), String> {
        match mode {
            ResetMode::System => self.session.reset(log),
            ResetMode::Edl => self.session.reset_to_edl(log),
        }
        .map_err(|error| error.to_string())
    }
}
