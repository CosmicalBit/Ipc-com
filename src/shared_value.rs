use crate::{
    allocation::{Allocation, ReadWrite},
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
pub struct SharedValue<T>
where
    T: SharedData,
{
    mapping: Mapping,
    _phantom: PhantomData<T>,
}

impl<T> From<Allocation<T, ReadWrite>> for SharedValue<T>
where
    T: SharedData,
{
    fn from(value: Allocation<T, ReadWrite>) -> Self {
        Self {
            mapping: value.mapping,
            _phantom: PhantomData,
        }
    }
}
impl<T> SharedValue<T>
where
    T: SharedData,
{
    fn lock(atomic: &AtomicU64) {
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
}
