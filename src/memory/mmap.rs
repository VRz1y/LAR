//! Virtual Memory Allocation and Protection with 16KB Segment Alignment.
//!
//! Provides RAII memory mapping, protection flag management, and 16KB-aligned
//! address range reservations for ELF loaders.

use crate::memory::page::{PAGE_SIZE_16K, align_up_16k, is_16k_aligned};
use std::fmt;
use std::ptr::NonNull;

/// Protection flags for memory regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtFlags(pub i32);

impl ProtFlags {
    pub const NONE: Self = Self(libc::PROT_NONE);
    pub const READ: Self = Self(libc::PROT_READ);
    pub const WRITE: Self = Self(libc::PROT_WRITE);
    pub const EXEC: Self = Self(libc::PROT_EXEC);
    pub const READ_WRITE: Self = Self(libc::PROT_READ | libc::PROT_WRITE);
    pub const READ_EXEC: Self = Self(libc::PROT_READ | libc::PROT_EXEC);
    pub const READ_WRITE_EXEC: Self = Self(libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC);

    #[inline]
    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub const fn union(&self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    pub const fn can_read(&self) -> bool {
        (self.0 & libc::PROT_READ) != 0
    }

    #[inline]
    pub const fn can_write(&self) -> bool {
        (self.0 & libc::PROT_WRITE) != 0
    }

    #[inline]
    pub const fn can_exec(&self) -> bool {
        (self.0 & libc::PROT_EXEC) != 0
    }
}

impl std::ops::BitOr for ProtFlags {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// Errors related to virtual memory operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    AllocationFailed {
        size: usize,
        errno: i32,
    },
    ProtectionFailed {
        addr: usize,
        size: usize,
        errno: i32,
    },
    OutOfBounds {
        offset: usize,
        len: usize,
        total: usize,
    },
    InvalidAlignment {
        addr: usize,
        expected_align: usize,
    },
    DeallocationFailed {
        addr: usize,
        size: usize,
        errno: i32,
    },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { size, errno } => {
                write!(
                    f,
                    "Failed to allocate {} bytes via mmap (errno {})",
                    size, errno
                )
            }
            Self::ProtectionFailed { addr, size, errno } => {
                write!(
                    f,
                    "Failed to mprotect 0x{:x} ({} bytes) (errno {})",
                    addr, size, errno
                )
            }
            Self::OutOfBounds { offset, len, total } => {
                write!(
                    f,
                    "Out of bounds access: offset {} + len {} exceeds total {}",
                    offset, len, total
                )
            }
            Self::InvalidAlignment {
                addr,
                expected_align,
            } => {
                write!(
                    f,
                    "Address 0x{:x} is not aligned to {} bytes",
                    addr, expected_align
                )
            }
            Self::DeallocationFailed { addr, size, errno } => {
                write!(
                    f,
                    "Failed to munmap 0x{:x} ({} bytes) (errno {})",
                    addr, size, errno
                )
            }
        }
    }
}

impl std::error::Error for MemoryError {}

/// RAII wrapper for an anonymous or mapped virtual memory region.
/// Guarantees unmapping on drop.
pub struct MemoryRegion {
    ptr: NonNull<u8>,
    len: usize,
    prot: ProtFlags,
}

// MemoryRegion can be safely sent across threads if properly synchronized
unsafe impl Send for MemoryRegion {}
unsafe impl Sync for MemoryRegion {}

impl MemoryRegion {
    /// Allocates an anonymous virtual memory region of `size` bytes, aligned to at least 16KB.
    pub fn allocate_16k(size: usize, prot: ProtFlags) -> Result<Self, MemoryError> {
        if size == 0 {
            return Err(MemoryError::AllocationFailed {
                size: 0,
                errno: libc::EINVAL,
            });
        }

        let aligned_size =
            size.checked_add(PAGE_SIZE_16K - 1)
                .ok_or(MemoryError::AllocationFailed {
                    size,
                    errno: libc::ENOMEM,
                })?
                & !(PAGE_SIZE_16K - 1);
        let alloc_size =
            aligned_size
                .checked_add(PAGE_SIZE_16K)
                .ok_or(MemoryError::AllocationFailed {
                    size: aligned_size,
                    errno: libc::ENOMEM,
                })?;

        let raw_ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                alloc_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if raw_ptr == libc::MAP_FAILED || raw_ptr.is_null() {
            let errno = unsafe { *libc::__errno_location() };
            return Err(MemoryError::AllocationFailed {
                size: alloc_size,
                errno,
            });
        }

        let addr = raw_ptr as usize;
        let aligned_addr = align_up_16k(addr);
        let prefix_len = aligned_addr - addr;
        let suffix_len = alloc_size - prefix_len - aligned_size;

        // Unmap prefix padding if any
        if prefix_len > 0 {
            unsafe {
                libc::munmap(raw_ptr, prefix_len);
            }
        }

        // Unmap suffix padding if any
        if suffix_len > 0 {
            unsafe {
                libc::munmap(
                    (aligned_addr + aligned_size) as *mut libc::c_void,
                    suffix_len,
                );
            }
        }

        let final_ptr = aligned_addr as *mut u8;

        // Apply desired protection
        if prot != ProtFlags::READ_WRITE {
            let ret =
                unsafe { libc::mprotect(final_ptr as *mut libc::c_void, aligned_size, prot.0) };
            if ret != 0 {
                let errno = unsafe { *libc::__errno_location() };
                unsafe {
                    libc::munmap(final_ptr as *mut libc::c_void, aligned_size);
                }
                return Err(MemoryError::ProtectionFailed {
                    addr: aligned_addr,
                    size: aligned_size,
                    errno,
                });
            }
        }

        let non_null = NonNull::new(final_ptr).expect("Pointer must not be null");

        Ok(Self {
            ptr: non_null,
            len: aligned_size,
            prot,
        })
    }

    /// Reserves a contiguous address range without physical backing (PROT_NONE).
    pub fn reserve_address_space(size: usize) -> Result<Self, MemoryError> {
        Self::allocate_16k(size, ProtFlags::NONE)
    }

    /// Changes the memory protection flags for the entire region.
    pub fn protect(&mut self, new_prot: ProtFlags) -> Result<(), MemoryError> {
        let ret =
            unsafe { libc::mprotect(self.ptr.as_ptr() as *mut libc::c_void, self.len, new_prot.0) };
        if ret != 0 {
            let errno = unsafe { *libc::__errno_location() };
            return Err(MemoryError::ProtectionFailed {
                addr: self.as_ptr() as usize,
                size: self.len,
                errno,
            });
        }
        self.prot = new_prot;
        Ok(())
    }

    /// Returns a raw immutable pointer to the base of the memory region.
    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Returns a raw mutable pointer to the base of the memory region.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Returns the length in bytes (aligned to 16KB).
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Checks if the region is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the current protection flags.
    #[inline]
    pub fn prot(&self) -> ProtFlags {
        self.prot
    }

    /// Returns the region as an immutable slice if readable.
    pub fn as_slice(&self) -> Result<&[u8], MemoryError> {
        if !self.prot.can_read() {
            return Err(MemoryError::ProtectionFailed {
                addr: self.as_ptr() as usize,
                size: self.len,
                errno: libc::EACCES,
            });
        }
        unsafe { Ok(std::slice::from_raw_parts(self.ptr.as_ptr(), self.len)) }
    }

    /// Returns the region as a mutable slice if writable.
    pub fn as_mut_slice(&mut self) -> Result<&mut [u8], MemoryError> {
        if !self.prot.can_write() {
            return Err(MemoryError::ProtectionFailed {
                addr: self.as_ptr() as usize,
                size: self.len,
                errno: libc::EACCES,
            });
        }
        unsafe { Ok(std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len)) }
    }

    /// Writes a slice of data at a specific offset.
    pub fn write_at(&mut self, offset: usize, data: &[u8]) -> Result<(), MemoryError> {
        if offset
            .checked_add(data.len())
            .is_none_or(|end| end > self.len)
        {
            return Err(MemoryError::OutOfBounds {
                offset,
                len: data.len(),
                total: self.len,
            });
        }
        let slice = self.as_mut_slice()?;
        slice[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    /// Reads data from a specific offset into a vector.
    pub fn read_at(&self, offset: usize, len: usize) -> Result<Vec<u8>, MemoryError> {
        if offset.checked_add(len).is_none_or(|end| end > self.len) {
            return Err(MemoryError::OutOfBounds {
                offset,
                len,
                total: self.len,
            });
        }
        let slice = self.as_slice()?;
        Ok(slice[offset..offset + len].to_vec())
    }
}

impl Drop for MemoryRegion {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.len);
        }
    }
}

impl fmt::Debug for MemoryRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryRegion")
            .field("addr", &format_args!("0x{:x}", self.as_ptr() as usize))
            .field("len", &self.len)
            .field("prot", &self.prot)
            .field("16k_aligned", &is_16k_aligned(self.as_ptr() as usize))
            .finish()
    }
}

/// High-level virtual memory manager for ELF load segment orchestration.
#[derive(Debug, Default)]
pub struct VirtualMemoryManager {
    allocated_regions: Vec<MemoryRegion>,
}

impl VirtualMemoryManager {
    pub fn new() -> Self {
        Self {
            allocated_regions: Vec::new(),
        }
    }

    /// Allocates and tracks a 16KB-aligned memory region.
    pub fn allocate(&mut self, size: usize, prot: ProtFlags) -> Result<*mut u8, MemoryError> {
        let region = MemoryRegion::allocate_16k(size, prot)?;
        let ptr = region.as_ptr() as *mut u8;
        self.allocated_regions.push(region);
        Ok(ptr)
    }

    /// Total mapped virtual memory in bytes.
    pub fn total_mapped_bytes(&self) -> usize {
        self.allocated_regions.iter().map(|r| r.len()).sum()
    }

    /// Number of active 16KB regions.
    pub fn region_count(&self) -> usize {
        self.allocated_regions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mmap_16k_allocation() {
        let mut region = MemoryRegion::allocate_16k(0x1000, ProtFlags::READ_WRITE).unwrap();
        assert!(is_16k_aligned(region.as_ptr() as usize));
        assert!(region.len() >= 0x4000);

        let data = [1u8, 2, 3, 4, 5];
        region.write_at(0, &data).unwrap();
        let read_back = region.read_at(0, 5).unwrap();
        assert_eq!(read_back, data);
    }

    #[test]
    fn test_mprotect_transition() {
        let mut region = MemoryRegion::allocate_16k(0x4000, ProtFlags::READ_WRITE).unwrap();
        region.write_at(10, &[0xAA, 0xBB]).unwrap();

        // Make it Read-Only
        region.protect(ProtFlags::READ).unwrap();
        let read_back = region.read_at(10, 2).unwrap();
        assert_eq!(read_back, &[0xAA, 0xBB]);

        // Attempt to write should fail our check
        assert!(region.as_mut_slice().is_err());
    }

    #[test]
    fn test_virtual_memory_manager() {
        let mut vmm = VirtualMemoryManager::new();
        let ptr1 = vmm.allocate(0x2000, ProtFlags::READ_WRITE).unwrap();
        let ptr2 = vmm.allocate(0x5000, ProtFlags::READ_EXEC).unwrap();

        assert!(is_16k_aligned(ptr1 as usize));
        assert!(is_16k_aligned(ptr2 as usize));
        assert_eq!(vmm.region_count(), 2);
        assert!(vmm.total_mapped_bytes() >= 0x4000 + 0x8000);
    }
}
