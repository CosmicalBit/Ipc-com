//! Run after sender: cargo run --release --example receiver
use std::borrow::Cow;

use ipc_com::{Result, SharedData, SharedValue};

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
    let mut value = SharedValue::<Counter>::open(NAME)?;
    println!("current: {}", value.read()?.0);
    loop {
        println!("received: {}", value.wait_for_change_value()?.0);
    }
}
