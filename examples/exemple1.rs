//! Create a value in shared memory, then read it through a second handle.
//!
//! Run with: `cargo run --example exemple1`

use std::borrow::Cow;

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

    // The creator initializes a read-only shared value with day 30.
    let creator = SharedMemoryOptions::new().name(&name).with_data(Moment { day: 30 }).create()?;

    // A separate process could use the same name to connect. Here we open a
    // second handle in this process so the whole example runs in one command.
    let reader = SharedValue::<Moment, ReadOnly>::new_reader(&name)?;
    let moment = reader.read()?;

    println!("Day read from shared memory: {}", moment.day);

    // Keep the creator alive until the reader is finished with the mapping.
    drop(reader);
    drop(creator);
    Ok(())
}
