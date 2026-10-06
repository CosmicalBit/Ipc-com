//! Create and open a shared value with the default read-only access.
//!
//! Run with: `cargo run --example read_only`

use std::borrow::Cow;

use ipc_com::{Result, SharedData, SharedMemoryOptions, SharedValue};

#[derive(Debug, PartialEq, Eq)]
struct Version(u32);

impl SharedData for Version {
    fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
        Ok(Cow::Owned(self.0.to_be_bytes().to_vec()))
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; 4] = bytes.try_into()?;
        Ok(Self(u32::from_be_bytes(bytes)))
    }
}

fn main() -> Result<()> {
    let name = format!("ipc_com_version_{}", std::process::id());

    // SharedMemoryOptions<Version> defaults to read-only access.
    let options: SharedMemoryOptions<Version> = SharedMemoryOptions::new();
    let mut creator = options.name(&name).with_data(Version(1)).create()?;
    assert_eq!(creator.read()?, Version(1));

    // Another process can open the same name while the creator is alive.
    let mut reader = SharedValue::<Version>::open(&name)?;
    assert_eq!(reader.read()?, Version(1));

    println!("Shared version: {}", reader.read()?.0);
    Ok(())
}
