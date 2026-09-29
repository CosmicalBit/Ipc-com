use crate::ipc::{FixedSize, SharedHeader};
use std::ffi::NulError;
use std::os::fd::AsRawFd;
use std::os::raw::c_void;
use std::ptr::NonNull;
use std::ptr::{self, slice_from_raw_parts_mut};
use std::sync::atomic::AtomicBool;
use std::{ffi::CString, str::FromStr};

pub type Result<T> = std::result::Result<T, Error>;

pub enum Error {
    FdError,
    Mmap,
    Null(NulError),
    NullPtr,
}

impl From<NulError> for Error {
    fn from(error: NulError) -> Self {
        Self::Null(error)
    }
}

pub struct Mapping {
    start: u32,
    ptr: NonNull<u8>,
    size: u32,
    fd: i32,
}
//TODO add a show header funciton

impl Mapping {
    fn new(mut_ptr: *mut u8, size: u32, fd: i32) -> Result<Self> {
        Ok(Mapping {
            start: mut_ptr.addr() as u32,
            ptr: NonNull::new(mut_ptr).ok_or_else(|| Error::NullPtr)?,
            size,
            fd,
        })
    }
    pub fn reset_ptr(&mut self) {
        //the ptr was here how is this a error
        self.ptr = NonNull::new(self.start as *mut u8).unwrap();
    }

    pub fn ptr(&self) -> &NonNull<u8> {
        &self.ptr
    }

    pub fn size(&self) -> &u32 {
        &self.size
    }

    ///seeks a certain ptr position
    pub fn ptr_seek(&mut self, offset: usize) -> Result<()> {
        let ptr = NonNull::new(offset as *mut u8).ok_or_else(|| Error::NullPtr)?;
        self.ptr = ptr;

        Ok(())
    }

    ///# Safety
    /// `self.ptr` must be:
    /// pointed to a valid place
    /// aligned for `T`
    /// have size_of::<T>() available
    pub unsafe fn ptr_write<T>(&mut self, data: T) {
        let ptr = self.ptr.cast::<T>();
        unsafe { ptr.write(data) };
    }

    pub fn init_shared_mem(name: &str, size: u32) -> Result<Mapping> {
        let mem = unsafe {
            let name = CString::from_str(name)?;

            let fd = libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC);

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
    pub fn write_header(&self, header: SharedHeader) -> & mut SharedHeader {
        let mut head_ptr = self.ptr.cast::<SharedHeader>();

        unsafe {
            head_ptr.write(header);
            head_ptr.as_mut()
        }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        self.reset_ptr();
        unsafe {
            let ptr = self.ptr.as_ptr() as *mut c_void;
            libc::munmap(ptr, self.size as usize);
            libc::close(self.fd);
        }
    }
}
