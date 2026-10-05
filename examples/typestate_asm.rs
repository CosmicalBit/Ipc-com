//! Public-API assembly probe. Build with:
//! cargo rustc --release --example typestate_asm -- --emit=asm,llvm-ir -C strip=none

use ipc_com::{Missing, Present, ReadOnly, ReadWrite, Result, SharedData, SharedMemoryOptions, SharedValue};
use std::borrow::Cow;
use std::hint::black_box;

struct Payload(String);

impl SharedData for Payload {
    fn as_bytes(&self) -> Result<Cow<'_, [u8]>> {
        Ok(Cow::Borrowed(self.0.as_bytes()))
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(Self(String::from_utf8_lossy(bytes).into_owned()))
    }
}

// These ordinary public-API calls have explicit boundaries so their ABI costs
// can be examined separately from the inlined calls in main.
#[unsafe(no_mangle)]
#[inline(never)]
fn probe_builder_access(
    options: SharedMemoryOptions<Payload, ReadOnly, Present, Present>,
) -> SharedMemoryOptions<Payload, ReadWrite, Present, Present> {
    options.to_mutable()
}

#[unsafe(no_mangle)]
#[inline(never)]
fn probe_value_access(value: SharedValue<Payload, ReadOnly>) -> SharedValue<Payload, ReadWrite> {
    value.to_mut()
}

#[unsafe(no_mangle)]
#[inline(never)]
fn probe_value_access_read(value: SharedValue<Payload, ReadOnly>) -> Result<Payload> {
    value.to_mut().read()
}

#[unsafe(no_mangle)]
#[inline(never)]
fn probe_with_data(
    options: SharedMemoryOptions<Payload, ReadOnly, Missing, Missing>,
    data: Payload,
) -> SharedMemoryOptions<Payload, ReadOnly, Present, Missing> {
    options.with_data(data)
}

#[unsafe(no_mangle)]
#[inline(never)]
fn probe_name(
    options: SharedMemoryOptions<Payload, ReadOnly, Present, Missing>,
    name: &str,
) -> SharedMemoryOptions<Payload, ReadOnly, Present, Present> {
    options.name(name)
}

fn main() {
    let name = format!("/ipc_com_typestate_asm_{}", std::process::id());
    let data = Payload(black_box(String::from("first")));

    let options = SharedMemoryOptions::new();
    let options = probe_with_data(options, data);
    let options = probe_name(options, &name);
    let options = probe_builder_access(options);
    let mut value = options.create().unwrap();
    value.write(Payload(black_box(String::from("second")))).unwrap();
    black_box(value.read().unwrap().0);

    let reader = SharedValue::<Payload, ReadOnly>::new_reader(&name).unwrap();
    let mut writer = probe_value_access(reader);
    writer.write(Payload(black_box(String::from("third")))).unwrap();
    black_box(writer.read().unwrap().0);

    // Also use both access conversions without a probe function boundary.
    let name2 = format!("{name}_inline");
    let mut value2 = SharedMemoryOptions::new()
        .with_data(Payload(black_box(String::from("inline"))))
        .name(&name2)
        .to_mutable()
        .create()
        .unwrap();
    value2.write(Payload(black_box(String::from("updated")))).unwrap();
    black_box(value2.read().unwrap().0);

    let reader2 = SharedValue::<Payload, ReadOnly>::new_reader(&name2).unwrap();
    let mut writer2 = reader2.to_mut();
    writer2.write(Payload(black_box(String::from("updated again")))).unwrap();
    black_box(writer2.read().unwrap().0);

    let reader3 = SharedValue::<Payload, ReadOnly>::new_reader(&name2).unwrap();
    black_box(probe_value_access_read(reader3).unwrap().0);
}
