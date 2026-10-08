use std::{marker::PhantomData, sync::atomic::AtomicU32};

use crate::shared_mem::{Error, Mapping, Result};

pub struct ReadOnly;
pub struct ReadWrite;

pub(crate) const HEADER_SIZE: usize = size_of::<AtomicU32>().checked_mul(3).unwrap().checked_add(size_of::<u32>()).unwrap();

// The lock is used to lock the memory so its thread safe.
/// The `Generation` is the current Generation that we are on, its used to wake wathers if they exist
/// The `Watchers` shows the current number of watchers, if is 0 theres no need to awake watchers
/// bcs theres none
pub(crate) const LOCK_OFFSET: usize = 0;
pub(crate) const GENERATION_OFFSET: usize = size_of::<AtomicU32>();
pub(crate) const WATCHERS_OFFSET: usize = size_of::<AtomicU32>() * 2;
pub(crate) const LEN_OFFSET: usize = size_of::<AtomicU32>() * 3;
pub(crate) const START_OF_DATA_OFFSET: usize = size_of::<AtomicU32>() * 3 + size_of::<u32>();

pub(crate) struct Allocation<Access> {
    pub(crate) mapping: Mapping,
    pub(crate) phantom: PhantomData<Access>,
}
impl<Access> Allocation<Access> {
    pub(crate) fn allocate_space(name: &str, data: &[u8]) -> Result<Allocation<Access>> {
        u32::try_from(data.len())?;
        let size = HEADER_SIZE.checked_add(data.len()).ok_or(Error::ArithmeticOverflow)?;
        let mapping = Mapping::init_shared_memory(name, size)?;
        Ok(Allocation::<Access> { mapping, phantom: PhantomData })
    }

    pub(crate) fn write_all(&mut self, data: &[u8]) -> Result<()> {
        let len = u32::try_from(data.len())?;
        unsafe { self.mapping.write_concrete_type(AtomicU32::new(0), LOCK_OFFSET)? };
        unsafe { self.mapping.write_concrete_type(AtomicU32::new(0), GENERATION_OFFSET)? };
        unsafe { self.mapping.write_concrete_type(AtomicU32::new(0), WATCHERS_OFFSET)? };
        unsafe { self.mapping.write_bytes(&len.to_be_bytes(), LEN_OFFSET)? };
        unsafe { self.mapping.write_bytes(data, START_OF_DATA_OFFSET)? };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SharedData, SharedMemoryOptions};

    #[test]
    fn header_has_three_atomic_words_and_length() {
        assert_eq!(HEADER_SIZE, 4 * size_of::<u32>());
        assert_eq!(align_of::<AtomicU32>(), align_of::<u32>());
        assert_eq!(GENERATION_OFFSET, LOCK_OFFSET + size_of::<AtomicU32>());
        assert_eq!(WATCHERS_OFFSET, GENERATION_OFFSET + size_of::<AtomicU32>());
        assert_eq!(LEN_OFFSET, WATCHERS_OFFSET + size_of::<AtomicU32>());
        assert_eq!(START_OF_DATA_OFFSET, LEN_OFFSET + size_of::<u32>());
        assert_eq!(HEADER_SIZE, START_OF_DATA_OFFSET);
    }
}
