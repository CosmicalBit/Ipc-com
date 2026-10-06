use crate::{Error, Result};
use std::sync::atomic::AtomicU32;
use std::time::Duration;

pub(crate) struct Futex<'a> {
    value: &'a AtomicU32,
}

pub(crate) enum WaitResult {
    Woken,
    TimedOut,
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
    pub(crate) fn wait_timeout(&self, expected: u32, timeout: Duration) -> Result<WaitResult> {
        let timeout = libc::timespec {
            tv_sec: libc::time_t::try_from(timeout.as_secs())?,
            tv_nsec: libc::c_long::from(i32::try_from(timeout.subsec_nanos())?),
        };

        let ret = unsafe { libc::syscall(libc::SYS_futex, self.value.as_ptr(), libc::FUTEX_WAIT, expected, &timeout) };

        if ret == 0 {
            return Ok(WaitResult::Woken);
        }

        let error = std::io::Error::last_os_error();

        match error.raw_os_error() {
            Some(libc::ETIMEDOUT) => Ok(WaitResult::TimedOut),

            // Value changed before we entered the kernel.
            Some(libc::EAGAIN) => Ok(WaitResult::Woken),

            // Interrupted by signal; just retry.
            Some(libc::EINTR) => Ok(WaitResult::Woken),

            _ => Err(Error::Futex(error)),
        }
    }
}

#[cfg(all(test, not(miri)))]
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
