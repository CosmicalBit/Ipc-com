use crate::shared_mem::{Error, Mapping, Result};
use std::marker::PhantomData;
use std::sync::atomic::AtomicU32;
pub struct ReadOnly;
pub struct ReadWrite;

pub(crate) const HEADER_SIZE: usize = size_of::<AtomicU32>() + size_of::<u32>() + size_of::<AtomicU32>();

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
        let offset = unsafe { self.mapping.write_concrete_type(AtomicU32::new(0), 0)? };
        let offset = unsafe { self.mapping.write_concrete_type(AtomicU32::new(0), offset)? };
        let offset = unsafe { self.mapping.write_bytes(&len.to_be_bytes(), offset) };
        unsafe { self.mapping.write_bytes(data, offset) };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_has_two_atomic_words_and_a_length_word() {
        assert_eq!(HEADER_SIZE, 3 * size_of::<u32>());
        assert_eq!(align_of::<AtomicU32>(), align_of::<u32>());
    }
}
