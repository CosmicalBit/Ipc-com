use crate::{Error, Result};
use std::sync::atomic::AtomicU32;

pub(crate) struct Futex<'a> {
    value: &'a AtomicU32,
}

impl<'a> Futex<'a> {
    pub(crate) fn new(value: &'a AtomicU32) -> Self {
        Self { value }
    }
    ///its BLOCKING
    pub(crate) fn wait(&self, expected: u32) -> Result<()> {
        let ret = unsafe { libc::syscall(libc::SYS_futex, self.value as *const AtomicU32, libc::FUTEX_WAIT, expected, std::ptr::null::<libc::timespec>()) };

        if ret == -1 {
            let error = std::io::Error::last_os_error();
            return Err(Error::Os(error));
        }
        Ok(())
    }

    pub(crate) fn wake_all(&self) -> Result<()> {
        let ret = unsafe { libc::syscall(libc::SYS_futex, self.value as *const AtomicU32, libc::FUTEX_WAKE, i32::MAX) };
        if ret == -1 {
            let error = std::io::Error::last_os_error();
            return Err(Error::Os(error));
        }
        Ok(())
    }
}
