use crate::{
    allocation::{Allocation, ReadOnly, ReadWrite},
    futex::Futex,
    shared_mem::{Mapping, Result},
};
use std::{
    borrow::Cow,
    marker::PhantomData,
    sync::atomic::{self, AtomicU32, Ordering},
};

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

pub struct SharedValue<T, Access>
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
        let atomic = self.mapping.atomic_ref()?;

        Self::lock(atomic);

        let res = self.mapping.write_data(data);
        let new_atomic = self.mapping.atomic_ref()?;

        Self::unlock(new_atomic);

        if res.is_ok() {
            let generation = self.mapping.atomic_gen()?;
            generation.fetch_add(1, Ordering::Release);
            Futex::new(generation).wake_all()?;
        }

        res
    }
}

impl<T, Access> SharedValue<T, Access>
where
    T: SharedData,
{
    fn lock(atomic: &AtomicU32) {
        loop {
            let current = atomic.load(std::sync::atomic::Ordering::Relaxed);

            if current % 2 != 0 {
                continue;
            }

            if atomic
                .compare_exchange(current, current.wrapping_add(1), std::sync::atomic::Ordering::Acquire, std::sync::atomic::Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
    }
    fn unlock(atomic: &AtomicU32) {
        atomic.fetch_add(1, atomic::Ordering::Release);
    }
    /// Reads the current value, growing this handle's mapping if needed.
    pub fn read(&mut self) -> Result<T> {
        {
            let atomic = self.mapping.atomic_ref()?;

            Self::lock(atomic);
        }
        // Unlock even when reading or decoding fails.
        let result = self.mapping.read_data();

        // The lock address may have moved with the mapping.
        let atomic = self.mapping.atomic_ref()?;
        Self::unlock(atomic);

        result
    }
}
impl<T> SharedValue<T, ReadOnly>
where
    T: SharedData,
{
    ///user_facing: function for reading IPC data from the `name`
    pub fn new_reader(name: &str) -> Result<Self> {
        let name = if name.starts_with('/') { name.to_owned() } else { format!("/{name}") };
        let mapping = Mapping::new_connect(&name)?;

        Ok(Self { mapping, _phantom: PhantomData })
    }
}

impl<T, Access> SharedValue<T, Access>
where
    T: SharedData,
{
    /// Blocks until a successful write, then returns the current value.
    pub fn wait_for_change_value(&mut self) -> Result<T> {
        let generation = self.mapping.atomic_gen()?;
        let expected = generation.load(Ordering::Acquire);
        let futex = Futex::new(generation);
        while generation.load(Ordering::Acquire) == expected {
            futex.wait(expected)?;
        }
        self.read()
    }
    ///waits  for a change nonblockin for a change nonblocking
    pub fn wait_for_change_async<F>(&self) -> Result<std::thread::JoinHandle<Result<T>>>
    where
        T: Send + 'static,
    {
        let name = self.mapping.name().to_owned();

        Ok(std::thread::spawn(move || -> Result<T> {
            let mut value: SharedValue<T, ReadOnly> = SharedValue::new_reader(name.to_str().expect("error convertingto str"))?;
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
        let mut reader = SharedValue::<String, ReadOnly>::new_reader(&name).unwrap();

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

        let mut mem = SharedMemoryOptions::new().name(&name).with_data(value.clone()).create().unwrap();
        assert_eq!(mem.read().unwrap(), value);
    }

    #[test]
    fn reader_connects_to_mutable_value() {
        let name = format!("ipc_com_reader_{}", std::process::id());
        let value = String::from("shajred data");

        let mut owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(value.clone()).create().unwrap();
        let mut reader = SharedValue::<String, ReadOnly>::new_reader(&name).unwrap();

        assert_eq!(owner.read().unwrap(), value);
        assert_eq!(reader.read().unwrap(), value);
    }

    #[test]
    fn blocking_wait_returns_updated_value() {
        let name = format!("ipc_com_blocking_wait_{}", std::process::id());
        let mut owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(String::from("before")).create().unwrap();

        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| {
                let mut reader = SharedValue::<String, ReadOnly>::new_reader(&name).unwrap();
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
        let reader = SharedValue::<String, ReadOnly>::new_reader(&name).unwrap();

        let waiting = reader.wait_for_change_async::<()>().unwrap();
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
                    let mut reader = SharedValue::<TestData, ReadOnly>::new_reader(name).unwrap();

                    for _ in 0..ITERATIONS {
                        let value = reader.read().unwrap();
                        let number = u32::from_be_bytes(value.name);

                        assert_eq!(value.year, number as u16, "torn read: name encoded {number}, year was {}", value.year);
                    }
                });
            }

            for i in 0..ITERATIONS {
                owner
                    .write(&TestData {
                        name: (i as u32).to_be_bytes(),
                        year: i,
                    })
                    .unwrap();
            }
        });
    }
}
