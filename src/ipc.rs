use crate::shared_mem::Mapping;
use crate::shared_mem::Result;
use std::alloc::Layout;
use std::hint::select_unpredictable;
use std::sync::atomic::AtomicBool;

const CHUNK_SIZE: u32 = 32;

pub trait FixedSize: Sized {
    const SIZE: u32 = size_of::<Self>() as u32;
}

pub struct Pages {
    pub page_start: u32,
    pub num_of_pages: u32,
    pub first_page: u32,
}

pub struct BitMap {
    pub offset: u32,
    pub len: u32,
}
impl BitMap {
    pub fn write(&self, mapping: &mut Mapping) -> Result<()> {
        mapping.reset_ptr();

        for i in self.offset..=self.len {
            mapping.ptr_seek(i as usize)?;
            unsafe {
                mapping.ptr_write(AtomicBool::new(false));
            }
        }

        Ok(())
    }
}
struct AllocMetadata {
    is_locked: AtomicBool,
    len: u32,
    starting_offset_from_here: u32,
}

pub struct SharedHeader {
    pages: Pages,
    bitmap: BitMap,
}

impl SharedHeader {
    pub fn pages(&self) -> &Pages {
        &self.pages
    }
    pub fn bitmap(&self) -> &BitMap {
        &self.bitmap
    }
}

impl SharedHeader {
    pub fn new(name: &str, requested_size: u32) -> Result<Self> {
        let size = requested_size + SharedHeader::SIZE + requested_size / CHUNK_SIZE * AllocMetadata::SIZE;

        Ok(SharedHeader::init(requested_size))
    }

    fn init(requested_size: u32) -> SharedHeader {
        let num_of_pages = requested_size.div_ceil(CHUNK_SIZE) as u32;
        let bit_map_bytes = requested_size.div_ceil(8);
        let header_size = SharedHeader::SIZE;
        let bitmap_offset = header_size;

        let page_start = (bitmap_offset + bit_map_bytes);

        let is_locked = AtomicBool::new(false);

        //create pages
        let pages = Pages::new(page_start, num_of_pages);

        let bit_map = BitMap {
            offset: bitmap_offset as u32,
            len: bit_map_bytes as u32,
        };

        SharedHeader { pages, bitmap: bit_map }
    }
}

impl Pages {
    fn new(bit_map_end: u32, num_of_pages: u32) -> Self {
        Pages {
            page_start: bit_map_end,
            num_of_pages,
            first_page: 0,
        }
    }
}

//all this headders are fixed size
impl FixedSize for Pages {}
impl FixedSize for BitMap {}
impl FixedSize for AllocMetadata {}
impl FixedSize for SharedHeader {}
