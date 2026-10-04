use std::ffi::NulError;
use std::intrinsics::copy_nonoverlapping;
use std::os::raw::c_void;
use std::ptr;
use std::ptr::NonNull;
use std::sync::atomic::AtomicU64;
use std::{ffi::CString, str::FromStr};

use libc::MREMAP_MAYMOVE;

use crate::allocation::{AccessLayout, ReadWrite};
use crate::shared_value::SharedData;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    FdError,
    Mmap,
    Null(NulError),
    NullPtr,
    TryError,
    DifferentLenghsSameMem,
}

impl From<NulError> for Error {
    fn from(error: NulError) -> Self {
        Self::Null(error)
    }
}

pub struct Mapping {
    start: NonNull<u8>,
    ptr: NonNull<u8>,
    size: usize,
    fd: i32,
}
//TODO add a show header funciton

impl Mapping {
    fn new(mut_ptr: *mut u8, size: usize, fd: i32) -> Result<Self> {
        Ok(Mapping {
            start: NonNull::new(mut_ptr).ok_or(Error::NullPtr)?,
            ptr: NonNull::new(mut_ptr).ok_or(Error::NullPtr)?,
            size,
            fd,
        })
    }

    pub fn remap(&mut self, new_len: usize) -> Result<()> {
        let result = unsafe { libc::ftruncate(self.fd, new_len as libc::off_t) };

        if result != 0 {
            return Err(Error::FdError);
        }

        let new_ptr = unsafe { libc::mremap(self.start.as_ptr().cast(), self.size, new_len, MREMAP_MAYMOVE) };

        if new_ptr == libc::MAP_FAILED {
            return Err(Error::Mmap);
        }

        self.start = NonNull::new(new_ptr.cast::<u8>()).ok_or(Error::NullPtr)?;
        self.size = new_len;

        Ok(())
    }
    ///# Safety
    /// `self.ptr` must be:
    /// pointed to a valid place
    /// aligned for `T`
    /// have size_of::<T>() available
    pub unsafe fn write_bytes(&self, data: &[u8], offset: usize) -> usize {
        //check for out of bounds
        let end = offset.checked_add(data.len()).expect("write offset overflowed");
        assert!(end <= self.size, "write exceeded mapping");

        let ptr = unsafe { self.start.add(offset).as_ptr() };
        unsafe { copy_nonoverlapping(data.as_ptr(), ptr, data.len()) };
        offset + data.len()
    }
    //returns the offset where the ptr was left of
    pub unsafe fn write_concrete_type<T>(&self, data: T, offset: usize) -> usize {
        //check for out of bounds
        let end = offset.checked_add(size_of::<T>()).expect("write offset overflowed");
        assert!(end <= self.size, "write exceeded mapping");

        let ptr = unsafe { self.start.add(offset).cast::<T>() };
        unsafe { ptr.write(data) }
        offset + size_of::<T>()
    }

    pub unsafe fn read_bytes(&self, ammount: usize, offset: usize) -> &[u8] {
        //protect the read
        let end = offset.checked_add(ammount).expect("read out of bounds");
        assert!(end <= self.size, "read excedded mapping");

        let ptr = unsafe { self.start.add(offset).as_ptr() };
        unsafe { std::slice::from_raw_parts(ptr, ammount) }
    }
    pub fn atomic_ref(&self) -> Result<&AtomicU64> {
        unsafe { self.start.as_ptr().cast::<AtomicU64>().as_ref().ok_or(Error::NullPtr) }
    }
    pub fn read_data<T>(&self) -> Result<T>
    where
        T: SharedData,
    {
        let bytes: [u8; 4] = unsafe { self.read_bytes(size_of::<u32>(), size_of::<AtomicU64>()).try_into().unwrap() };
        let len = u32::from_be_bytes(bytes) as usize;
        let bytes = unsafe { self.read_bytes(len, size_of::<AtomicU64>() + size_of::<u32>()) };

        let data = T::from_bytes(bytes).map_err(|_| Error::TryError)?;
        Ok(data)
    }

    pub fn write_data<T>(&mut self, data: T, _atomic: AtomicU64) -> Result<()>
    where
        T: SharedData,
    {
        let bytes: [u8; 4] = unsafe { self.read_bytes(size_of::<u32>(), size_of::<AtomicU64>()).try_into().unwrap() };
        let len = u32::from_be_bytes(bytes) as usize;

        let offset = size_of::<AtomicU64>() + size_of::<u32>();

        let bytes = data.as_bytes()?;

        let required_size = ReadWrite::prefix_size() + bytes.len();

        if required_size > self.size {
            self.remap(required_size)?;
        }

        let new_len = bytes.len() as u32;

        unsafe {
            self.write_bytes(&new_len.to_be_bytes(), size_of::<AtomicU64>());
            self.write_bytes(&bytes, offset);
        };

        Ok(())
    }
    pub fn init_shared_mem(name: &str, size: usize) -> Result<Mapping> {
        let mem = unsafe {
            let name = CString::from_str(name)?;

            let fd = libc::shm_open(name.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o600);

            if fd < 0 {
                return Err(Error::FdError);
            }

            let result = libc::ftruncate(fd, size as libc::off_t);

            if result != 0 {
                return Err(Error::FdError);
            }

            let ptr = libc::mmap(ptr::null_mut(), size as usize, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);

            if ptr == libc::MAP_FAILED {
                return Err(Error::Mmap);
            }

            let bytes = ptr.cast::<u8>();

            Mapping::new(bytes, size, fd)?
        };

        Ok(mem)
    }
}
pub(crate) fn aligned_offset<T>(offset: usize) -> usize {
    offset.next_multiple_of(align_of::<T>())
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            let ptr = self.start.as_ptr() as *mut c_void;
            libc::munmap(ptr, self.size as usize);
            libc::close(self.fd);
        }
    }
}
