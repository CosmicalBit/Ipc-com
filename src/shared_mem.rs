use libc::{MREMAP_MAYMOVE, close, shm_unlink};
use std::ffi::CString;
use std::ffi::NulError;
use std::mem::MaybeUninit;
use std::os::raw::c_void;
use std::ptr;
use std::ptr::NonNull;
use std::sync::atomic::AtomicU32;

use crate::allocation::HEADER_SIZE;
use crate::shared_value::SharedData;

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
    DuplicatedName(std::io::Error),
    Futex(std::io::Error),
}
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
//TODO add a show header funciton

impl Mapping {
    pub(crate) fn name(&self) -> &CString {
        &self.name
    }
    fn new(mut_ptr: *mut u8, size: usize, fd: i32, name: &str, owner: bool) -> Result<Self> {
        Ok(Mapping {
            start: NonNull::new(mut_ptr).ok_or(Error::NullPtr)?,
            size,
            fd,
            name: CString::new(name)?,
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

        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };

        if ptr == libc::MAP_FAILED {
            let error = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(Error::Mmap(error));
        }

        Self::new(ptr.cast(), size, fd, name.to_str().unwrap(), false)
    }

    pub(crate) fn remap(&mut self, new_len: usize) -> Result<()> {
        let file_size = libc::off_t::try_from(new_len)?;
        let result = unsafe { libc::ftruncate(self.fd, file_size) };

        if result != 0 {
            return Err(Error::FileDescriptor(std::io::Error::last_os_error()));
        }

        let new_ptr = unsafe { libc::mremap(self.start.as_ptr().cast(), self.size, new_len, MREMAP_MAYMOVE) };

        if new_ptr == libc::MAP_FAILED {
            return Err(Error::Mmap(std::io::Error::last_os_error()));
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

        //Safety: This two arent even the smae memory (one is mmap and the othre is process normal
        //ram mem) so its safe
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
    pub(crate) fn read_data<T>(&mut self) -> Result<T>
    where
        T: SharedData,
    {
        let bytes: [u8; 4] = unsafe { self.read_bytes(size_of::<u32>(), size_of::<AtomicU32>() + size_of::<AtomicU32>()).try_into().unwrap() };
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
        let bytes = unsafe { self.read_bytes(len, HEADER_SIZE) };
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
            self.write_bytes(&new_len.to_be_bytes(), size_of::<AtomicU32>().checked_mul(2).ok_or(Error::ArithmeticOverflow)?);
            self.write_bytes(&bytes, HEADER_SIZE);
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

            Mapping::new(bytes, size, fd, &name.to_string_lossy(), true)?
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
