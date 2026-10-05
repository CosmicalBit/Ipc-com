use crate::shared_mem::Mapping;
use crate::shared_mem::Result;
use crate::shared_value::SharedData;
use crate::user_facing::Transformed;
use std::sync::atomic::AtomicU64;

pub struct ReadOnly;
pub struct ReadWrite;

pub(crate) const HEADER_SIZE: usize = size_of::<AtomicU64>() + size_of::<u32>();

#[repr(C)]
pub(crate) struct Header<T, Access>
where
    T: crate::shared_value::SharedData,
{
    pub(crate) len: u32,
    pub(crate) data: T,
    pub(crate) access: Access,
}
impl<T, Access> TryFrom<Transformed<T, Access>> for Header<T, Access>
where
    T: crate::shared_value::SharedData,
{
    type Error = crate::shared_mem::Error;
    fn try_from(transformed: Transformed<T, Access>) -> Result<Self> {
        let len = transformed.data.as_bytes()?.len() as u32;
        Ok(Self {
            len,
            data: transformed.data,
            access: transformed.access,
        })
    }
}
pub(crate) struct Allocation<T, Access>
where
    T: crate::shared_value::SharedData,
{
    pub(crate) mapping: Mapping,
    pub(crate) header: Header<T, Access>,
}
impl<T, Access> Allocation<T, Access>
where
    T: crate::shared_value::SharedData,
{
    pub(crate) fn allocate_space(name: &str, header: Header<T, Access>) -> Result<Allocation<T, Access>> {
        let size = header.total_size_to_alloc()?;
        let mapping = Mapping::init_shared_mem(name, size)?;
        Ok(Allocation::<T, Access> { mapping, header })
    }

    pub(crate) fn write_all(&mut self) -> Result<()> {
        let offset = unsafe { self.mapping.write_concrete_type(AtomicU64::new(0), 0) };
        let offset = unsafe { self.mapping.write_bytes(&self.header.len_bytes(), offset) };
        unsafe { self.mapping.write_bytes(&self.header.data.as_bytes()?, offset) };
        Ok(())
    }
}

impl<T, Access> Header<T, Access>
where
    T: SharedData,
{
    pub(crate) fn total_size_to_alloc(&self) -> Result<usize> {
        let data_len = self.data.as_bytes()?.len();
        Ok(HEADER_SIZE + data_len)
    }
    pub(crate) fn len_bytes(&self) -> [u8; 4] {
        self.len.to_be_bytes()
    }
}
