//! Memory subsystem for LAR: 16KB page masking, virtual memory mapping, and protection.

pub mod mmap;
pub mod page;

pub use mmap::{MemoryError, MemoryRegion, ProtFlags, VirtualMemoryManager};
pub use page::{
    align_down, align_down_16k, align_up, align_up_16k, host_page_size, is_16k_aligned,
    is_aligned, page_count_16k, page_offset_16k, PAGE_MASK_16K, PAGE_OFFSET_MASK_16K,
    PAGE_SIZE_16K, PAGE_SIZE_4K,
};
