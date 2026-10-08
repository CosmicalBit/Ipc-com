use crate::{
    allocation::START_OF_DATA_OFFSET,
    shared_mem::{Mapping, Result},
    shared_value::{LockUlock, SharedData, SharedValue},
};
/// Decodes a view that may borrow directly from shared memory.
pub trait SharedView: SharedData {
    type View<'a>
    where
        Self: 'a;

    fn view_from_bytes(data: &[u8]) -> Result<Self::View<'_>>;
}

struct Lock;
impl LockUlock for Lock {}

impl<T: SharedView, Access> SharedValue<T, Access> {
    /// Locks the value and grows this handle's mapping if the data has grown.
    pub fn read_guard(&mut self) -> Result<ReadGuard<'_, T, Access>> {
        ReadGuard::new(self)
    }
}

/// Holds the shared-memory lock while views borrow from this handle's mapping.
pub struct ReadGuard<'a, T: SharedView, Access> {
    value: &'a mut SharedValue<T, Access>,
    len: usize,
}

impl<'a, T: SharedView, Access> ReadGuard<'a, T, Access> {
    pub(crate) fn new(value: &'a mut SharedValue<T, Access>) -> Result<Self> {
        let lock = value.mapping.atomic_lock()?;
        let watchers = value.mapping.atomic_watchers()?;
        Lock::lock(lock, watchers)?;

        // Create the guard before the fallible remap so errors still unlock.
        let mut guard = Self { value, len: 0 };
        guard.len = guard.value.mapping.data_len()?;
        Ok(guard)
    }

    /// Returns a view borrowed from this guard. The view cannot outlive the lock.
    pub fn view(&self) -> Result<T::View<'_>> {
        let bytes = unsafe { self.value.mapping.read_bytes(self.len, START_OF_DATA_OFFSET)? };
        T::view_from_bytes(bytes)
    }
}

impl<T: SharedView, Access> Drop for ReadGuard<'_, T, Access> {
    #[inline]
    fn drop(&mut self) {
        // The mapping may have moved while the guard was created.
        let mapping: &Mapping = &self.value.mapping;
        if let (Ok(lock), Ok(watchers)) = (mapping.atomic_lock(), mapping.atomic_watchers()) {
            let _ = Lock::unlock(lock, watchers);
        }
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use std::{borrow::Cow, sync::atomic::Ordering};

    use super::*;
    use crate::{Error, SharedMemoryOptions};

    struct Bytes(Vec<u8>);

    impl SharedData for Bytes {
        fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
            Ok(Cow::Borrowed(&self.0))
        }

        fn from_bytes(bytes: &[u8]) -> Result<Self> {
            Ok(Self(bytes.to_vec()))
        }
    }

    impl SharedView for Bytes {
        type View<'a> = &'a [u8];

        fn view_from_bytes(data: &[u8]) -> Result<Self::View<'_>> {
            if data.first() == Some(&0) {
                return Err(Error::IncompatibleHeader);
            }
            Ok(data)
        }
    }

    #[test]
    fn view_survives_mapping_growth_and_unlocks_on_drop() {
        let name = format!("ipc_com_view_{}", std::process::id());
        let mut owner = SharedMemoryOptions::new().to_mutable().name(&name).with_data(Bytes(vec![1])).create().unwrap();
        let mut reader = SharedValue::<Bytes>::open(&name).unwrap();

        owner.write(&Bytes(vec![2; 4096])).unwrap();
        {
            let guard = reader.read_guard().unwrap();
            let view = guard.view().unwrap();
            assert_eq!(view, vec![2; 4096]);
            assert_ne!(owner.mapping.atomic_lock().unwrap().load(Ordering::Acquire), 0);
        }
        assert_eq!(owner.mapping.atomic_lock().unwrap().load(Ordering::Acquire), 0);

        owner.write(&Bytes(vec![0])).unwrap();
        let guard = reader.read_guard().unwrap();
        assert!(matches!(guard.view(), Err(Error::IncompatibleHeader)));
        drop(guard);
        assert_eq!(owner.mapping.atomic_lock().unwrap().load(Ordering::Acquire), 0);
    }
}
