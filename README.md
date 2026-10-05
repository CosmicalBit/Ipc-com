# ipc-com

Shared-memory IPC for Rust on Linux. Create a named value in one process, then open it by name in another. Values are encoded as bytes through the `SharedData` trait.

This crate is at version `0.1.0`. Its API and shared-memory format may change.

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

The repository also includes a [smaller example](examples/exemple1.rs), runnable with `cargo run --example exemple1`.

## Current limitations

- Linux only: the implementation uses POSIX shared-memory functions and Linux `mremap`.
- Choose a unique name for each independent value. Creating another value with an existing name can truncate and overwrite that shared-memory object.
- Keep the creator alive while other processes need to connect. Dropping any handle unlinks the name, so new readers may no longer be able to open it.
- A write that grows the value remaps the writer's handle, but does not remap existing reader handles. Keep the encoded size fixed when other handles are attached.
- `ReadOnly` restricts the Rust API; it is not an operating-system permission boundary. A reader can be converted to a writable handle with `to_mut()`.

Run the tests with `cargo test`.
