use crate::ipc::SharedHeader;
use std::ffi::NulError;
use std::os::raw::c_void;
use std::ptr;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;
use std::{ffi::CString, str::FromStr};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq)]
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
    start: NonNull<u8>,
    ptr: NonNull<u8>,
    size: u32,
    fd: i32,
}
//TODO add a show header funciton

impl Mapping {
    fn new(mut_ptr: *mut u8, size: u32, fd: i32) -> Result<Self> {
        Ok(Mapping {
            start: NonNull::new(mut_ptr).ok_or_else(|| Error::NullPtr)?,
            ptr: NonNull::new(mut_ptr).ok_or_else(|| Error::NullPtr)?,
            size,
            fd,
        })
    }

    pub fn reset_ptr(&mut self) {
        self.ptr = NonNull::new(self.start.as_ptr()).unwrap();
    }

    pub fn ptr(&self) -> &NonNull<u8> {
        &self.ptr
    }

    pub fn header(&self) -> &SharedHeader {
        //the ptr was here how is this a error
        unsafe { self.start.cast::<SharedHeader>().as_ref() }
    }

    pub fn size(&self) -> &u32 {
        &self.size
    }

    /// Set the cursor to a byte offset within this mapping.
    pub unsafe fn ptr_seek(&mut self, address: usize) -> Result<()> {
        self.ptr = NonNull::new(address as *mut u8).ok_or_else(|| Error::NullPtr)?;

        Ok(())
    }
    pub fn ptr_from_addr<T>(&self, address: usize) -> Result<NonNull<T>> {
        NonNull::new(address as *mut T).ok_or_else(|| Error::NullPtr)
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
    pub fn write_header(&self, header: SharedHeader) -> &mut SharedHeader {
        let mut head_ptr = self.ptr.cast::<SharedHeader>();

        unsafe {
            head_ptr.write(header);
            head_ptr.as_mut()
        }
    }
    pub unsafe fn write<T>(&mut self, offset: usize, data: T) -> Result<()> {
        self.ptr_seek(offset)?;
        unsafe {
            self.ptr.cast::<T>().write(data);
        }
        Ok(())
    }

    pub fn attomic_bool_slice(&mut self) -> &[AtomicBool] {
        let bitmap = self.header().bitmap();
        self.ptr_from_addr::<&[AtomicBool]>(bitmap.offset as usize);

        unsafe { std::slice::from_raw_parts(bitmap.offset as *const AtomicBool, bitmap.len as usize) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            self.reset_ptr();
            let ptr = self.ptr.as_ptr() as *mut c_void;
            libc::munmap(ptr, self.size as usize);
            libc::close(self.fd);
        }
    }
}
