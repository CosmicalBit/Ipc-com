use crate::shared_mem::Mapping;
use crate::shared_mem::Result;
use crate::shared_value::SharedData;
use crate::user_facing::Transformed;
use std::sync::atomic::AtomicU64;

pub(crate) struct ReadOnly;
pub(crate) struct ReadWrite {
    pub(crate) atom_safe_counter: AtomicU64,
}
impl ReadWrite {
    pub(crate) const fn new() -> Self {
        Self { atom_safe_counter: AtomicU64::new(0) }
    }
}

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
pub struct Allocation<T, Access>
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
    pub(crate) fn allocate_space(name: &str, header: Header<T, Access>) -> Result<Allocation<T, Access>>
    where
        Access: AccessLayout,
    {
        let size = header.total_size_to_alloc()?;
        let mapping = Mapping::init_shared_mem(name, size)?;
        Ok(Allocation::<T, Access> { mapping, header })
    }
}

impl<T, Access> Header<T, Access>
where
    T: SharedData,
    Access: AccessLayout,
{
    pub(crate) fn total_size_to_alloc(&self) -> Result<usize> {
        let data_len = self.data.as_bytes()?.len();
        Ok(Access::prefix_size() + data_len)
    }
    pub(crate) fn len_bytes(&self) -> [u8; 4] {
        self.len.to_be_bytes()
    }
}
pub(crate) trait AccessLayout {
    fn prefix_size() -> usize;
}
impl AccessLayout for ReadWrite {
    fn prefix_size() -> usize {
        size_of::<AtomicU64>().next_multiple_of(align_of::<u32>())
    }
}
impl AccessLayout for ReadOnly {
    fn prefix_size() -> usize {
        size_of::<u32>() + size_of::<u32>()
    }
}

pub(crate) trait WriteAll {
    fn write_all(&mut self) -> Result<()>;
}

impl<T: SharedData> WriteAll for Allocation<T, ReadOnly> {
    fn write_all(&mut self) -> Result<()> {
        let offset = unsafe { self.mapping.write_bytes(&self.header.len_bytes(), 0) };
        unsafe { self.mapping.write_bytes(&self.header.data.as_bytes()?, offset) };
        Ok(())
    }
}
impl<T: SharedData> WriteAll for Allocation<T, ReadWrite> {
    fn write_all(&mut self) -> Result<()> {
        //false clone of the atomic
        let current_atomic = AtomicU64::new(self.header.access.atom_safe_counter.load(std::sync::atomic::Ordering::Relaxed));
        //write the atomic
        let offset = unsafe { self.mapping.write_concrete_type::<AtomicU64>(current_atomic, 0) };

        //write the lenght
        let offset = unsafe { self.mapping.write_bytes(&self.header.len_bytes(), offset) };
        unsafe { self.mapping.write_bytes(&self.header.data.as_bytes()?, offset) };
        Ok(())
    }
}
