use crate::ipc::{FixedSize, SharedHeader};
use std::ffi::NulError;
use std::os::fd::AsRawFd;
use std::os::raw::c_void;
use std::ptr::NonNull;
use std::ptr::{self, slice_from_raw_parts_mut};
use std::{ffi::CString, str::FromStr};

pub type Result<T> = std::result::Result<T, Error>;

pub enum Error {
    FdError,
    Mmap,
    Null(NulError),
}

impl From<NulError> for Error {
    fn from(error: NulError) -> Self {
        Self::Null(error)
    }
}

pub struct Mapping {
    ptr: *mut u8,
    size: usize,
    fd: i32,
}

impl Mapping {
    fn new(mut_ptr: *mut u8, size: usize, fd: i32) -> Self {
        Mapping { ptr: mut_ptr, size, fd }
    }
    pub fn init_shared_mem(name: &str, size: usize) -> Result<Mapping> {
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

            let ptr = libc::mmap(ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);

            if ptr == libc::MAP_FAILED {
                return Err(Error::Mmap);
            }

            let bytes = ptr.cast::<u8>();

            Mapping::new(bytes, size, fd)
        };

        Ok(mem)
    }
    pub fn write_header(&self, header: SharedHeader) -> &SharedHeader {
        let head_ptr = unsafe { self.ptr.cast::<SharedHeader>() };

        unsafe {
            head_ptr.write(header);
            &*head_ptr
        }
    }
    pub fn change_data<T,R>(handle: T, f: impl FnOnce(&mut T));

    
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            let ptr = self.ptr as *mut c_void;
            libc::munmap(ptr, self.size);
            libc::close(self.fd);
        }
    }
}
