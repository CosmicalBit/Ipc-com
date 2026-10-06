# ipc-com

`ipc-com` is a Linux shared-memory IPC crate for sharing values between processes.

It uses POSIX shared memory for the data and futexes for synchronization, so processes can read, write and wait for changes without continuously polling. Shared values can also grow after creation, and existing readers will remap themselves when needed.

## Features

- Named shared values that can be opened from another process
- Read-only by default, with an explicit move into writable access
- Multiple processes can write to the same shared value
- Futex-based locking instead of spin loops
- Blocking notifications when a value changes
- Variable-sized values can grow after creation
- Detection of a process dying while holding the lock
- Explicit recovery from a dead lock owner
- Custom serialization through `SharedData`
- Only one runtime dependency: `libc`

## Quick start

Add the crate:

```sh
cargo add ipc-com
```

A type only needs to describe how it becomes bytes and how to build it back from those bytes:

```rust
use std::borrow::Cow;

use ipc_com::{
    Result,
    SharedData,
    SharedMemoryOptions,
    SharedValue,
};

#[derive(Debug, PartialEq, Eq)]
struct Counter(u64);

impl SharedData for Counter {
    fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
        Ok(Cow::Owned(self.0.to_be_bytes().to_vec()))
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; 8] = bytes.try_into()?;
        Ok(Self(u64::from_be_bytes(bytes)))
    }
}

fn main() -> Result<()> {
    let name = format!("counter_{}", std::process::id());

    let mut owner = SharedMemoryOptions::new()
        .to_mutable()
        .name(&name)
        .with_data(Counter(1))
        .create()?;

    let mut reader = SharedValue::<Counter>::open(&name)?;

    owner.write(&Counter(2))?;

    assert_eq!(reader.read()?, Counter(2));

    Ok(())
}
```

The example uses two handles in one process because it fits nicely in one snippet. Normally `SharedValue::open()` would be called from another process using the same name.

Names may be passed with or without the leading `/`. Processes sharing a value must use the same `SharedData` encoding and compatible crate versions.

## Creating a value

A shared value is created with `SharedMemoryOptions`:

```rust
let value = SharedMemoryOptions::new()
    .name("config")
    .with_data(config)
    .create()?;
```

Values are read-only by default.

If the creator also needs to write:

```rust
let mut value = SharedMemoryOptions::new()
    .to_mutable()
    .name("config")
    .with_data(config)
    .create()?;
```

The builder keeps track of whether the name and data have been provided, so `create()` only becomes available once there is actually enough information to create something.

No `Missing`, `Present`, `ReadOnly` or `ReadWrite` types are needed in normal user code.

## Opening a value

Another process opens an existing value with:

```rust
let mut value = SharedValue::<Config>::open("config")?;

let config = value.read()?;
```

`SharedValue<T>` is read-only by default.

If that process also wants to write, the handle can be turned into a writable one:

```rust
let mut value = SharedValue::<Config>::open("config")?
    .into_mutable();

value.write(&new_config)?;
```

`into_mutable()` consumes the old handle and returns a writable one.

So this:

```rust
let value = SharedValue::<Config>::open("config")?;
```

cannot call `write()`, while this can:

```rust
let mut value = value.into_mutable();

value.write(&new_config)?;
```

The access mode is kept in the type instead of being checked through a runtime boolean. It controls which methods Rust exposes; the shared-memory mapping itself is currently opened with write permission.

## Waiting for changes

A process can block until somebody successfully writes a new value:

```rust
let value = reader.wait_for_change_value()?;
```

This does not sit in a loop constantly checking memory. The reader sleeps on a futex associated with the generation counter and wakes when a writer changes it.

There is also:

```rust
let pending = reader.wait_for_change_async()?;

// do other work

let value = pending
    .join()
    .expect("wait thread panicked")?;
```

`wait_for_change_async()` currently uses a blocking thread and returns its `JoinHandle`. It is not an async-runtime `Future`.

## SharedData

Shared memory contains bytes, so the crate needs to know how your type maps to them:

```rust
pub trait SharedData: Sized {
    fn as_bytes(&self) -> Result<Cow<'_, [u8]>>;
    fn from_bytes(bytes: &[u8]) -> Result<Self>;
}
```

`ipc-com` does not copy the raw memory layout of your struct into shared memory and hope the layout gods are feeling generous.

You choose the representation.

For a number that might simply be:

```rust
Cow::Owned(self.value.to_be_bytes().to_vec())
```

For something with variable-sized fields you can make your own little format:

```text
name length
name bytes
age
whatever else you put in there
```

As long as `from_bytes()` knows how to undo what `as_bytes()` did, the crate does not care what the format looks like.

`Cow<[u8]>` also means a type can borrow an existing byte representation instead of allocating a new buffer when possible.

## What is actually shared

The current shared-memory layout is small:

```text
+----------------------+
| lock owner PID       |
+----------------------+
| generation           |
+----------------------+
| payload length       |
+----------------------+
|                      |
| encoded value ...    |
|                      |
+----------------------+
```

The lock and generation are atomic `u32`s.

The lock contains `0` while free and the PID of the process that currently owns it while locked.

A write roughly goes through:

```text
acquire lock
    |
grow mapping if needed
    |
write length + data
    |
unlock
    |
advance generation
    |
wake change waiters
```

A reader takes the same lock while reading, so it cannot normally observe half of one write and half of another.

If the stored value has grown since that process last looked at it, the reader grows its mapping before reading the payload.

That is why `read()` takes `&mut self`: reading the shared value itself does not change it, but the local mapping may need updating.

## Futexes

The lock is atomic, but a process that loses the lock does not need to burn a CPU core repeatedly loading it.

It can sleep through Linux `futex` until the lock changes.

The generation counter uses the same idea for change notifications:

```text
generation = 12
      |
reader waits on 12
      |
writer writes something
      |
generation = 13
      |
wake readers
```

The reader checks the generation again after waking, so interrupted and stale futex waits are fine.

## When a process dies with the lock

The lock stores the PID of its owner.

If another process has been waiting on the same owner for long enough, `ipc-com` checks whether that PID still exists.

If it does not:

```rust
Error::OwnerDied
```

is returned.

The crate deliberately does not automatically unlock the value. A process could have died halfway through changing the payload, and pretending everything is definitely fine would be a fairly dangerous guess.

For applications that know how their data should be recovered, there are explicit unsafe recovery operations:

```rust
unsafe {
    value.force_unlock()?;
    value.force_awake()?;
}
```

They are unsafe because the application has to make sure recovery is coordinated and that the shared value is in a usable state before other processes continue.

## Linux

`ipc-com` currently targets Linux and uses Linux/POSIX primitives directly:

```text
shm_open
mmap / mremap
futex
kill(pid, 0)
```

Processes opening the same value also need to use the same `SharedData` encoding. Synchronizing six bytes is easy; deciding whether those six bytes are an integer, a name or half a JPEG is thankfully not the lock's job.

## Examples

For a real two-process example, start the [sender](examples/sender.rs) and then
the [receiver](examples/receiver.rs) in separate terminals:

```sh
cargo run --release --example sender
cargo run --release --example receiver
```

The sender updates a named counter once per second. The receiver opens it and
prints each change returned by `wait_for_change_value()`. Stop both with Ctrl-C.

The [writable example](examples/exemple1.rs) shows updates and change notifications:

```sh
cargo run --example exemple1
```

The [read-only example](examples/read_only.rs) shows the default access mode:

```sh
cargo run --example read_only
```

## Profiling and safety tests

```sh
./scripts/perf.sh stat --payload 64
./scripts/perf.sh record --payload 65536
./scripts/perf.sh report
./scripts/test.sh
```

The profiling example uses two processes and checks every request and response.
`--payload` includes its 8-byte sequence and defaults to 64 bytes;
`--iterations` defaults to 100,000. The profiling build keeps release
optimizations and includes debug symbols. `perf record` writes `perf.data`,
which can also be opened in Hotspot. The Linux `perf` tool and profiling
permissions must be set up on the host.

`./scripts/test.sh` runs normal tests, five deterministic randomized-layout
seeds, AddressSanitizer with layout seed 17, and three Miri seeds. Run a single
mode with `normal`, `layout`, `asan`, or `miri`. Plain `cargo test` stays fast
and uses the normal toolchain.

## License

Apache-2.0
