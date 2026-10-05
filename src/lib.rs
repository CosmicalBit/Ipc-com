mod allocation;
mod shared_mem;
mod shared_value;
mod user_facing;

pub use allocation::{ReadOnly, ReadWrite};
pub use shared_mem::{Error, Result};
pub use shared_value::{SharedData, SharedValue};
pub use user_facing::{Missing, Present, SharedMemoryOptions};
