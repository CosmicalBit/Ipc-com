//! Create, read, update, and wait for a shared value.
//!
//! Run with: `cargo run --example exemple1`

use std::{borrow::Cow, time::Duration};

use ipc_com::{Result, SharedData, SharedMemoryOptions, SharedValue};

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
        let day_bytes: [u8; 2] = bytes.try_into()?;
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
    let mut reader = SharedValue::<Moment>::open(&name)?;
    assert_eq!(reader.read()?, Moment { day: 30 });

    creator.write(&Moment { day: 31 })?;
    assert_eq!(reader.read()?, Moment { day: 31 });

    // Wait on a connected reader while the creator writes the next value.
    std::thread::scope(|scope| -> Result<()> {
        let waiting = scope.spawn(|| {
            let mut reader = SharedValue::<Moment>::open(&name)?;
            reader.wait_for_change_value()
        });
        std::thread::sleep(Duration::from_millis(50));
        creator.write(&Moment { day: 32 })?;
        assert_eq!(waiting.join().expect("wait thread panicked")?, Moment { day: 32 });
        Ok(())
    })?;

    // The async method returns a thread handle, so other work can continue
    // before joining it to get the updated value.
    let pending = reader.spawn_wait_for_change()?;
    std::thread::sleep(Duration::from_millis(50));
    creator.write(&Moment { day: 33 })?;
    assert_eq!(pending.join().expect("wait thread panicked")?, Moment { day: 33 });

    println!("Latest day read from shared memory: {}", reader.read()?.day);

    Ok(())
}
