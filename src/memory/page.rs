//! 16KB Memory Page Masking and Alignment Subsystem.
//!
//! Android 15+ supports and recommends 16KB page sizes (`0x4000` = 16,384 bytes).
//! LAR ensures all ELF segments, virtual memory allocations, and boundary checks
//! adhere to 16KB alignment on all hosts (including 4KB x86_64, RISC-V, and 4KB ARM64).

/// Standard 16KB page size in bytes (0x4000 = 16384).
pub const PAGE_SIZE_16K: usize = 0x4000;

/// Standard 4KB page size in bytes (0x1000 = 4096).
pub const PAGE_SIZE_4K: usize = 0x1000;

/// Bitmask for 16KB page offset (0x3FFF).
pub const PAGE_OFFSET_MASK_16K: usize = PAGE_SIZE_16K - 1;

/// Bitmask for 16KB page boundary alignment (!0x3FFF).
pub const PAGE_MASK_16K: usize = !PAGE_OFFSET_MASK_16K;

/// Returns the host operating system's hardware/kernel page size in bytes.
#[inline]
pub fn host_page_size() -> usize {
    #[cfg(unix)]
    {
        // Safe sysconf call for _SC_PAGESIZE
        let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if ps > 0 {
            return ps as usize;
        }
    }
    PAGE_SIZE_4K
}

/// Checks if an address or size is aligned to a given power-of-two alignment.
#[inline]
pub const fn is_aligned(addr: usize, align: usize) -> bool {
    if align == 0 {
        return false;
    }
    (addr & (align - 1)) == 0
}

/// Checks if an address or size is aligned to 16KB boundary.
#[inline]
pub const fn is_16k_aligned(addr: usize) -> bool {
    (addr & PAGE_OFFSET_MASK_16K) == 0
}

/// Aligns an address or size up to the next multiple of `align` (must be a power of two).
#[inline]
pub const fn align_up(addr: usize, align: usize) -> usize {
    if align == 0 {
        return addr;
    }
    (addr + (align - 1)) & !(align - 1)
}

/// Aligns an address or size down to the nearest multiple of `align` (must be a power of two).
#[inline]
pub const fn align_down(addr: usize, align: usize) -> usize {
    if align == 0 {
        return addr;
    }
    addr & !(align - 1)
}

/// Aligns an address up to the next 16KB page boundary.
#[inline]
pub const fn align_up_16k(addr: usize) -> usize {
    (addr + PAGE_OFFSET_MASK_16K) & PAGE_MASK_16K
}

/// Aligns an address down to the nearest 16KB page boundary.
#[inline]
pub const fn align_down_16k(addr: usize) -> usize {
    addr & PAGE_MASK_16K
}

/// Calculates the offset within a 16KB page.
#[inline]
pub const fn page_offset_16k(addr: usize) -> usize {
    addr & PAGE_OFFSET_MASK_16K
}

/// Calculates the number of 16KB pages needed to span a given byte length from a starting offset.
#[inline]
pub const fn page_count_16k(offset: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let end = align_up_16k(offset + len);
    let start = align_down_16k(offset);
    (end - start) / PAGE_SIZE_16K
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_16k_alignment() {
        assert!(is_16k_aligned(0));
        assert!(is_16k_aligned(0x4000));
        assert!(is_16k_aligned(0x8000));
        assert!(is_16k_aligned(0x100000));

        assert!(!is_16k_aligned(1));
        assert!(!is_16k_aligned(0x1000)); // 4KB is not 16KB aligned
        assert!(!is_16k_aligned(0x3FFF));
        assert!(!is_16k_aligned(0x4001));
    }

    #[test]
    fn test_align_up_and_down_16k() {
        assert_eq!(align_up_16k(0), 0);
        assert_eq!(align_up_16k(1), 0x4000);
        assert_eq!(align_up_16k(0x1000), 0x4000);
        assert_eq!(align_up_16k(0x4000), 0x4000);
        assert_eq!(align_up_16k(0x4001), 0x8000);

        assert_eq!(align_down_16k(0), 0);
        assert_eq!(align_down_16k(1), 0);
        assert_eq!(align_down_16k(0x3FFF), 0);
        assert_eq!(align_down_16k(0x4000), 0x4000);
        assert_eq!(align_down_16k(0x7FFF), 0x4000);
        assert_eq!(align_down_16k(0x8000), 0x8000);
    }

    #[test]
    fn test_page_offset_and_count() {
        assert_eq!(page_offset_16k(0), 0);
        assert_eq!(page_offset_16k(0x1234), 0x1234);
        assert_eq!(page_offset_16k(0x4100), 0x100);

        assert_eq!(page_count_16k(0, 0), 0);
        assert_eq!(page_count_16k(0, 1), 1);
        assert_eq!(page_count_16k(0, 0x4000), 1);
        assert_eq!(page_count_16k(0, 0x4001), 2);
        assert_eq!(page_count_16k(0x3000, 0x2000), 2);
    }

    #[test]
    fn test_host_page_size() {
        let hps = host_page_size();
        assert!(hps >= PAGE_SIZE_4K);
        assert!(is_aligned(hps, PAGE_SIZE_4K));
    }
}
