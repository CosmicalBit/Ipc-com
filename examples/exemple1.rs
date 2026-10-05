//! Create, read, update, and wait for a shared value.
//!
//! Run with: `cargo run --example exemple1`

use std::borrow::Cow;
use std::time::Duration;

use ipc_com::{Error, ReadOnly, Result, SharedData, SharedMemoryOptions, SharedValue};

#[derive(Debug, PartialEq, Eq)]
struct Moment {
    day: u16,
}

// Shared memory stores bytes, so each custom type must describe how to
// convert itself to bytes and back. Big-endian keeps the format explicit.
impl SharedData for Moment {
    fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
        Ok(Cow::Owned(self.day.to_be_bytes().to_vec()))
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let day_bytes: [u8; 2] = bytes.try_into().map_err(|_| Error::TryConversion)?;
        Ok(Self { day: u16::from_be_bytes(day_bytes) })
    }
}

fn main() -> Result<()> {
    // Use a unique name so repeated runs do not collide with each other.
    let name = format!("ipc_com_moment_{}", std::process::id());

    // The creator allows updates to the initial value.
    let mut creator = SharedMemoryOptions::new().to_mutable().name(&name).with_data(Moment { day: 30 }).create()?;

    // A separate process could use the same name to connect. Here we open a
    // second handle in this process so the whole example runs in one command.
    let reader = SharedValue::<Moment, ReadOnly>::new_reader(&name)?;
    assert_eq!(reader.read()?, Moment { day: 30 });

    // A reader can also become a writer. Keep the creator alive so the name
    // remains available while the second handle is open.
    let mut writer = SharedValue::<Moment, ReadOnly>::new_reader(&name)?.to_mut();
    writer.write(Moment { day: 31 })?;
    assert_eq!(reader.read()?, Moment { day: 31 });

    // The blocking wait returns the value written after the wait begins.
    std::thread::scope(|scope| -> Result<()> {
        let update = scope.spawn(|| {
            let mut writer = SharedValue::<Moment, ReadOnly>::new_reader(&name)?.to_mut();
            std::thread::sleep(Duration::from_millis(50));
            writer.write(Moment { day: 32 })
        });
        assert_eq!(reader.wait_for_change_value()?, Moment { day: 32 });
        update.join().expect("writer thread panicked")?;
        Ok(())
    })?;

    // The async method returns a thread handle, so other work can continue
    // before joining it to get the updated value.
    let pending = reader.wait_for_change_async::<()>()?;
    std::thread::sleep(Duration::from_millis(50));
    creator.write(Moment { day: 33 })?;
    assert_eq!(pending.join().expect("wait thread panicked")?, Moment { day: 33 });

    println!("Latest day read from shared memory: {}", reader.read()?.day);

    Ok(())
}
