use crate::shared_mem::Mapping;
use crate::shared_mem::Result;
use std::sync::atomic::AtomicBool;

const CHUNK_SIZE: u32 = 32;

pub trait FixedSize: Sized {
    const SIZE: u32 = size_of::<Self>() as u32;
}

pub struct Pages {
    pub page_start: u32,
    pub num_of_pages: u32,
}

pub struct BitMap {
    pub offset: u32,
    pub len: u32,
}

pub fn align<T>(offset: usize) -> usize {
    offset.next_multiple_of(std::mem::align_of::<T>())
}

impl BitMap {
    pub fn write(&self, mapping: &mut Mapping) -> Result<()> {
        mapping.reset_ptr();

        for i in self.offset..=self.len {
            unsafe {
                mapping.ptr_seek(i as usize)?;

                mapping.ptr_write(AtomicBool::new(false));
            }
        }

        Ok(())
    }
    pub fn first_alloc_header(&self) -> usize {
        let end = self.len + self.offset;
        align::<AllocMetadata>(end as usize)
    }
}
struct AllocMetadata {
    is_locked: AtomicBool,
    len: u32,
    starting_offset_from_here: u32,
}

impl AllocMetadata {
    fn init_first(mem: &mut Mapping, header: &SharedHeader, metadata: AllocMetadata) -> Result<()> {
        let first_page = header.page.page_start;

        unsafe { mem.write::<AllocMetadata>(first_page as usize, metadata)? };

        Ok(())
    }
}

pub struct SharedHeader {
    bitmap: BitMap,
    page: Pages,
}

impl SharedHeader {
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

        SharedHeader { page: pages, bitmap: bit_map }
    }
}

impl Pages {
    fn new(bit_map_end: u32, num_of_pages: u32) -> Self {
        let first_page = align::<AllocMetadata>(bit_map_end as usize);
        Pages {
            page_start: first_page as u32,
            num_of_pages,
        }
    }
}

//all this headders are fixed size
impl FixedSize for Pages {}
impl FixedSize for BitMap {}
impl FixedSize for AllocMetadata {}
impl FixedSize for SharedHeader {}

#[cfg(test)]
mod tests {
    use super::{AllocMetadata, BitMap};

    #[test]
    fn alloc_metadata_allign() {
        let bitmap = BitMap { offset: 1, len: 1 };
        let first_alloc_header = bitmap.first_alloc_header();
        let alignment = std::mem::align_of::<AllocMetadata>();

        assert_eq!(first_alloc_header % alignment, 0);
        assert!(first_alloc_header >= (bitmap.offset + bitmap.len) as usize);
        assert!(first_alloc_header - ((bitmap.offset + bitmap.len) as usize) < alignment);
    }
}
