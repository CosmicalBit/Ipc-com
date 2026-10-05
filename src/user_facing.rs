use crate::allocation::{Allocation, Header, ReadOnly, ReadWrite};
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

pub(crate) struct Transformed<T, Access>
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
    pub(crate) fn inner_create(self) -> Result<Allocation<T, Access>> {
        let name = self.name.clone();
        let header = Header::try_from(self)?;
        Allocation::allocate_space(&name, header)
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
        let access = ReadWrite;
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

impl<T> SharedMemoryOptions<T, ReadOnly, Present, Present>
where
    T: SharedData,
{
    fn inner_transform(self) -> Result<Transformed<T, ReadOnly>> {
        // The Present states guarantee both values were set by the builder.
        Ok(Transformed {
            data: self.data.expect("Present data state"),
            name: self.name.expect("Present name state"),
            access: self.access,
        })
    }

    pub fn create(self) -> Result<SharedValue<T, ReadOnly>> {
        let transformed = self.inner_transform()?;
        let mut allocation = transformed.inner_create()?;
        allocation.write_all()?;
        Ok(SharedValue::from(allocation))
    }
}

impl<T> SharedMemoryOptions<T, ReadWrite, Present, Present>
where
    T: SharedData,
{
    fn inner_transform(self) -> Result<Transformed<T, ReadWrite>> {
        Ok(Transformed {
            data: self.data.expect("Present data state"),
            name: self.name.expect("Present name state"),
            access: self.access,
        })
    }

    pub fn create(self) -> Result<SharedValue<T, ReadWrite>> {
        let transformed = self.inner_transform()?;
        let mut allocation = transformed.inner_create()?;
        allocation.write_all()?;
        Ok(SharedValue::from(allocation))
    }
}
