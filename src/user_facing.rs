use crate::ipc::SharedHeader;
use crate::shared_mem::Mapping;
use crate::shared_mem::Result;
use std::io;

pub struct SharedMemory {
    mem: Mapping,
}

impl SharedMemory {
    pub fn new(name: &str, size: usize) -> Result<Self> {
        let size = size as u32;
        //create header and init mem
        let header = SharedHeader::new(name, size)?;
        let mut mem = Mapping::init_shared_mem(name, size)?;

        //actually init all the atomic bitmap
        header.bitmap().write(&mut mem)?;

        //write the header and save it
        mem.write_header(header);
        let shared = SharedMemory { mem };

        Ok(shared)
    }
    pub fn alloc(&mut self, size: usize) -> Result<()> {
        let header = self.mem.attomic_bool_slice();

        //TODO
        
        Ok(())
    }
}
