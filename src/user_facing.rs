use crate::ipc::SharedHeader;
use crate::shared_mem::Mapping;
use crate::shared_mem::Result;
use std::io;

struct SharedMemory<'a> {
    header: &'a mut SharedHeader,
    mem: Mapping,
}

//TODO FIX ALL LIFETIME ISSUES
impl<'a> SharedMemory<'a> {
    fn new(name: &str, size: usize) -> Result<()> {
        let size = size as u32;
        let header = SharedHeader::new(name, size)?;

        let mem = Mapping::init_shared_mem(name, size)?;

        let header = mem.write_header(header);

        let mut shared = SharedMemory { header, mem };

        header.bitmap().write(&mut shared.mem)?;

        //TODO
        Ok(())
    }
}
