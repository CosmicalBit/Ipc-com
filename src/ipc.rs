use crate::shared_mem::Mapping;
use crate::shared_mem::Result;
use std::alloc::Layout;
use std::sync::atomic::AtomicBool;

const CHUNK_SIZE: usize = 32;

pub trait FixedSize: Sized {
    const SIZE: usize = size_of::<Self>();
}

struct Pages {
    page_start: u32,
    num_of_pages: u32,
}

struct BitMap {
    is_locked: AtomicBool,
    offset: u32,
    end: u32,
}

struct AllocMetadata {
    len: u32,
}

pub struct SharedHeader {
    mem: Pages,
    bitmap: BitMap,
}


impl SharedHeader {
    pub fn new(name: &str, requested_size: usize) -> Result<()> {
        let size = requested_size + SharedHeader::SIZE + requested_size / CHUNK_SIZE * AllocMetadata::SIZE;

        let header = SharedHeader::init(requested_size);

        let mem = Mapping::init_shared_mem(name, size)?;

        let header = mem.write_header(header);
        
        Ok(())
    }

    fn init(requested_size: usize) -> SharedHeader {
        let num_of_pages = requested_size.div_ceil(CHUNK_SIZE) as u32;
        let bit_map_bytes = requested_size.div_ceil(8);
        let header_size = SharedHeader::SIZE;
        let bitmap_offset = header_size;

        let page_start = (bitmap_offset + bit_map_bytes).next_multiple_of(CHUNK_SIZE) as u32;

        let is_locked = AtomicBool::new(false);

        let pages = Pages { num_of_pages, page_start };
        let bit_map = BitMap {
            is_locked,
            offset: bitmap_offset as u32,
            end: (bitmap_offset + bit_map_bytes) as u32,
        };

        SharedHeader { mem: pages, bitmap: bit_map }
    }
}

//all this headders are fixed size
impl FixedSize for Pages {}
impl FixedSize for BitMap {}
impl FixedSize for AllocMetadata {}
impl FixedSize for SharedHeader {}
