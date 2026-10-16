//! The exact packed KeyMint messages and Linux ARM64 UAPI from the validated transport.
pub const SHARED_SIZE: usize = 0x1000;
pub const GET_VERSION: u32 = 0x200;
pub const READ_STATE: u32 = 0x202;
pub const WRITE_STATE: u32 = 0x203;
pub const SHARED_POINTER_OFFSET: u32 = 4;
pub const DMA_HEAP_ALLOC: u32 = 0xC018_4800;
pub const DMA_BUF_SYNC: u32 = 0x4008_6200;
pub const BLOCK_SIZE64: u32 = 0x8008_1272;

#[repr(C)]
#[derive(Debug, Default)]
pub struct DmaAllocation {
    pub len: u64,
    pub fd: u32,
    pub fd_flags: u32,
    pub heap_flags: u64,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FdData {
    pub fd: i32,
    pub cmd_buf_offset: u32,
}
#[repr(C)]
pub struct FdInfo {
    pub data: [FdData; 4],
}
impl FdInfo {
    pub fn for_buffer(fd: i32) -> Self {
        let mut info = Self {
            data: [FdData {
                fd: -1,
                cmd_buf_offset: 0,
            }; 4],
        };
        info.data[0] = FdData {
            fd,
            cmd_buf_offset: SHARED_POINTER_OFFSET,
        };
        info
    }
}
const _: () = assert!(size_of::<DmaAllocation>() == 24);
const _: () = assert!(std::mem::offset_of!(DmaAllocation, heap_flags) == 16);
const _: () = assert!(size_of::<FdInfo>() == 32);

pub fn state_request(command: u32) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..4].copy_from_slice(&command.to_le_bytes());
    // Bytes 4..12 are the packed pointer, patched by QSEECom using the DMA FD.
    bytes[12..].copy_from_slice(&(SHARED_SIZE as u32).to_le_bytes());
    bytes
}
pub fn initial_state_response() -> [u8; 12] {
    let mut bytes = [0; 12];
    bytes[8..].copy_from_slice(&i32::MIN.to_le_bytes());
    bytes
}
pub fn state_response_ok(transport: i32, response: [u8; 12]) -> bool {
    let status = i32::from_le_bytes(response[..4].try_into().unwrap());
    let unknown = u32::from_le_bytes(response[4..8].try_into().unwrap());
    let result = i32::from_le_bytes(response[8..].try_into().unwrap());
    eprintln!("transport={transport} status={status} unknown={unknown} rsp_status={result}");
    transport == 0 && status == 0 && result == 0
}
pub fn version_ok(transport: i32, response: [u8; 20]) -> bool {
    let status = i32::from_le_bytes(response[..4].try_into().unwrap());
    let fields: [u32; 4] = std::array::from_fn(|i| {
        u32::from_le_bytes(response[4 + i * 4..8 + i * 4].try_into().unwrap())
    });
    eprintln!(
        "GET_VERSION transport={transport} status={status} version={}.{} app={}.{}",
        fields[0], fields[1], fields[2], fields[3]
    );
    transport == 0 && status == 0 && fields == [5, 3, 5, 3000]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_wire_matches_the_original_arm64_abi() {
        assert_eq!(
            state_request(WRITE_STATE),
            [3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 16, 0, 0]
        );
        let fd = FdInfo::for_buffer(17);
        assert_eq!(fd.data[0].fd, 17);
        assert_eq!(fd.data[0].cmd_buf_offset, 4);
        assert!(
            fd.data[1..]
                .iter()
                .all(|item| item.fd == -1 && item.cmd_buf_offset == 0)
        );
        assert!(!state_response_ok(0, initial_state_response()));
        assert!(state_response_ok(0, [0; 12]));
        assert!(!state_response_ok(-1, [0; 12]));
    }
    #[test]
    fn every_version_field_and_status_is_checked() {
        let mut good = [0; 20];
        for (i, value) in [5_u32, 3, 5, 3000].iter().enumerate() {
            good[4 + i * 4..8 + i * 4].copy_from_slice(&value.to_le_bytes());
        }
        assert!(version_ok(0, good));
        assert!(!version_ok(1, good));
        for offset in [0, 4, 8, 12, 16] {
            let mut bad = good;
            bad[offset] ^= 1;
            assert!(!version_ok(0, bad));
        }
    }
}
