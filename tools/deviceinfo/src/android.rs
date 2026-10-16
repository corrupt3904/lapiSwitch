//! Android ARM64 transport. Unsafe code is limited to the OS and QSEECom ABI.
use lapiswitch_deviceinfo::{
    DiskEvidence, Options, Result, State, Transport,
    abi::{self, DmaAllocation, FdInfo},
    execute, state_from_bytes,
};
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    fs::{File, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    path::Path,
    ptr::{self, NonNull},
};

const HEAP: &str = "/dev/dma_heap/qcom,qseecom-ta";
const LIBRARY: &CStr = c"/vendor/lib64/libQSEEComAPI.so";
fn os_error(context: &str) -> String {
    format!("{context}: {}", io::Error::last_os_error())
}

struct ProcessLock(File);
impl ProcessLock {
    fn acquire() -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open("/tmp/lapiswitch-deviceinfo.lock")
            .map_err(|e| e.to_string())?;
        // SAFETY: file owns an open FD and flock does not retain a Rust pointer.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(os_error("DeviceInfo helper lock is held"));
        }
        Ok(Self(file))
    }
}
impl Drop for ProcessLock {
    fn drop(&mut self) {
        // SAFETY: the lock FD stays open until after this destructor.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut length = 0_u64;
    // SAFETY: the ioctl writes exactly one u64 to a valid local variable.
    let block = unsafe { libc::ioctl(file.as_raw_fd(), abi::BLOCK_SIZE64 as _, &mut length) } == 0;
    if !block || length == 0 {
        length = file.metadata().map_err(|e| e.to_string())?.len();
    }
    if length == 0 || length > 64 * 1024 * 1024 {
        return Err("Unreasonable or unknown TA/input size".into());
    }
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

struct DmaBuffer {
    fd: OwnedFd,
    address: NonNull<u8>,
    length: usize,
}
impl DmaBuffer {
    fn new() -> Result<Self> {
        // SAFETY: sysconf has no pointer arguments.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page <= 0 || !(page as usize).is_power_of_two() {
            return Err("Invalid page size".into());
        }
        let length = abi::SHARED_SIZE.div_ceil(page as usize) * page as usize;
        let heap = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(HEAP)
            .map_err(|e| format!("{HEAP}: {e}"))?;
        let mut allocation = DmaAllocation {
            len: length as u64,
            fd_flags: (libc::O_RDWR | libc::O_CLOEXEC) as u32,
            ..Default::default()
        };
        // SAFETY: DmaAllocation has the asserted ARM64 Linux UAPI layout (24 bytes).
        if unsafe { libc::ioctl(heap.as_raw_fd(), abi::DMA_HEAP_ALLOC as _, &mut allocation) } < 0 {
            return Err(os_error("DMA_HEAP_IOCTL_ALLOC"));
        }
        let raw_fd = i32::try_from(allocation.fd).map_err(|_| "Invalid DMA buffer FD")?;
        // SAFETY: the successful allocation ioctl returned a new owned FD.
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        // SAFETY: the FD owns a DMA buffer of at least length bytes; mmap returns its mapping.
        let address = unsafe {
            libc::mmap(
                ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(os_error("mmap qseecom-ta DMA buffer"));
        }
        let Some(address) = NonNull::new(address.cast::<u8>()) else {
            // SAFETY: a non-failed mmap result is a mapping, including at zero.
            unsafe {
                libc::munmap(address, length);
            }
            return Err("DMA mapping returned a null address".into());
        };
        let buffer = Self {
            fd,
            address,
            length,
        };
        buffer.sync(3)?;
        // SAFETY: mapping is writable, uniquely owned, and length bytes long; no TA request is in flight.
        unsafe {
            ptr::write_bytes(buffer.address.as_ptr(), 0, length);
        }
        buffer.sync(3 | 4)?;
        Ok(buffer)
    }
    fn sync(&self, flags: u64) -> Result<()> {
        let mut flags = flags;
        // SAFETY: DMA_BUF_IOCTL_SYNC takes the address of one u64 flag field.
        if unsafe { libc::ioctl(self.fd.as_raw_fd(), abi::DMA_BUF_SYNC as _, &mut flags) } < 0 {
            Err(os_error("DMA_BUF_IOCTL_SYNC"))
        } else {
            Ok(())
        }
    }
    fn fill(&mut self, bytes: &State) -> Result<()> {
        self.sync(3)?;
        // SAFETY: source and destination are distinct, 4096 bytes long, and CPU access is synchronized.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), self.address.as_ptr(), bytes.len());
        }
        self.sync(3 | 4)
    }
    fn copy_state(&self) -> Result<State> {
        self.sync(1)?;
        let mut state = [0; abi::SHARED_SIZE];
        // SAFETY: mapping is at least 4096 bytes and no QSEE call is in flight during the copy.
        unsafe {
            ptr::copy_nonoverlapping(self.address.as_ptr(), state.as_mut_ptr(), state.len());
        }
        self.sync(1 | 4)?;
        Ok(state)
    }
}
impl Drop for DmaBuffer {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns exactly this mmap mapping.
        unsafe {
            libc::munmap(self.address.as_ptr().cast(), self.length);
        }
    }
}

#[repr(C)]
struct QseeHandle {
    _opaque: [u8; 0],
}
type Start = unsafe extern "C" fn(*mut *mut QseeHandle, *const c_char, *mut u8, u32, u32) -> c_int;
type Stop = unsafe extern "C" fn(*mut *mut QseeHandle) -> c_int;
type Send = unsafe extern "C" fn(*mut QseeHandle, *mut c_void, u32, *mut c_void, u32) -> c_int;
type Modified =
    unsafe extern "C" fn(*mut QseeHandle, *mut c_void, u32, *mut c_void, u32, *mut FdInfo) -> c_int;
struct Library(NonNull<c_void>);
impl Library {
    fn load() -> Result<Self> {
        // SAFETY: a static NUL-terminated path and valid dlopen flags.
        NonNull::new(unsafe { libc::dlopen(LIBRARY.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) })
            .map(Self)
            .ok_or_else(|| "Cannot load libQSEEComAPI.so".into())
    }
    fn symbol(&self, name: &CStr) -> Result<*mut c_void> {
        // SAFETY: this object holds an open library and name is NUL-terminated.
        NonNull::new(unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) })
            .map(NonNull::as_ptr)
            .ok_or_else(|| format!("Missing QSEECom symbol: {}", name.to_string_lossy()))
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: library outlives the session and all calls using its function pointers.
        unsafe {
            libc::dlclose(self.0.as_ptr());
        }
    }
}
struct Qsee {
    _library: Library,
    start_fn: Start,
    stop_fn: Stop,
    send_fn: Send,
    modified_fn: Modified,
    handle: *mut QseeHandle,
    ta: Vec<u8>,
    write_buffer: Option<DmaBuffer>,
    write_attempted: bool,
}
impl Qsee {
    fn load(ta: Vec<u8>) -> Result<Self> {
        let library = Library::load()?;
        // SAFETY: these symbols use the exact signatures of the validated 64-bit QSEECom API.
        let start_fn = unsafe {
            std::mem::transmute::<*mut c_void, Start>(library.symbol(c"QSEECom_start_app_V2")?)
        };
        // SAFETY: QSEECom_shutdown_app's ABI accepts the address of its opaque handle pointer.
        let stop_fn = unsafe {
            std::mem::transmute::<*mut c_void, Stop>(library.symbol(c"QSEECom_shutdown_app")?)
        };
        // SAFETY: QSEECom_send_cmd uses pointer/length pairs with u32 lengths.
        let send_fn = unsafe {
            std::mem::transmute::<*mut c_void, Send>(library.symbol(c"QSEECom_send_cmd")?)
        };
        // SAFETY: the modified-command ABI additionally takes the asserted 32-byte FdInfo.
        let modified_fn = unsafe {
            std::mem::transmute::<*mut c_void, Modified>(
                library.symbol(c"QSEECom_send_modified_cmd_64")?,
            )
        };
        Ok(Self {
            _library: library,
            start_fn,
            stop_fn,
            send_fn,
            modified_fn,
            handle: ptr::null_mut(),
            ta,
            write_buffer: None,
            write_attempted: false,
        })
    }
    fn request(&self, command: u32, buffer: &DmaBuffer) -> Result<()> {
        if self.handle.is_null() {
            return Err("QSEECom session is not open".into());
        }
        let mut request = abi::state_request(command);
        let mut response = abi::initial_state_response();
        let mut info = FdInfo::for_buffer(buffer.fd.as_raw_fd());
        // SAFETY: all buffers are live for this synchronous call; lengths and packed pointer offset match the TA ABI.
        let result = unsafe {
            (self.modified_fn)(
                self.handle,
                request.as_mut_ptr().cast(),
                request.len() as u32,
                response.as_mut_ptr().cast(),
                response.len() as u32,
                &mut info,
            )
        };
        eprint!("cmd=0x{command:x} ");
        if abi::state_response_ok(result, response) {
            Ok(())
        } else {
            Err(format!("QSEECom command 0x{command:x} failed"))
        }
    }
    fn stop(&mut self) -> Result<()> {
        if self.handle.is_null() {
            return Ok(());
        }
        // SAFETY: handle was returned by start_fn from the still-loaded library.
        let result = unsafe { (self.stop_fn)(&mut self.handle) };
        eprintln!("QSEECom_shutdown_app={result}");
        if result == 0 {
            self.handle = ptr::null_mut();
            Ok(())
        } else {
            Err("QSEECom shutdown failed".into())
        }
    }
}
impl Transport for Qsee {
    fn start(&mut self) -> Result<()> {
        if !self.handle.is_null() {
            return Err("Session is already open".into());
        }
        // SAFETY: TA buffer lives for the session; lengths were bounded when read. API writes its opaque handle.
        let result = unsafe {
            (self.start_fn)(
                &mut self.handle,
                c"keymaster".as_ptr(),
                self.ta.as_mut_ptr(),
                self.ta.len() as u32,
                abi::SHARED_SIZE as u32,
            )
        };
        if result != 0 || self.handle.is_null() {
            return Err("QSEECom_start_app_V2 failed".into());
        }
        let mut request = abi::GET_VERSION.to_le_bytes();
        let mut response = [0; 20];
        // SAFETY: these 4-byte/20-byte packed messages and their lifetimes match GET_VERSION exactly.
        let result = unsafe {
            (self.send_fn)(
                self.handle,
                request.as_mut_ptr().cast(),
                request.len() as u32,
                response.as_mut_ptr().cast(),
                response.len() as u32,
            )
        };
        if abi::version_ok(result, response) {
            Ok(())
        } else {
            Err("Unsupported Keymaster version; refusing DeviceInfo I/O".into())
        }
    }
    fn read(&mut self) -> Result<State> {
        let buffer = DmaBuffer::new()?;
        self.request(abi::READ_STATE, &buffer)?;
        buffer.copy_state()
    }
    fn prepare_write(&mut self, planned: &State) -> Result<()> {
        if self.write_attempted {
            return Err("A write has already been attempted; no retry is permitted".into());
        }
        let mut buffer = DmaBuffer::new()?;
        buffer.fill(planned)?;
        self.write_buffer = Some(buffer);
        Ok(())
    }
    fn write_once(&mut self) -> Result<()> {
        if self.write_attempted {
            return Err("A write has already been attempted; no retry is permitted".into());
        }
        // Keep the original write DMA allocation alive through both readbacks.
        self.write_attempted = true;
        let buffer = self
            .write_buffer
            .as_ref()
            .ok_or("No prepared write buffer")?;
        self.request(abi::WRITE_STATE, buffer)
    }
    fn reopen(&mut self) -> Result<()> {
        self.stop()?;
        self.start()
    }
}
impl Drop for Qsee {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub fn run(options: Options) -> u8 {
    let _lock = match ProcessLock::acquire() {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("{e}");
            return 3;
        }
    };
    let run = (|| -> Result<u8> {
        let expected = options
            .expected
            .as_deref()
            .map(read_file)
            .transpose()?
            .map(state_from_bytes)
            .transpose()?;
        let planned = options
            .planned
            .as_deref()
            .map(read_file)
            .transpose()?
            .map(state_from_bytes)
            .transpose()?;
        let ta = read_file(&options.ta)?;
        let mut transport = Qsee::load(ta)?;
        let mut evidence = DiskEvidence(options.prefix.clone());
        match execute(
            &options,
            expected.as_ref(),
            planned.as_ref(),
            &mut transport,
            &mut evidence,
        ) {
            Ok(()) => Ok(0),
            Err(failure) => {
                eprintln!("{}", failure.message);
                Ok(failure.exit)
            }
        }
    })();
    match run {
        Ok(exit) => exit,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}
