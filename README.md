# ipc-com

Shared-memory Inter Process Communication for Rust on Linux. Create a named value in one process, then open it by name in another. Values are encoded as bytes through the `SharedData` trait.

This crate is at version `0.1.2`. Its API and shared-memory format may change.

## Quick start

Add the crate to your project:

```toml
[dependencies]
ipc-com = "0.1"
```

Define how your type is encoded, create a value, and connect a second handle:

```rust
use std::borrow::Cow;
use ipc_com::{Error, ReadOnly, Result, SharedData, SharedMemoryOptions, SharedValue};

#[derive(Debug, PartialEq, Eq)]
struct Counter(u64);

impl SharedData for Counter {
    fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
        Ok(Cow::Owned(self.0.to_be_bytes().to_vec()))
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; 8] = bytes.try_into().map_err(|_| Error::TryConversion)?;
        Ok(Self(u64::from_be_bytes(bytes)))
    }
}

fn main() -> Result<()> {
    let name = format!("ipc_com_counter_{}", std::process::id());

    let mut owner = SharedMemoryOptions::new()
        .to_mutable()
        .name(&name)
        .with_data(Counter(1))
        .create()?;

    let reader = SharedValue::<Counter, ReadOnly>::new_reader(&name)?;
    assert_eq!(reader.read()?, Counter(1));

    owner.write(Counter(2))?;
    assert_eq!(reader.read()?, Counter(2));
    Ok(())
}
```

The example opens both handles in one process. For IPC, create the value in one process and call `SharedValue::<YourType, ReadOnly>::new_reader(name)` in another process while the creator is still alive. Both processes must use the same name and the same encoding. Names may be passed with or without a leading `/`.

For a value that does not need updates, omit `.to_mutable()`. The resulting handle can still call `read()`, but does not expose `write()`.

To wait for an update, call `reader.wait_for_change_value()?` or use
`reader.wait_for_change_async::<()>()?` and join the returned thread handle.
An existing reader can be turned into a writer with `reader.to_mut()`.
See the [complete example](examples/exemple1.rs), runnable with `cargo run --example exemple1`.
