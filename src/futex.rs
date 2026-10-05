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
        let ret = unsafe { libc::syscall(libc::SYS_futex, self.value.as_ptr(), libc::FUTEX_WAIT, expected, std::ptr::null::<libc::timespec>()) };

        if ret == -1 {
            let error = std::io::Error::last_os_error();
            // A changed value or a signal only means the caller must check
            // the generation again. FUTEX_WAIT can also wake spuriously.
            if matches!(error.raw_os_error(), Some(libc::EAGAIN | libc::EINTR)) {
                return Ok(());
            }
            return Err(Error::Futex(error));
        }
        Ok(())
    }

    pub(crate) fn wake_all(&self) -> Result<()> {
        let ret = unsafe { libc::syscall(libc::SYS_futex, self.value.as_ptr(), libc::FUTEX_WAKE, i32::MAX) };
        if ret == -1 {
            let error = std::io::Error::last_os_error();
            return Err(Error::Futex(error));
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn changed_expected_value_is_retryable() {
        let value = AtomicU32::new(1);
        let futex = Futex::new(&value);
        futex.wait(0).unwrap();
        futex.wake_all().unwrap();
    }
}
