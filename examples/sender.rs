//! Run first: cargo run --release --example sender
use std::{borrow::Cow, thread, time::Duration};

use ipc_com::{Result, SharedData, SharedMemoryOptions};

const NAME: &str = "ipc_com_example_counter";

struct Counter(u64);

impl SharedData for Counter {
    fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
        Ok(Cow::Owned(self.0.to_be_bytes().to_vec()))
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(Self(u64::from_be_bytes(bytes.try_into()?)))
    }
}

fn main() -> Result<()> {
    let mut value = SharedMemoryOptions::new().name(NAME).with_data(Counter(0)).to_mutable().create()?;
    println!("Sharing {NAME}; run the receiver in another terminal");
    for n in 1.. {
        thread::sleep(Duration::from_secs(1));
        value.write(&Counter(n))?;
        println!("sent {n}");
    }
    Ok(())
}
