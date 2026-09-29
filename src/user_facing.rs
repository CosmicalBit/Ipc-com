use crate::ipc::SharedHeader;
use crate::shared_mem::Mapping;
use crate::shared_mem::Result;
use std::io;

struct SharedMemory{
    mem: Mapping,
}

impl SharedMemory{
    pub fn new(name: &str, size: usize) -> Result<()> {
        let size = size as u32;
        //create header and init mem
        let header = SharedHeader::new(name, size)?;
        let mut mem = Mapping::init_shared_mem(name, size)?;

        //actually init all the atomic bitmap
        header.bitmap().write(&mut mem)?;

        //write the header and save it
        let header = mem.write_header(header);
        let shared = SharedMemory {  mem };

        //TODO init the alloc header

        Ok(())
    }
}
