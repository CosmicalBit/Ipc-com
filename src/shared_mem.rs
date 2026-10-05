use libc::{MREMAP_MAYMOVE, close, shm_unlink};
use std::ffi::NulError;
use std::mem::MaybeUninit;
use std::os::raw::c_void;
use std::ptr;
use std::ptr::NonNull;
use std::sync::atomic::AtomicU32;
use std::{ffi::CString, str::FromStr};

use crate::allocation::HEADER_SIZE;
use crate::shared_value::SharedData;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    FileDescriptor,
    Mmap,
    Null(NulError),
    NullPtr,
    TryConversion,
    DuplicatedName(std::io::Error),
    Futex(std::io::Error),
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
    name: String,
}
//TODO add a show header funciton

impl Mapping {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    fn new(mut_ptr: *mut u8, size: usize, fd: i32, name: &str, owner: bool) -> Result<Self> {
        Ok(Mapping {
            start: NonNull::new(mut_ptr).ok_or(Error::NullPtr)?,
            size,
            fd,
            name: name.to_string(),
            owner,
        })
    }
    pub(crate) fn new_connect(name: &str) -> Result<Self> {
        let name = CString::new(name)?;

        let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, 0) };

        if fd == -1 {
            return Err(Error::FileDescriptor);
        }

        let mut stat = MaybeUninit::<libc::stat>::uninit();

        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } == -1 {
            unsafe { libc::close(fd) };
            return Err(Error::FileDescriptor);
        }

        let stat = unsafe { stat.assume_init() };
        let size = stat.st_size as usize;

        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };

        if ptr == libc::MAP_FAILED {
            unsafe { libc::close(fd) };
            return Err(Error::Mmap);
        }

        Self::new(ptr.cast(), size, fd, name.to_str().unwrap(), false)
    }

    pub(crate) fn remap(&mut self, new_len: usize) -> Result<()> {
        let result = unsafe { libc::ftruncate(self.fd, new_len as libc::off_t) };

        if result != 0 {
            return Err(Error::FileDescriptor);
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
    /// `self.start` must be:
    /// pointed to a valid place
    /// aligned for `T`
    /// have size_of::<T>() available
    pub(crate) unsafe fn write_bytes(&self, data: &[u8], offset: usize) -> usize {
        //check for out of bounds
        let end = offset.checked_add(data.len()).expect("write offset overflowed");
        assert!(end <= self.size, "write exceeded mapping");

        let ptr = unsafe { self.start.add(offset).as_ptr() };
        unsafe { ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len()) };
        offset + data.len()
    }
    //returns the offset where the ptr was left of
    pub(crate) unsafe fn write_concrete_type<T>(&self, data: T, offset: usize) -> usize {
        //check for out of bounds
        let end = offset.checked_add(size_of::<T>()).expect("write offset overflowed");
        assert!(end <= self.size, "write exceeded mapping");

        let ptr = unsafe { self.start.add(offset).cast::<T>() };
        unsafe { ptr.write(data) }
        offset + size_of::<T>()
    }

    pub(crate) unsafe fn read_bytes(&self, ammount: usize, offset: usize) -> &[u8] {
        //protect the read
        let end = offset.checked_add(ammount).expect("read out of bounds");
        assert!(end <= self.size, "read excedded mapping");

        let ptr = unsafe { self.start.add(offset).as_ptr() };
        unsafe { std::slice::from_raw_parts(ptr, ammount) }
    }
    pub(crate) fn atomic_ref(&self) -> Result<&AtomicU32> {
        unsafe { self.start.as_ptr().cast::<AtomicU32>().as_ref().ok_or(Error::NullPtr) }
    }
    pub(crate) fn atomic_gen(&self) -> Result<&AtomicU32> {
        unsafe { self.start.add(size_of::<AtomicU32>()).as_ptr().cast::<AtomicU32>().as_ref().ok_or(Error::NullPtr) }
    }
    pub(crate) fn read_data<T>(&self) -> Result<T>
    where
        T: SharedData,
    {
        let bytes: [u8; 4] = unsafe { self.read_bytes(size_of::<u32>(), size_of::<AtomicU32>() + size_of::<AtomicU32>()).try_into().unwrap() };
        let len = u32::from_be_bytes(bytes) as usize;
        let required_size = HEADER_SIZE.checked_add(len).ok_or(Error::TryConversion)?;
        let data = if required_size <= self.size {
            let bytes = unsafe { self.read_bytes(len, HEADER_SIZE) };
            T::from_bytes(bytes)
        } else {
            // A reader may have connected before a writer expanded the object.
            // Map the larger payload while the caller holds the shared lock.
            let ptr = unsafe { libc::mmap(ptr::null_mut(), required_size, libc::PROT_READ, libc::MAP_SHARED, self.fd, 0) };
            if ptr == libc::MAP_FAILED {
                return Err(Error::Mmap);
            }
            let bytes = unsafe { std::slice::from_raw_parts((ptr as *const u8).add(HEADER_SIZE), len) };
            let data = T::from_bytes(bytes);
            unsafe { libc::munmap(ptr, required_size) };
            data
        }
        .map_err(|_| Error::TryConversion)?;
        Ok(data)
    }

    pub(crate) fn write_data<T>(&mut self, data: &T) -> Result<()>
    where
        T: SharedData,
    {
        let bytes = data.as_bytes()?;

        let required_size = HEADER_SIZE + bytes.len();

        if required_size > self.size {
            self.remap(required_size)?;
        }

        let new_len = bytes.len() as u32;

        unsafe {
            self.write_bytes(&new_len.to_be_bytes(), size_of::<AtomicU32>() * 2);
            self.write_bytes(&bytes, HEADER_SIZE);
        };

        Ok(())
    }
    pub(crate) fn init_shared_memory(name: &str, size: usize) -> Result<Mapping> {
        let mem = unsafe {
            let name = CString::from_str(name)?;

            let fd = libc::shm_open(name.as_ptr(), libc::O_CREAT | libc::O_RDWR | libc::O_EXCL, 0o600);

            if fd < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EEXIST) {
                    return Err(Error::DuplicatedName(error));
                }
                return Err(Error::FileDescriptor);
            }

            let result = libc::ftruncate(fd, size as libc::off_t);

            if result != 0 {
                close(fd);
                shm_unlink(name.as_ptr());
                return Err(Error::FileDescriptor);
            }

            let ptr = libc::mmap(ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);

            if ptr == libc::MAP_FAILED {
                close(fd);
                shm_unlink(name.as_ptr());
                return Err(Error::Mmap);
            }

            let bytes = ptr.cast::<u8>();

            Mapping::new(bytes, size, fd, &name.to_string_lossy(), true)?
        };

        Ok(mem)
    }
}
impl Drop for Mapping {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            let ptr = self.start.as_ptr() as *mut c_void;
            libc::munmap(ptr, self.size);
            libc::close(self.fd);

            if self.owner {
                let name = CString::new(self.name.as_str()).expect("impossible cstring conversion");
                shm_unlink(name.as_ptr());
            }
        }
    }
}
