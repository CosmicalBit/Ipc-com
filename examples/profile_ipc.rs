//! Two-process IPC workload for `scripts/perf.sh`.
//! The watcher reads the shared header's generation word (after its 4-byte lock)
//! so a write cannot be missed between a read and arming FUTEX_WAIT.
use std::{
    borrow::Cow,
    env,
    error::Error,
    ffi::CString,
    io::{self, Read, Write},
    process::{Child, Command, Stdio},
    ptr::NonNull,
    sync::atomic::{AtomicU32, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use ipc_com::{ReadWrite, Result as IpcResult, SharedData, SharedMemoryOptions, SharedValue};

const SHUTDOWN: u64 = u64::MAX;
const DEFAULT_PAYLOAD: usize = 64;
const DEFAULT_ITERATIONS: u64 = 100_000;
const GENERATION_OFFSET: usize = size_of::<AtomicU32>();
const WATCHERS_OFFSET: usize = GENERATION_OFFSET + size_of::<AtomicU32>();
const WATCHER_MAPPING_SIZE: usize = WATCHERS_OFFSET + size_of::<AtomicU32>();
type WorkResult<T> = Result<T, Box<dyn Error>>;

struct Packet(Vec<u8>);

impl Packet {
    fn new(size: usize) -> Self {
        Self(vec![0xa5; size])
    }

    fn sequence(&self) -> u64 {
        u64::from_be_bytes(self.0[..8].try_into().expect("payload has room for sequence"))
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.0[..8].copy_from_slice(&sequence.to_be_bytes());
    }
}

impl SharedData for Packet {
    fn as_bytes(&self) -> IpcResult<Cow<'_, [u8]>> {
        Ok(Cow::Borrowed(&self.0))
    }

    fn from_bytes(bytes: &[u8]) -> IpcResult<Self> {
        Ok(Self(bytes.to_vec()))
    }
}

struct ChangeWatcher {
    mapping: NonNull<libc::c_void>,
    generation: NonNull<AtomicU32>,
    watchers: NonNull<AtomicU32>,
    observed: u32,
}

impl ChangeWatcher {
    fn open(name: &str) -> io::Result<Self> {
        let name = CString::new(format!("/{name}"))?;
        let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mapping = unsafe { libc::mmap(std::ptr::null_mut(), WATCHER_MAPPING_SIZE, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
        unsafe { libc::close(fd) };
        if mapping == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let mapping = NonNull::new(mapping).expect("mmap returned a non-null address");
        // The header begins with lock, generation, and watcher count.
        let generation = unsafe { NonNull::new_unchecked(mapping.as_ptr().cast::<u8>().add(GENERATION_OFFSET).cast::<AtomicU32>()) };
        let watchers = unsafe { NonNull::new_unchecked(mapping.as_ptr().cast::<u8>().add(WATCHERS_OFFSET).cast::<AtomicU32>()) };
        let observed = unsafe { generation.as_ref().load(Ordering::Acquire) };
        Ok(Self {
            mapping,
            generation,
            watchers,
            observed,
        })
    }

    fn wait(&mut self) -> io::Result<()> {
        loop {
            let generation = unsafe { self.generation.as_ref() };
            let current = generation.load(Ordering::Acquire);
            if current != self.observed {
                self.observed = current;
                return Ok(());
            }
            let timeout = libc::timespec { tv_sec: 10, tv_nsec: 0 };
            unsafe { self.watchers.as_ref().fetch_add(1, Ordering::SeqCst) };
            let result = unsafe { libc::syscall(libc::SYS_futex, generation.as_ptr(), libc::FUTEX_WAIT, self.observed, &timeout) };
            unsafe { self.watchers.as_ref().fetch_sub(1, Ordering::SeqCst) };
            if result == -1 {
                let error = io::Error::last_os_error();
                if !matches!(error.raw_os_error(), Some(libc::EAGAIN | libc::EINTR)) {
                    return Err(error);
                }
            }
        }
    }
}

impl Drop for ChangeWatcher {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.mapping.as_ptr(), WATCHER_MAPPING_SIZE) };
    }
}

struct Exchange {
    request: SharedValue<Packet, ReadWrite>,
    response: SharedValue<Packet>,
    response_watcher: ChangeWatcher,
    child: Child,
}

impl Exchange {
    fn start(payload: usize) -> WorkResult<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let prefix = format!("ipc_com_profile_{}_{}", std::process::id(), nonce);
        let request_name = format!("{prefix}_request");
        let response_name = format!("{prefix}_response");
        let request = SharedMemoryOptions::new().name(&request_name).with_data(Packet::new(payload)).to_mutable().create()?;
        let response = SharedMemoryOptions::new().name(&response_name).with_data(Packet::new(payload)).create()?;
        let response_watcher = ChangeWatcher::open(&response_name)?;
        let mut child = Command::new(env::current_exe()?)
            .args(["--worker", &request_name, &response_name])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?;
        let mut ready = [0];
        child.stdout.take().expect("child stdout was piped").read_exact(&mut ready)?;
        if ready != [1] {
            return Err("worker did not become ready".into());
        }
        Ok(Self {
            request,
            response,
            response_watcher,
            child,
        })
    }

    fn run(&mut self, payload: usize, iterations: u64) -> WorkResult<()> {
        let mut packet = Packet::new(payload);
        for sequence in 1..=iterations {
            packet.set_sequence(sequence);
            self.request.write(&packet)?;
            self.response_watcher.wait()?;
            let reply = self.response.read()?;
            if reply.sequence() != sequence || reply.0 != packet.0 {
                return Err(format!("incorrect response at sequence {sequence}").into());
            }
        }
        packet.set_sequence(SHUTDOWN);
        self.request.write(&packet)?;
        if !self.child.wait()?.success() {
            return Err("worker failed".into());
        }
        Ok(())
    }
}

impl Drop for Exchange {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn worker(request_name: &str, response_name: &str) -> WorkResult<()> {
    let mut request = SharedValue::<Packet>::open(request_name)?;
    let mut response = SharedValue::<Packet>::open(response_name)?.into_mutable();
    let mut request_watcher = ChangeWatcher::open(request_name)?;
    io::stdout().write_all(&[1])?;
    io::stdout().flush()?;
    let mut last = 0;
    loop {
        request_watcher.wait()?;
        let packet = request.read()?;
        let sequence = packet.sequence();
        if sequence == SHUTDOWN {
            return Ok(());
        }
        if sequence != last + 1 {
            return Err(format!("incorrect request sequence: expected {}, got {sequence}", last + 1).into());
        }
        response.write(&packet)?;
        last = sequence;
    }
}

fn main() -> WorkResult<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--worker") {
        if args.len() != 3 {
            return Err("worker needs request and response names".into());
        }
        return worker(&args[1], &args[2]);
    }
    let (mut payload, mut iterations) = (DEFAULT_PAYLOAD, DEFAULT_ITERATIONS);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = args.next().ok_or(format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--payload" => payload = value.parse()?,
            "--iterations" => iterations = value.parse()?,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if payload < 8 || iterations == 0 || iterations == SHUTDOWN {
        return Err("payload must be at least 8 bytes and iterations must be 1..u64::MAX".into());
    }
    let mut exchange = Exchange::start(payload)?;
    exchange.run(payload, iterations)?;
    println!("Completed {iterations} checked IPC exchanges ({payload} bytes each way)");
    Ok(())
}
