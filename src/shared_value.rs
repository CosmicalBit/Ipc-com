use crate::{
    allocation::{Allocation, ReadOnly, ReadWrite},
    futex::{Futex, WaitResult},
    shared_mem::{Error, Mapping, Result},
};
use std::{
    borrow::Cow,
    marker::PhantomData,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

///0 means ulocked any number thats   not 0 is the process holders PID
pub(crate) const ULOCKED: u32 = 0;

/// This trait is needed implemented for the data that you wish to share over ipc
///
/// Note: if by any chance you type has variable sized fields or its variable size like String or
/// other types in the [`SharedData::as_bytes()`] and [`SharedData::from_bytes()`] dont forget to
/// store fields with a fixed size indecating the size of the variable payload so they can be
/// written and read correctly
pub trait SharedData: Sized {
    fn as_bytes(&self) -> Result<Cow<'_, [u8]>>;
    fn from_bytes(bytes: &[u8]) -> Result<Self>;
}

pub struct SharedValue<T, Access = ReadOnly>
where
    T: SharedData,
{
    mapping: Mapping,
    _phantom: PhantomData<(Access, T)>,
}

impl<T, Access> From<Allocation<Access>> for SharedValue<T, Access>
where
    T: SharedData,
{
    fn from(value: Allocation<Access>) -> Self {
        Self {
            mapping: value.mapping,
            _phantom: PhantomData,
        }
    }
}
impl<T> SharedValue<T, ReadWrite>
where
    T: SharedData,
{
    pub fn write(&mut self, data: &T) -> Result<()> {
        let atomic = self.mapping.atomic_lock()?;

        Self::lock(atomic, self.mapping.atomic_watchers()?)?;

        let res = self.mapping.write_data(data);
        let new_atomic = self.mapping.atomic_lock()?;

        let unlock_result = Self::unlock(new_atomic, self.mapping.atomic_watchers()?);

        if res.is_ok() {
            let generation = self.mapping.atomic_generation()?;
            generation.fetch_add(1, Ordering::Release);
            Futex::new(generation, self.mapping.atomic_watchers()?).wake_all()?;
        }

        res.and(unlock_result)
    }
}

impl<T, Access> SharedValue<T, Access>
where
    T: SharedData,
{
    /// Clears the lock after [`Error::OwnerDied`] without changing `generation`
    /// or waking processes waiting for the lock.
    ///
    /// # Safety
    ///
    /// The caller must ensure only one process performs recovery and no other
    /// process acquires the lock until recovery is complete.
    pub unsafe fn force_unlock(&self) -> Result<bool> {
        let atomic = self.mapping.atomic_lock()?;

        let unlocked = atomic.compare_exchange(atomic.load(Ordering::Acquire), ULOCKED, Ordering::Release, Ordering::Relaxed).is_ok();

        Ok(unlocked)
    }

    /// Wakes processes waiting for the lock during recovery.
    ///
    /// # Safety
    ///
    /// The caller must coordinate recovery so waiters do not acquire the lock
    /// before the shared value is ready to use.
    pub unsafe fn force_awake(&self) -> Result<()> {
        let atomic = self.mapping.atomic_lock()?;
        Futex::new(atomic, self.mapping.atomic_watchers()?).wake_all()?;
        Ok(())
    }

    #[cfg(all(test, not(miri)))]
    unsafe fn force_lock(&mut self) -> Result<()> {
        let pid = unsafe { u32::try_from(libc::getpid())? };
        let atomic = self.mapping.atomic_lock()?;
        atomic.store(pid, Ordering::Release);
        Ok(())
    }

    fn lock(atomic: &AtomicU32, watchers: &AtomicU32) -> Result<()> {
        Self::lock_with_timeout(atomic, watchers, Duration::from_secs(5))
    }

    fn lock_with_timeout(atomic: &AtomicU32, watchers: &AtomicU32, timeout: Duration) -> Result<()> {
        // The lock word contains the owning process ID, or zero when unlocked.
        let pid = u32::try_from(unsafe { libc::getpid() })?;
        let mut observed_owner = ULOCKED;
        let mut last_check = Instant::now();
        loop {
            let current = atomic.load(Ordering::Acquire);

            if current != ULOCKED {
                if current != observed_owner {
                    observed_owner = current;
                    last_check = Instant::now();
                }
                let futex = Futex::new(atomic, watchers);

                match futex.wait_timeout(current, timeout.saturating_sub(last_check.elapsed()))? {
                    WaitResult::Woken => continue,

                    WaitResult::TimedOut => {
                        if atomic.load(Ordering::Acquire) == current && !Self::owner_is_alive(current)? && atomic.load(Ordering::Acquire) == current {
                            return Err(Error::OwnerDied);
                        }
                        last_check = Instant::now();
                        continue;
                    },
                }
            }

            if atomic.compare_exchange(ULOCKED, pid, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                return Ok(());
            }
        }
    }
    fn owner_is_alive(pid: u32) -> Result<bool> {
        let pid = libc::pid_t::try_from(pid)?;
        if unsafe { libc::kill(pid, 0) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => Ok(false),
            Some(libc::EPERM) => Ok(true),
            _ => Err(Error::Futex(error)),
        }
    }
    fn unlock(atomic: &AtomicU32, watchers: &AtomicU32) -> Result<()> {
        atomic.store(ULOCKED, Ordering::Release);
        Futex::new(atomic, watchers).wake_all()
    }
    /// Reads the current value, growing this handle's mapping if needed.
    pub fn read(&mut self) -> Result<T> {
        {
            let atomic = self.mapping.atomic_lock()?;

            Self::lock(atomic, self.mapping.atomic_watchers()?)?;
        }
        // Unlock even when reading or decoding fails.
        let result = self.mapping.read_data();

        // The lock address may have moved with the mapping.
        let atomic = self.mapping.atomic_lock()?;
        let unlock_result = Self::unlock(atomic, self.mapping.atomic_watchers()?);
        let value = result?;
        unlock_result?;
        Ok(value)
    }
}

impl<T> SharedValue<T, ReadOnly>
where
    T: SharedData,
{
    ///user_facing: function for reading IPC data from the `name`
    pub fn open(name: &str) -> Result<Self> {
        let name = if name.starts_with('/') { name.to_owned() } else { format!("/{name}") };
        let mapping = Mapping::new_connect(&name)?;

        Ok(Self { mapping, _phantom: PhantomData })
    }
    pub fn into_mutable(self) -> SharedValue<T, ReadWrite> {
        SharedValue {
            mapping: self.mapping,
            _phantom: PhantomData,
        }
    }
}

impl<T, Access> SharedValue<T, Access>
where
    T: SharedData,
{
    /// Blocks until a successful write, then returns the current value.
    pub fn wait_for_change_value(&mut self) -> Result<T> {
        let generation = self.mapping.atomic_generation()?;
        let expected = generation.load(Ordering::Acquire);
        let futex = Futex::new(generation, self.mapping.atomic_watchers()?);
        while generation.load(Ordering::Acquire) == expected {
            futex.wait(expected)?;
        }
        self.read()
    }
    ///waits  for a change nonblockin for a change nonblocking
    pub fn wait_for_change_async(&self) -> Result<std::thread::JoinHandle<Result<T>>>
    where
        T: Send + 'static,
    {
        let name = self.mapping.name().to_owned();

        Ok(std::thread::spawn(move || -> Result<T> {
            let mut value: SharedValue<T> = SharedValue::open(name.to_str().expect("error convertingto str"))?;
            value.wait_for_change_value()
        }))
    }
}
// These integration tests require POSIX shared memory, which Miri cannot emulate.
#[cfg(all(test, not(miri)))]
mod test {
    use crate::user_facing::SharedMemoryOptions;
    use std::time::Duration;

    use super::*;

    impl SharedData for String {
        fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
            Ok(Cow::Borrowed(self.as_bytes()))
        }
        fn from_bytes(bytes: &[u8]) -> Result<Self> {
            Ok(String::from_utf8_lossy(bytes).to_string())
        }
    }
    #[test]
    fn dead_lock_owner_is_reported() {
        // Linux PIDs are bounded well below pid_t::MAX.
        let lock = AtomicU32::new(libc::pid_t::MAX as u32);
        let watchers = AtomicU32::new(0);
        assert!(matches!(SharedValue::<String>::lock_with_timeout(&lock, &watchers, Duration::from_millis(1)), Err(Error::OwnerDied)));
    }

    #[test]
    fn forced_unlock_clears_lock_without_changing_generation() {
        let name = format!("/ipc_com_forced_unlock_{}", std::process::id());
        let mut owner = SharedMemoryOptions::new().with_data(String::from("papa")).name(&name).create().unwrap();
        let generation_before = owner.mapping.atomic_generation().unwrap().load(Ordering::Acquire);

        unsafe { owner.force_lock().unwrap() };
        assert_eq!(owner.mapping.atomic_lock().unwrap().load(Ordering::Acquire), u32::try_from(unsafe { libc::getpid() }).unwrap());
        unsafe { owner.force_unlock().unwrap() };
        assert_eq!(owner.mapping.atomic_lock().unwrap().load(Ordering::Acquire), 0);
        assert_eq!(owner.mapping.atomic_generation().unwrap().load(Ordering::Acquire), generation_before);
    }
    #[test]
    fn unlocking_wakes_waiting_reader() {
        let lock = AtomicU32::new(0);
        let watchers = AtomicU32::new(0);
        SharedValue::<String>::lock(&lock, &watchers).unwrap();

        std::thread::scope(|scope| {
            let (started, ready) = std::sync::mpsc::channel();
            let lock = &lock;
            let watchers = &watchers;
            let waiter = scope.spawn(move || {
                started.send(()).unwrap();
                let start = Instant::now();
                SharedValue::<String>::lock_with_timeout(lock, watchers, Duration::from_secs(2)).unwrap();
                assert!(start.elapsed() < Duration::from_secs(1), "unlock did not wake the waiter");
                SharedValue::<String>::unlock(lock, watchers).unwrap();
            });
            ready.recv().unwrap();
            std::thread::sleep(Duration::from_millis(10));
            SharedValue::<String>::unlock(lock, watchers).unwrap();
            waiter.join().unwrap();
        });
    }

    #[test]
    fn round_trip_same_size() {
        let string = String::from("batata");
        let name = format!("/ipc_com_same_size_{}", std::process::id());

        let mut mem = SharedMemoryOptions::new().to_mutable().name(&name).with_data(string.clone()).create().unwrap();
        let red = mem.read().unwrap();

        assert!(red == string);
    }

    #[test]
    fn round_trip_realloc() {
        let string = String::from("b");
        let name = format!("/ipc_com_realloc_{}", std::process::id());

        let to_write = String::from("dkkkkkkkkkkkkkkkslfjlsdkjflsdkjflskdjfkdsljflsdjfi f8 ");
        let mut mem = SharedMemoryOptions::new().to_mutable().name(&name).with_data(string).create().unwrap();

        mem.write(&to_write).unwrap();
        let red = mem.read().unwrap();

        assert!(red == to_write);
    }

    #[test]
    fn existing_reader_reads_grown_value() {
        let name = format!("ipc_com_grown_reader_{}", std::process::id());
        let mut owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(String::from("b")).create().unwrap();
        let mut reader = SharedValue::<String>::open(&name).unwrap();

        owner.write(&String::from("a much longer value")).unwrap();
        assert_eq!(reader.read().unwrap(), "a much longer value");
        assert_eq!(reader.read().unwrap(), "a much longer value");

        owner.write(&String::from("an even longer value than before")).unwrap();
        assert_eq!(reader.read().unwrap(), "an even longer value than before");
    }

    #[test]
    fn read_only_creation_uses_shared_header() {
        let name = format!("ipc_com_read_only_{}", std::process::id());
        let value = String::from("read only data");

        let options: SharedMemoryOptions<String> = SharedMemoryOptions::new();
        let mut mem: SharedValue<String> = options.name(&name).with_data(value.clone()).create().unwrap();
        assert_eq!(mem.read().unwrap(), value);
    }

    #[test]
    fn reader_connects_to_mutable_value() {
        let name = format!("ipc_com_reader_{}", std::process::id());
        let value = String::from("shajred data");

        let mut owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(value.clone()).create().unwrap();
        let mut reader = SharedValue::<String>::open(&name).unwrap();

        assert_eq!(owner.read().unwrap(), value);
        assert_eq!(reader.read().unwrap(), value);
    }

    #[test]
    fn blocking_wait_returns_updated_value() {
        let name = format!("ipc_com_blocking_wait_{}", std::process::id());
        let mut owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(String::from("before")).create().unwrap();

        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| {
                let mut reader = SharedValue::<String>::open(&name).unwrap();
                reader.wait_for_change_value().unwrap()
            });
            std::thread::sleep(Duration::from_millis(50));
            owner.write(&String::from("after a longer write")).unwrap();
            assert_eq!(waiting.join().unwrap(), "after a longer write");
        });
    }

    #[test]
    fn async_wait_returns_updated_value() {
        let name = format!("ipc_com_async_wait_{}", std::process::id());
        let mut owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(String::from("before")).create().unwrap();
        let reader = SharedValue::<String>::open(&name).unwrap();

        let waiting = reader.wait_for_change_async().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        owner.write(&String::from("after")).unwrap();

        assert_eq!(waiting.join().unwrap().unwrap(), "after");
    }
    struct TestData {
        name: [u8; 4],
        year: u16,
    }
    impl SharedData for TestData {
        fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
            let mut vec = Vec::new();
            vec.extend_from_slice(&self.name);
            vec.extend_from_slice(&self.year.to_be_bytes());
            Ok(Cow::Owned(vec))
        }

        fn from_bytes(bytes: &[u8]) -> Result<Self> {
            let name: [u8; 4] = bytes[0..4].try_into().unwrap();
            let year = u16::from_be_bytes(bytes[4..6].try_into().unwrap());

            Ok(Self { name, year })
        }
    }
    #[test]
    fn concurrent_read_write() {
        use std::thread;

        const ITERATIONS: u16 = 10_000;
        const READERS: usize = 8;

        let name = format!("/ipc_race_{}", std::process::id());

        let initial = TestData { name: 0u32.to_be_bytes(), year: 0 };

        // Only create the shared memory here.
        let mut owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(initial).create().unwrap();

        thread::scope(|scope| {
            for _ in 0..READERS {
                let name = &name;

                scope.spawn(move || {
                    let mut reader = SharedValue::<TestData>::open(name).unwrap();

                    for _ in 0..ITERATIONS {
                        let value = reader.read().unwrap();
                        let number = u32::from_be_bytes(value.name);

                        assert_eq!(value.year, u16::try_from(number).unwrap(), "torn read: name encoded {number}, year was {}", value.year);
                    }
                });
            }

            for i in 0..ITERATIONS {
                owner
                    .write(&TestData {
                        name: u32::from(i).to_be_bytes(),
                        year: i,
                    })
                    .unwrap();
            }
        });
    }
}
