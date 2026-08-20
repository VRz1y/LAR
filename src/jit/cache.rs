//! Disk MMAP Execution Cache for Zero-Stutter Cold Starts.
//!
//! Hashes compiled blocks (`FastHash64(ARM64_Block + Register_State)`), persists them
//! to binary cache format (`LARCACH1`), and maps ready-to-run machine code instantly
//! via `mmap(PROT_READ | PROT_EXEC)`.

use crate::arch::{Arm64CpuContext, HostArch};
use crate::memory::mmap::{MemoryError, MemoryRegion, ProtFlags};
use crate::memory::page::{PAGE_SIZE_16K, align_up_16k};
use std::collections::HashMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

pub const CACHE_MAGIC: [u8; 8] = *b"LARCACH1";
pub const CACHE_ABI_CONTEXT: u8 = 1;

/// Errors related to JIT execution caching.
#[derive(Debug)]
pub enum CacheError {
    Io(std::io::Error),
    Memory(MemoryError),
    InvalidMagic,
    ArchMismatch { expected: HostArch, found: u8 },
    CorruptedCache(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Cache I/O error: {}", e),
            Self::Memory(e) => write!(f, "Cache Memory error: {}", e),
            Self::InvalidMagic => write!(f, "Invalid LAR cache magic identifier"),
            Self::ArchMismatch { expected, found } => {
                write!(
                    f,
                    "Cache architecture mismatch: expected {:?}, found {}",
                    expected, found
                )
            }
            Self::CorruptedCache(msg) => write!(f, "Corrupted cache: {}", msg),
        }
    }
}

impl std::error::Error for CacheError {}

impl From<std::io::Error> for CacheError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<MemoryError> for CacheError {
    fn from(e: MemoryError) -> Self {
        Self::Memory(e)
    }
}

/// Computes a fast 64-bit non-cryptographic hash for a block of ARM64 opcodes.
pub fn hash_arm64_block(guest_pc: u64, opcodes: &[u32]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64; // FNV-1a 64 offset basis
    const PRIME: u64 = 0x100000001b3;

    hash ^= guest_pc;
    hash = hash.wrapping_mul(PRIME);

    for &op in opcodes {
        hash ^= op as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

pub fn hash_arm64_block_with_context(ctx: &Arm64CpuContext, opcodes: &[u32]) -> u64 {
    let mut hash = hash_arm64_block(ctx.pc, opcodes);
    const PRIME: u64 = 0x100000001b3;

    for &reg in &ctx.regs {
        hash ^= reg;
        hash = hash.wrapping_mul(PRIME);
    }
    hash ^= ctx.sp;
    hash = hash.wrapping_mul(PRIME);
    hash ^= ctx.pstate & (0xF << 28);
    hash.wrapping_mul(PRIME)
}

/// Metadata entry for a cached JIT block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct CacheBlockEntry {
    pub block_hash: u64,
    pub guest_pc: u64,
    pub code_offset: u32,
    pub code_size: u32,
}

/// Single compiled block to be saved into cache.
pub struct CompiledBlock {
    pub block_hash: u64,
    pub guest_pc: u64,
    pub machine_code: Vec<u8>,
}

/// In-memory Mmapped Execution Cache.
pub struct MmapExecutionCache {
    pub host_arch: HostArch,
    pub region: MemoryRegion,
    pub entries: HashMap<u64, (usize, usize)>, // hash -> (offset, size)
}

impl MmapExecutionCache {
    /// Persists compiled blocks into a `.larcache` file on disk.
    pub fn create_and_save<P: AsRef<Path>>(
        path: P,
        arch: HostArch,
        blocks: &[CompiledBlock],
    ) -> Result<(), CacheError> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;

        let arch_tag = match arch {
            HostArch::X86_64 => 1u8,
            HostArch::Riscv64 => 2u8,
            HostArch::Arm64 => 3u8,
            HostArch::Unknown => 0u8,
        };

        // Header: 8 bytes magic + 1 byte arch + 3 bytes padding + 4 bytes num_entries (16 bytes total)
        file.write_all(&CACHE_MAGIC)?;
        file.write_all(&[arch_tag, CACHE_ABI_CONTEXT, 0, 0])?;
        file.write_all(&(blocks.len() as u32).to_le_bytes())?;

        // Calculate offsets
        let header_and_table_size = 16 + blocks.len() * 24;
        let code_base_offset = align_up_16k(header_and_table_size);

        let mut current_code_offset = code_base_offset;
        let mut entries = Vec::with_capacity(blocks.len());

        for b in blocks {
            entries.push(CacheBlockEntry {
                block_hash: b.block_hash,
                guest_pc: b.guest_pc,
                code_offset: current_code_offset as u32,
                code_size: b.machine_code.len() as u32,
            });
            current_code_offset += b.machine_code.len();
        }

        // Write entries metadata
        for e in &entries {
            file.write_all(&e.block_hash.to_le_bytes())?;
            file.write_all(&e.guest_pc.to_le_bytes())?;
            file.write_all(&e.code_offset.to_le_bytes())?;
            file.write_all(&e.code_size.to_le_bytes())?;
        }

        // Pad up to code_base_offset
        let pad_len = code_base_offset - header_and_table_size;
        if pad_len > 0 {
            file.write_all(&vec![0u8; pad_len])?;
        }

        // Write compiled machine code
        for b in blocks {
            file.write_all(&b.machine_code)?;
        }

        file.flush()?;
        Ok(())
    }

    /// Loads and memory-maps a `.larcache` file into executable memory.
    pub fn load_from_file<P: AsRef<Path>>(
        path: P,
        expected_arch: HostArch,
    ) -> Result<Self, CacheError> {
        let mut file = File::open(path)?;
        let mut header = [0u8; 16];
        file.read_exact(&mut header)?;

        if header[0..8] != CACHE_MAGIC {
            return Err(CacheError::InvalidMagic);
        }

        let arch_tag = header[8];
        if header[9] != CACHE_ABI_CONTEXT {
            return Err(CacheError::CorruptedCache(
                "unsupported execution ABI".into(),
            ));
        }
        let expected_tag = match expected_arch {
            HostArch::X86_64 => 1u8,
            HostArch::Riscv64 => 2u8,
            HostArch::Arm64 => 3u8,
            HostArch::Unknown => 0u8,
        };
        if arch_tag != expected_tag {
            return Err(CacheError::ArchMismatch {
                expected: expected_arch,
                found: arch_tag,
            });
        }
        let num_entries = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;

        let entries_size = num_entries
            .checked_mul(24)
            .ok_or_else(|| CacheError::CorruptedCache("entry table size overflow".into()))?;
        let mut entries_bytes = vec![0u8; entries_size];
        file.read_exact(&mut entries_bytes)?;

        let mut entries_map = HashMap::with_capacity(num_entries);
        let mut max_end_offset = 0usize;

        for i in 0..num_entries {
            let start = i * 24;
            let hash = u64::from_le_bytes(entries_bytes[start..start + 8].try_into().unwrap());
            let _guest_pc =
                u64::from_le_bytes(entries_bytes[start + 8..start + 16].try_into().unwrap());
            let offset =
                u32::from_le_bytes(entries_bytes[start + 16..start + 20].try_into().unwrap())
                    as usize;
            let size = u32::from_le_bytes(entries_bytes[start + 20..start + 24].try_into().unwrap())
                as usize;

            let end = offset
                .checked_add(size)
                .ok_or_else(|| CacheError::CorruptedCache("block range overflow".into()))?;
            if offset < align_up_16k(16 + entries_bytes.len()) || end < offset {
                return Err(CacheError::CorruptedCache(
                    "block range overlaps cache header".into(),
                ));
            }
            if end > max_end_offset {
                max_end_offset = end;
            }

            entries_map.insert(hash, (offset, size));
        }

        // Read rest of file
        let total_file_size = align_up_16k(
            max_end_offset
                .checked_add(PAGE_SIZE_16K)
                .ok_or_else(|| CacheError::CorruptedCache("cache size overflow".into()))?,
        );
        let mut mem_region = MemoryRegion::allocate_16k(total_file_size, ProtFlags::READ_WRITE)?;

        // Read entire file content into allocated memory
        let file_slice =
            unsafe { std::slice::from_raw_parts_mut(mem_region.as_mut_ptr(), total_file_size) };

        file_slice[0..16].copy_from_slice(&header);
        file_slice[16..16 + entries_bytes.len()].copy_from_slice(&entries_bytes);

        let remaining_read = file.read(&mut file_slice[16 + entries_bytes.len()..])?;
        let _ = remaining_read;

        // Apply executable protection
        mem_region.protect(ProtFlags::READ_EXEC)?;

        Ok(Self {
            host_arch: expected_arch,
            region: mem_region,
            entries: entries_map,
        })
    }

    /// Looks up a compiled block by hash and returns a function pointer.
    pub fn lookup_block(&self, hash: u64) -> Option<*const u8> {
        let &(offset, size) = self.entries.get(&hash)?;
        if size > 0
            && offset
                .checked_add(size)
                .is_some_and(|end| end <= self.region.len())
        {
            Some(unsafe { self.region.as_ptr().add(offset) })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::is_16k_aligned;

    #[test]
    fn test_block_hashing() {
        let h1 = hash_arm64_block(0x1000, &[0x9100a820, 0xd65f03c0]);
        let h2 = hash_arm64_block(0x1000, &[0x9100a820, 0xd65f03c0]);
        let h3 = hash_arm64_block(0x2000, &[0x9100a820, 0xd65f03c0]);

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_context_hash_includes_register_state() {
        let first = Arm64CpuContext::new();
        let mut second = first;
        second.regs[0] = 1;

        assert_ne!(
            hash_arm64_block_with_context(&first, &[0xd503201f]),
            hash_arm64_block_with_context(&second, &[0xd503201f])
        );
    }

    #[test]
    fn test_mmap_execution_cache_save_and_load() {
        let temp_dir = std::env::temp_dir();
        let cache_path = temp_dir.join("test_app.larcache");

        let block1 = CompiledBlock {
            block_hash: 0x1234_5678,
            guest_pc: 0x0040_0000,
            machine_code: vec![0x48, 0x31, 0xc0, 0xc3], // xor rax, rax; ret
        };

        MmapExecutionCache::create_and_save(&cache_path, HostArch::X86_64, &[block1])
            .expect("Failed to create cache");

        let loaded_cache = MmapExecutionCache::load_from_file(&cache_path, HostArch::X86_64)
            .expect("Failed to load cache");

        let ptr = loaded_cache.lookup_block(0x1234_5678);
        assert!(ptr.is_some());
        assert!(is_16k_aligned(loaded_cache.region.as_ptr() as usize));

        let _ = std::fs::remove_file(cache_path);
    }

    #[test]
    fn test_mmap_execution_cache_rejects_architecture_mismatch() {
        let cache_path = std::env::temp_dir().join("test_arch_mismatch.larcache");
        let block = CompiledBlock {
            block_hash: 1,
            guest_pc: 0,
            machine_code: vec![0xc3],
        };

        MmapExecutionCache::create_and_save(&cache_path, HostArch::X86_64, &[block]).unwrap();
        let result = MmapExecutionCache::load_from_file(&cache_path, HostArch::Riscv64);
        assert!(matches!(result, Err(CacheError::ArchMismatch { .. })));
        let _ = std::fs::remove_file(cache_path);
    }
}
