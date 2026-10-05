use crate::{
    allocation::{Allocation, ReadOnly, ReadWrite},
    shared_mem::{Mapping, Result},
};
use std::{
    borrow::Cow,
    marker::PhantomData,
    sync::atomic::{self, AtomicU64, Ordering},
};

/// A value that can be serialized to and reconstructed from shared memory.
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

impl<T, Access> From<Allocation<T, Access>> for SharedValue<T, Access>
where
    T: SharedData,
{
    fn from(value: Allocation<T, Access>) -> Self {
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
    pub fn write(&mut self, data: T) -> Result<()> {
        let atomic = self.mapping.atomic_ref()?;

        Self::lock(atomic);

        let atomic = AtomicU64::new(atomic.load(Ordering::Relaxed));
        let res = self.mapping.write_data(data, atomic);

        let new_atomic = self.mapping.atomic_ref()?;
        Self::unlock(new_atomic);

        res
    }
}

impl<T, Access> SharedValue<T, Access>
where
    T: SharedData,
{
    fn lock(atomic: &AtomicU64) {
        loop {
            let current = atomic.load(std::sync::atomic::Ordering::Relaxed);

            if !current.is_multiple_of(2) {
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
    fn unlock(atomic: &AtomicU64) {
        atomic.fetch_add(1, atomic::Ordering::Release);
    }
    pub fn read(&self) -> Result<T> {
        let atomic = self.mapping.atomic_ref()?;

        Self::lock(atomic);

        //dont propagate upstream error or we will be forever locked
        let result = self.mapping.read_data();

        Self::unlock(atomic);

        result
    }
}
impl<T> SharedValue<T, ReadOnly>
where
    T: SharedData,
{
    ///user_facing: used to switch from read_only to readwrite
    ///
    /// #Safety
    /// be carefull, you are able to change to mutable even if the creator selected as nonmutable.
    pub fn to_mut(self) -> SharedValue<T, ReadWrite> {
        SharedValue::<T, ReadWrite> {
            mapping: self.mapping,
            _phantom: PhantomData,
        }
    }
}

impl<T> SharedValue<T, ReadOnly>
where
    T: SharedData,
{
    ///user_facing: function for reading IPC data
    pub fn new_reader(name: &str) -> Result<Self> {
        let mapping = Mapping::new_connect(name)?;

        Ok(Self { mapping, _phantom: PhantomData })
    }
}

#[cfg(test)]
mod test {
    use crate::user_facing::SharedMemoryOptions;

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

        let mem = SharedMemoryOptions::new().to_mutable().name("nana").with_data(string.clone()).create().unwrap();
        let red = mem.read().unwrap();

        assert!(red == string);
    }

    #[test]
    fn round_trip_realloc() {
        let string = String::from("b");

        let to_write = String::from("dkkkkkkkkkkkkkkkslfjlsdkjflsdkjflskdjfkdsljflsdjfi f8 ");
        let mut mem = SharedMemoryOptions::new().to_mutable().name("nana").with_data(string).create().unwrap();

        mem.write(to_write.clone()).unwrap();
        let red = mem.read().unwrap();

        assert!(red == to_write);
    }

    #[test]
    fn read_only_creation_uses_shared_header() {
        let name = format!("ipc_com_read_only_{}", std::process::id());
        let value = String::from("read only data");

        let mem = SharedMemoryOptions::new().name(&name).with_data(value.clone()).create().unwrap();
        assert_eq!(mem.read().unwrap(), value);
    }

    #[test]
    fn reader_connects_to_mutable_value() {
        let name = format!("ipc_com_reader_{}", std::process::id());
        let value = String::from("shared data");

        let owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(value.clone()).create().unwrap();
        let reader = SharedValue::<String, ReadOnly>::new_reader(&name).unwrap();

        assert_eq!(owner.read().unwrap(), value);
        assert_eq!(reader.read().unwrap(), value);
    }
}
