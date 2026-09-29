use crate::shared_mem::Result;
use crate::user_facing::SharedMemory;
mod ipc;
mod shared_mem;
mod user_facing;

fn main() -> Result<()> {
    let name = "Banana";
    SharedMemory::new(name, 3000)?;
    Ok(())
}
