use crate::allocation::{AccessLayout, Allocation, Header, ReadOnly, ReadWrite, WriteAll};
use crate::shared_mem::Result;
use crate::shared_value::{SharedData, SharedValue};
use std::marker::PhantomData;
pub struct Missing;
pub struct Present;

pub struct SharedMemoryOptions<T, Access, DataState, NameState>
where
    T: SharedData,
{
    data: Option<T>,
    name: Option<String>,
    access: Access,
    _state: PhantomData<(DataState, NameState)>,
}

pub struct Transformed<T, Access>
where
    T: SharedData,
{
    pub(crate) data: T,
    pub(crate) name: String,
    pub(crate) access: Access,
}

impl<T, Access> Transformed<T, Access>
where
    T: SharedData,
{
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn to_header(self) -> Result<Header<T, Access>> {
        let len = self.data.as_bytes()?.len() as u32;
        Ok(Header {
            len,
            data: self.data,
            access: self.access,
        })
    }
}

impl<T> SharedMemoryOptions<T, ReadOnly, Missing, Missing>
where
    T: SharedData,
{
    pub fn new() -> Self {
        Self {
            data: None,
            name: None,
            access: ReadOnly,
            _state: PhantomData,
        }
    }
}

impl<T, Access, DataState, NameState> SharedMemoryOptions<T, Access, DataState, NameState>
where
    T: SharedData,
{
    pub fn to_mutable(self) -> SharedMemoryOptions<T, ReadWrite, DataState, NameState> {
        let access = ReadWrite::new();
        SharedMemoryOptions {
            data: self.data,
            name: self.name,
            access,
            _state: PhantomData,
        }
    }

    pub fn with_data(self, data: T) -> SharedMemoryOptions<T, Access, Present, NameState> {
        SharedMemoryOptions {
            data: Some(data),
            name: self.name,
            access: self.access,
            _state: PhantomData,
        }
    }

    pub fn name(self, name: &str) -> SharedMemoryOptions<T, Access, DataState, Present> {
        SharedMemoryOptions {
            data: self.data,
            access: self.access,
            name: Some(name.to_string()),
            _state: PhantomData,
        }
    }
}

impl<T, Access> SharedMemoryOptions<T, Access, Present, Present>
where
    T: SharedData,
    Access: AccessLayout,
    Allocation<T, Access>: WriteAll,
{
    pub(crate) fn inner_transform(self) -> Result<Transformed<T, Access>> {
        // The Present states guarantee both values were set by the builder.
        Ok(Transformed {
            data: self.data.expect("Present data state"),
            name: self.name.expect("Present name state"),
            access: self.access,
        })
    }
    //user facing abstranction
    pub fn create(self) -> Result<Access::Output>
    where
        Access: CreateOutput<T>,
    {
        let transformed = self.inner_transform()?;
        let mut allocation = transformed.inner_create()?;
        allocation.write_all()?;

        Ok(Access::finish(allocation))
    }
}
impl<T, Access> Transformed<T, Access>
where
    T: SharedData,
{
    pub fn inner_create(self) -> Result<Allocation<T, Access>>
    where
        Access: AccessLayout,
    {
        //we just need the space for the data plus one for the atomic boollean
        //TODO
        let _size = size_of_val(&self.data);
        let name = &self.name.to_owned();
        let header = Header::try_from(self)?;
        Allocation::allocate_space(name, header)
    }
}

pub(crate) trait CreateOutput<T>: Sized
where
    T: SharedData,
{
    type Output;
    fn finish(allocation: Allocation<T, Self>) -> Self::Output;
}

impl<T: SharedData> CreateOutput<T> for ReadWrite {
    type Output = SharedValue<T>;

    fn finish(allocation: Allocation<T, Self>) -> Self::Output {
        SharedValue::from(allocation)
    }
}
impl<T: SharedData> CreateOutput<T> for ReadOnly {
    type Output = Allocation<T, ReadOnly>;
    fn finish(allocation: Allocation<T, Self>) -> Self::Output {
        allocation
    }
}
