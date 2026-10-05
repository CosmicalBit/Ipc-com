use crate::allocation::{Allocation, ReadOnly, ReadWrite};
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
    _access: PhantomData<Access>,
}

impl<T, Access> Transformed<T, Access>
where
    T: SharedData,
{
    pub(crate) fn inner_create(self) -> Result<SharedValue<T, Access>> {
        let Transformed { data, name, .. } = self;
        let data = data.as_bytes()?;
        let mut allocation = Allocation::allocate_space(&name, &data)?;
        allocation.write_all(&data)?;
        Ok(SharedValue::from(allocation))
    }
}

impl<T> SharedMemoryOptions<T, ReadOnly, Missing, Missing>
where
    T: SharedData,
{
    /// Starts configuring a shared value. Call [`Self::with_data`] and
    /// [`Self::name`] before creating it. Call [`Self::to_mutable`] if the
    /// creator needs write access. These options can be set in any order.
    pub fn new() -> Self {
        Self {
            data: None,
            name: None,
            access: ReadOnly,
            _state: PhantomData,
        }
    }
}

impl<T> Default for SharedMemoryOptions<T, ReadOnly, Missing, Missing>
where
    T: SharedData,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, Access, DataState, NameState> SharedMemoryOptions<T, Access, DataState, NameState>
where
    T: SharedData,
{
    ///Chooses the option for the data to be to mutable int  the prespective of the sender
    pub fn to_mutable(self) -> SharedMemoryOptions<T, ReadWrite, DataState, NameState> {
        let access = ReadWrite;
        SharedMemoryOptions {
            data: self.data,
            name: self.name,
            access,
            _state: PhantomData,
        }
    }
    ///Adds the data desired to be access with ipc to the [`SharedMemoryOptions`] struct
    ///
    ///Note: The data must implement the [`SharedData`] trait
    pub fn with_data(self, data: T) -> SharedMemoryOptions<T, Access, Present, NameState> {
        SharedMemoryOptions {
            data: Some(data),
            name: self.name,
            access: self.access,
            _state: PhantomData,
        }
    }
    ///Chooses the name for the smh_link socket, it doest need to start with a foward slash.
    ///Note: This is the name that you will need to use when doing [`SharedValue::new_reader()`]
    pub fn name(self, name: &str) -> SharedMemoryOptions<T, Access, DataState, Present> {
        let name = if name.starts_with('/') { name.to_owned() } else { format!("/{name}") };

        SharedMemoryOptions {
            data: self.data,
            access: self.access,
            name: Some(name),
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
            _access: PhantomData,
        })
    }
    /// Creates a read-only shared value with the configured name and data.
    pub fn create(self) -> Result<SharedValue<T, ReadOnly>> {
        self.inner_transform()?.inner_create()
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
            _access: PhantomData,
        })
    }

    /// Creates a writable shared value with the configured name and data.
    pub fn create(self) -> Result<SharedValue<T, ReadWrite>> {
        self.inner_transform()?.inner_create()
    }
}
