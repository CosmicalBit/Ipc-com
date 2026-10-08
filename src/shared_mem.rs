use std::{
    ffi::{CString, NulError},
    mem::MaybeUninit,
    os::raw::c_void,
    ptr,
    ptr::NonNull,
    sync::atomic::AtomicU32,
};

use libc::{MREMAP_MAYMOVE, close, shm_unlink};

use crate::{
    allocation::{GENERATION_OFFSET, HEADER_SIZE, LEN_OFFSET, START_OF_DATA_OFFSET, WATCHERS_OFFSET},
    shared_value::SharedData,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    FileDescriptor(std::io::Error),
    Mmap(std::io::Error),
    Null(NulError),
    NullPtr,
    TryConversion(std::num::TryFromIntError),
    SliceConversion(std::array::TryFromSliceError),
    ArithmeticOverflow,
    IncompatibleHeader,
    DuplicatedName(std::io::Error),
    Futex(std::io::Error),
    OutOfBounds,
    OwnerDied,
    LockStateError,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileDescriptor(error) => write!(f, "shared memory file descriptor: {error}"),
            Self::Mmap(error) => write!(f, "shared memory mapping: {error}"),
            Self::Null(error) => write!(f, "invalid shared memory name: {error}"),
            Self::NullPtr => write!(f, "null shared memory pointer"),
            Self::TryConversion(error) => write!(f, "integer conversion: {error}"),
            Self::SliceConversion(error) => write!(f, "slice conversion: {error}"),
            Self::ArithmeticOverflow => write!(f, "shared memory size overflow"),
            Self::IncompatibleHeader => write!(f, "incompatible shared memory header"),
            Self::DuplicatedName(error) => write!(f, "shared memory name already exists: {error}"),
            Self::Futex(error) => write!(f, "futex operation: {error}"),
            Self::OwnerDied => write!(f, "shared memory lock owner died"),
            Self::LockStateError => write!(f, "the lock hit a critical failure"),
            Self::OutOfBounds => write!(f, "out of bounds read or write occured"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::num::TryFromIntError> for Error {
    fn from(value: std::num::TryFromIntError) -> Self {
        Error::TryConversion(value)
    }
}
impl From<std::array::TryFromSliceError> for Error {
    fn from(error: std::array::TryFromSliceError) -> Self {
        Self::SliceConversion(error)
    }
}
impl From<NulError> for Error {
    fn from(error: NulError) -> Self {
        Self::Null(error)
    }
}

pub(crate) struct Mapping {
    start: NonNull<u8>,
    size: usize,
    fd: i32,
    owner: bool,
    name: CString,
}

impl Mapping {
    pub(crate) fn name(&self) -> &CString {
        &self.name
    }
    fn new(mut_ptr: *mut u8, size: usize, fd: i32, name: CString, owner: bool) -> Result<Self> {
        Ok(Mapping {
            start: NonNull::new(mut_ptr).ok_or(Error::NullPtr)?,
            size,
            fd,
            name,
            owner,
        })
    }
    pub(crate) fn new_connect(name: &str) -> Result<Self> {
        let name = CString::new(name)?;

        let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, 0) };

        if fd == -1 {
            return Err(Error::FileDescriptor(std::io::Error::last_os_error()));
        }

        let mut stat = MaybeUninit::<libc::stat>::uninit();

        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } == -1 {
            let error = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(Error::FileDescriptor(error));
        }

        let stat = unsafe { stat.assume_init() };
        let size = match usize::try_from(stat.st_size) {
            Ok(size) => size,
            Err(error) => {
                unsafe { libc::close(fd) };
                return Err(error.into());
            },
        };

        if size < HEADER_SIZE {
            unsafe { libc::close(fd) };
            return Err(Error::IncompatibleHeader);
        }

        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };

        if ptr == libc::MAP_FAILED {
            let error = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(Error::Mmap(error));
        }

        Self::new(ptr.cast(), size, fd, name, false)
    }

    pub(crate) fn remap(&mut self, new_len: usize) -> Result<()> {
        let old_size = self.size;
        let old_file_size = libc::off_t::try_from(old_size)?;

        let file_size = libc::off_t::try_from(new_len)?;
        let result = unsafe { libc::ftruncate(self.fd, file_size) };

        if result != 0 {
            return Err(Error::FileDescriptor(std::io::Error::last_os_error()));
        }

        let new_ptr = unsafe { libc::mremap(self.start.as_ptr().cast(), self.size, new_len, MREMAP_MAYMOVE) };

        if new_ptr == libc::MAP_FAILED {
            let error = std::io::Error::last_os_error();
            unsafe {
                libc::ftruncate(self.fd, old_file_size);
            }
            return Err(Error::Mmap(error));
        }

        self.start = NonNull::new(new_ptr.cast::<u8>()).ok_or(Error::NullPtr)?;
        self.size = new_len;

        Ok(())
    }

    ///# Safety
    /// `self.start` must be:
    /// pointed to a valid place
    /// aligned for `T`
    /// have size_of::<T>() available
    pub(crate) unsafe fn write_bytes(&self, data: &[u8], offset: usize) -> Result<usize> {
        //check for out of bounds
        let end = offset.checked_add(data.len()).expect("write offset overflowed");
        if end < self.size {
            return Err(Error::OutOfBounds);
        }
        let ptr = unsafe { self.start.add(offset).as_ptr() };

        //Safety: This two arent even the smae memory (one is mmap and the othre is process normal
        //ram mem) so its safe
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()) };
        Ok(end)
    }
    //returns the offset where the ptr was left of
    pub(crate) unsafe fn write_concrete_type<T>(&self, data: T, offset: usize) -> Result<usize> {
        //check for out of bounds
        let end = offset.checked_add(size_of::<T>()).ok_or(Error::ArithmeticOverflow)?;

        if end <= self.size {
            return Err(Error::OutOfBounds);
        }

        let ptr = unsafe { self.start.add(offset).cast::<T>() };
        unsafe { ptr.write(data) }
        Ok(end)
    }

    pub(crate) unsafe fn read_bytes(&self, ammount: usize, offset: usize) -> Result<&[u8]> {
        //protect the read
        let end = offset.checked_add(ammount).ok_or(Error::ArithmeticOverflow)?;

        if end <= self.size {
            return Err(Error::OutOfBounds);
        }

        let ptr = unsafe { self.start.add(offset).as_ptr() };
        Ok(unsafe { std::slice::from_raw_parts(ptr, ammount) })
    }

    pub(crate) fn atomic_lock(&self) -> Result<&AtomicU32> {
        unsafe { self.start.as_ptr().cast::<AtomicU32>().as_ref().ok_or(Error::NullPtr) }
    }
    pub(crate) fn atomic_watchers(&self) -> Result<&AtomicU32> {
        unsafe { self.start.add(WATCHERS_OFFSET).as_ptr().cast::<AtomicU32>().as_ref().ok_or(Error::NullPtr) }
    }
    pub(crate) fn atomic_generation(&self) -> Result<&AtomicU32> {
        unsafe { self.start.add(GENERATION_OFFSET).as_ptr().cast::<AtomicU32>().as_ref().ok_or(Error::NullPtr) }
    }
    pub(crate) fn data_len(&mut self) -> Result<usize> {
        let bytes: [u8; 4] = unsafe { self.read_bytes(size_of::<u32>(), LEN_OFFSET)?.try_into()? };
        let len = usize::try_from(u32::from_be_bytes(bytes))?;
        let required_size = HEADER_SIZE.checked_add(len).ok_or(Error::ArithmeticOverflow)?;

        if required_size > self.size {
            // Grow this mapping
            let ptr = unsafe { libc::mremap(self.start.as_ptr().cast(), self.size, required_size, MREMAP_MAYMOVE) };
            if ptr == libc::MAP_FAILED || ptr.is_null() {
                return Err(Error::Mmap(std::io::Error::last_os_error()));
            }

            // Safety: `ptr` was checked above neither MAP_FAILED or its null
            self.start = unsafe { NonNull::new_unchecked(ptr.cast::<u8>()) };
            self.size = required_size;
        }
        Ok(len)
    }
    pub(crate) fn read_data<T>(&mut self) -> Result<T>
    where
        T: SharedData,
    {
        let len = self.data_len()?;
        let bytes = unsafe { self.read_bytes(len, START_OF_DATA_OFFSET)? };
        T::from_bytes(bytes)
    }

    pub(crate) fn write_data<T>(&mut self, data: &T) -> Result<()>
    where
        T: SharedData,
    {
        let bytes = data.as_bytes()?;
        let new_len = u32::try_from(bytes.len())?;

        let required_size = HEADER_SIZE.checked_add(bytes.len()).ok_or(Error::ArithmeticOverflow)?;

        if required_size > self.size {
            self.remap(required_size)?;
        }

        unsafe {
            self.write_bytes(&new_len.to_be_bytes(), LEN_OFFSET)?;
            self.write_bytes(&bytes, START_OF_DATA_OFFSET)?;
        };

        Ok(())
    }
    pub(crate) fn init_shared_memory(name: &str, size: usize) -> Result<Mapping> {
        let file_size = libc::off_t::try_from(size)?;
        let mem = unsafe {
            let name = CString::new(name)?;
            let fd = libc::shm_open(name.as_ptr(), libc::O_CREAT | libc::O_RDWR | libc::O_EXCL, 0o600);

            if fd < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EEXIST) {
                    return Err(Error::DuplicatedName(error));
                }
                return Err(Error::FileDescriptor(error));
            }

            let result = libc::ftruncate(fd, file_size);

            if result != 0 {
                let error = std::io::Error::last_os_error();
                close(fd);
                shm_unlink(name.as_ptr());
                return Err(Error::FileDescriptor(error));
            }

            let ptr = libc::mmap(ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);

            if ptr == libc::MAP_FAILED {
                let error = std::io::Error::last_os_error();
                close(fd);
                shm_unlink(name.as_ptr());
                return Err(Error::Mmap(error));
            }

            let bytes = ptr.cast::<u8>();

            Mapping::new(bytes, size, fd, name, true)?
        };

        Ok(mem)
    }
}
impl Drop for Mapping {
    #[inline]
    fn drop(&mut self) {
        //Safety: This is needed for the safe cleanup plus the pointers arent null here bcs it uses
        //[`NonNull`] type
        unsafe {
            let ptr = self.start.as_ptr() as *mut c_void;
            libc::munmap(ptr, self.size);
            libc::close(self.fd);

            if self.owner {
                shm_unlink(self.name.as_ptr());
            }
        }
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn undersized_header_is_rejected() {
        let name = format!("/ipc_com_short_header_{}", std::process::id());
        let _mapping = Mapping::init_shared_memory(&name, HEADER_SIZE - 1).unwrap();
        assert!(matches!(Mapping::new_connect(&name), Err(Error::IncompatibleHeader)));
    }

    #[test]
    fn failed_remap_restores_file_size_and_keeps_mapping_usable() {
        let name = format!("/ipc_com_failed_remap_{}", std::process::id());
        let mut mapping = Mapping::init_shared_memory(&name, HEADER_SIZE).unwrap();

        // Linux rejects a zero-length mremap after ftruncate has succeeded.
        assert!(matches!(mapping.remap(0), Err(Error::Mmap(_))));

        let mut stat = MaybeUninit::<libc::stat>::uninit();
        assert_eq!(unsafe { libc::fstat(mapping.fd, stat.as_mut_ptr()) }, 0);
        assert_eq!(unsafe { stat.assume_init() }.st_size, libc::off_t::try_from(HEADER_SIZE).unwrap());

        mapping.remap(HEADER_SIZE + 1).unwrap();
        unsafe { mapping.write_bytes(&[42], HEADER_SIZE).unwrap() };
        assert_eq!(unsafe { mapping.read_bytes(1, HEADER_SIZE).unwrap() }, &[42]);
    }
}
