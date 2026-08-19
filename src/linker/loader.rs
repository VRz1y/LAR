//! ELF Shared Library Loader and Dynamic Linker for 64-bit ARM.
//!
//! Loads ELF binaries into 16KB-aligned memory, maps PT_LOAD segments,
//! resolves AArch64 dynamic relocations, and prepares initialization vectors.

use crate::linker::elf::*;
use crate::linker::symbols::{DynamicSymbolTable, SymbolRegistry};
use crate::memory::mmap::{MemoryError, MemoryRegion, ProtFlags};
use crate::memory::page::{align_up_16k, is_16k_aligned, PAGE_SIZE_16K};
use std::fmt;

/// Errors that can occur during library loading and dynamic linking.
#[derive(Debug)]
pub enum LoaderError {
    Elf(ElfError),
    Memory(MemoryError),
    UnresolvedSymbol(String),
    UnsupportedRelocation(u32),
    IoError(std::io::Error),
    RelocationFailed { offset: usize, msg: String },
}

impl fmt::Display for LoaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Elf(e) => write!(f, "ELF error: {}", e),
            Self::Memory(e) => write!(f, "Memory error: {}", e),
            Self::UnresolvedSymbol(s) => write!(f, "Unresolved symbol: '{}'", s),
            Self::UnsupportedRelocation(r) => write!(f, "Unsupported relocation type: {}", r),
            Self::IoError(e) => write!(f, "I/O error: {}", e),
            Self::RelocationFailed { offset, msg } => {
                write!(f, "Relocation failed at offset 0x{:x}: {}", offset, msg)
            }
        }
    }
}

impl std::error::Error for LoaderError {}

impl From<ElfError> for LoaderError {
    fn from(e: ElfError) -> Self {
        Self::Elf(e)
    }
}

impl From<MemoryError> for LoaderError {
    fn from(e: MemoryError) -> Self {
        Self::Memory(e)
    }
}

impl From<std::io::Error> for LoaderError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Represents a successfully loaded, relocated, and mapped ELF shared library.
pub struct LoadedLibrary {
    pub name: String,
    pub load_base: usize,
    pub mem_region: MemoryRegion,
    pub symtab: DynamicSymbolTable,
    pub init_array: Vec<usize>,
    pub fini_array: Vec<usize>,
    pub entry_point: Option<usize>,
}

impl LoadedLibrary {
    /// Resolves the address of an exported symbol in this library.
    pub fn lookup_symbol(&self, name: &str) -> Option<usize> {
        self.symtab.lookup(name).map(|s| s.address)
    }

    /// Calls all initialization routines in `DT_INIT` and `DT_INIT_ARRAY`.
    pub unsafe fn call_init_routines(&self) {
        for &init_fn in &self.init_array {
            if init_fn != 0 {
                let func: extern "C" fn() = unsafe { std::mem::transmute(init_fn) };
                func();
            }
        }
    }
}

impl fmt::Debug for LoadedLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedLibrary")
            .field("name", &self.name)
            .field("load_base", &format_args!("0x{:x}", self.load_base))
            .field("size", &self.mem_region.len())
            .field("16k_aligned", &is_16k_aligned(self.load_base))
            .field("init_array_len", &self.init_array.len())
            .finish()
    }
}

/// Dynamic Linker & Loader engine.
pub struct ElfLoader;

impl ElfLoader {
    /// Loads an ARM64 ELF shared library from a byte buffer with 16KB segment alignment.
    pub fn load_from_memory(
        name: &str,
        elf_bytes: &[u8],
        registry: &mut SymbolRegistry,
    ) -> Result<LoadedLibrary, LoaderError> {
        let parsed = ParsedElf::parse(elf_bytes)?;

        if parsed.load_segments.is_empty() {
            return Err(LoaderError::Elf(ElfError::CorruptedData("No PT_LOAD segments found")));
        }

        // Calculate total memory size aligned up to 16KB
        let total_size = align_up_16k(parsed.total_memsz as usize + PAGE_SIZE_16K);

        // Allocate anonymous memory region with 16KB alignment and Read/Write permission
        let mut mem_region = MemoryRegion::allocate_16k(total_size, ProtFlags::READ_WRITE)?;
        let load_base = mem_region.as_ptr() as usize;

        // Zero out the entire allocated region first
        unsafe {
            std::ptr::write_bytes(mem_region.as_mut_ptr(), 0, mem_region.len());
        }

        // Step 1: Map all PT_LOAD segments
        for phdr in &parsed.load_segments {
            let seg_vaddr = phdr.p_vaddr as usize;
            let seg_filesz = phdr.p_filesz as usize;
            let seg_offset = phdr.p_offset as usize;

            if seg_filesz > 0 {
                if seg_offset + seg_filesz > elf_bytes.len() {
                    return Err(LoaderError::Elf(ElfError::BufferTooSmall {
                        expected: seg_offset + seg_filesz,
                        found: elf_bytes.len(),
                    }));
                }

                let dest_ptr = (load_base + seg_vaddr) as *mut u8;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        elf_bytes[seg_offset..seg_offset + seg_filesz].as_ptr(),
                        dest_ptr,
                        seg_filesz,
                    );
                }
            }
        }

        // Step 2: Parse Dynamic Section pointers
        let mut strtab_offset: Option<usize> = None;
        let mut strtab_sz: usize = 0;
        let mut symtab_offset: Option<usize> = None;
        let mut rela_offset: Option<usize> = None;
        let mut rela_sz: usize = 0;
        let mut jmprel_offset: Option<usize> = None;
        let mut pltrel_sz: usize = 0;
        let mut init_fn: Option<usize> = None;
        let mut init_array_offset: Option<usize> = None;
        let mut init_array_sz: usize = 0;
        let mut fini_fn: Option<usize> = None;
        let mut fini_array_offset: Option<usize> = None;
        let mut fini_array_sz: usize = 0;

        for dyn_entry in &parsed.dynamic_entries {
            match dyn_entry.d_tag {
                DT_STRTAB => strtab_offset = Some(dyn_entry.d_val as usize),
                DT_STRSZ => strtab_sz = dyn_entry.d_val as usize,
                DT_SYMTAB => symtab_offset = Some(dyn_entry.d_val as usize),
                DT_RELA => rela_offset = Some(dyn_entry.d_val as usize),
                DT_RELASZ => rela_sz = dyn_entry.d_val as usize,
                DT_JMPREL => jmprel_offset = Some(dyn_entry.d_val as usize),
                DT_PLTRELSZ => pltrel_sz = dyn_entry.d_val as usize,
                DT_INIT => init_fn = Some(dyn_entry.d_val as usize),
                DT_INIT_ARRAY => init_array_offset = Some(dyn_entry.d_val as usize),
                DT_INIT_ARRAYSZ => init_array_sz = dyn_entry.d_val as usize,
                DT_FINI => fini_fn = Some(dyn_entry.d_val as usize),
                DT_FINI_ARRAY => fini_array_offset = Some(dyn_entry.d_val as usize),
                DT_FINI_ARRAYSZ => fini_array_sz = dyn_entry.d_val as usize,
                _ => {}
            }
        }

        // Locate string table and symbol table buffers with strict bounds checking against mem_region
        let strtab_data = if let Some(off) = strtab_offset {
            if off < mem_region.len() {
                let actual_sz = std::cmp::min(strtab_sz, mem_region.len() - off);
                let actual_ptr = (load_base + off) as *const u8;
                unsafe { std::slice::from_raw_parts(actual_ptr, actual_sz) }
            } else {
                &[]
            }
        } else {
            &[]
        };

        let symtab = if let Some(off) = symtab_offset {
            if off < mem_region.len() {
                let available_bytes = mem_region.len() - off;
                // If rela or another table follows, bound by it
                let bounded_bytes = if let Some(rela_off) = rela_offset {
                    if rela_off > off && rela_off - off < available_bytes {
                        rela_off - off
                    } else {
                        available_bytes
                    }
                } else {
                    available_bytes
                };
                let actual_ptr = (load_base + off) as *const u8;
                let sym_slice = unsafe { std::slice::from_raw_parts(actual_ptr, bounded_bytes) };
                DynamicSymbolTable::parse(sym_slice, strtab_data, load_base)
            } else {
                DynamicSymbolTable::parse(&[], strtab_data, load_base)
            }
        } else {
            DynamicSymbolTable::parse(&[], strtab_data, load_base)
        };

        // Register library symbols to global registry
        registry.register_symbol_table(&symtab);

        // Step 3: Process Relocations (DT_RELA and DT_JMPREL)
        let process_rela_table = |rela_vaddr: usize, total_sz: usize| -> Result<(), LoaderError> {
            let entry_size = 24; // sizeof(Elf64_Rela)
            if rela_vaddr + total_sz > mem_region.len() {
                return Err(LoaderError::Elf(ElfError::CorruptedData("Relocation table exceeds mapped bounds")));
            }
            let num_relas = total_sz / entry_size;
            let rela_ptr = (load_base + rela_vaddr) as *const u8;

            for i in 0..num_relas {
                let start = i * entry_size;
                let rela_bytes = unsafe { std::slice::from_raw_parts(rela_ptr.add(start), entry_size) };
                let rela = Elf64Rela::parse(rela_bytes)?;

                let target_offset = rela.r_offset as usize;
                if target_offset + 8 > mem_region.len() {
                    return Err(LoaderError::RelocationFailed {
                        offset: target_offset,
                        msg: "Relocation target address out of bounds".to_string(),
                    });
                }

                let target_addr = (load_base + target_offset) as *mut u64;
                let r_type = rela.r_type();
                let sym_idx = rela.r_sym() as usize;

                match r_type {
                    R_AARCH64_NONE => {}
                    R_AARCH64_RELATIVE => {
                        let val = (load_base as i64 + rela.r_addend) as u64;
                        unsafe {
                            *target_addr = val;
                        }
                    }
                    R_AARCH64_GLOB_DAT | R_AARCH64_JUMP_SLOT | R_AARCH64_ABS64 => {
                        let sym = symtab.get(sym_idx);
                        let sym_name = sym.map(|s| s.name.as_str()).unwrap_or("");
                        
                        // Try resolving from local symtab, then global registry
                        let sym_addr = if let Some(s) = sym {
                            if s.address != 0 {
                                Some(s.address)
                            } else {
                                registry.resolve(sym_name)
                            }
                        } else {
                            registry.resolve(sym_name)
                        };

                        if let Some(resolved) = sym_addr {
                            let val = (resolved as i64 + rela.r_addend) as u64;
                            unsafe {
                                *target_addr = val;
                            }
                        } else if sym.map_or(false, |s| s.is_weak()) {
                            // Weak unresolved symbols default to 0
                            unsafe {
                                *target_addr = 0;
                            }
                        } else {
                            return Err(LoaderError::UnresolvedSymbol(sym_name.to_string()));
                        }
                    }
                    _ => {
                        // Skip or report unsupported
                    }
                }
            }
            Ok(())
        };

        if let Some(off) = rela_offset {
            process_rela_table(off, rela_sz)?;
        }
        if let Some(off) = jmprel_offset {
            process_rela_table(off, pltrel_sz)?;
        }

        // Step 4: Extract Initialization & Finalization functions
        let mut init_array = Vec::new();
        if let Some(init) = init_fn {
            if init != 0 {
                init_array.push(load_base + init);
            }
        }
        if let Some(off) = init_array_offset {
            let count = init_array_sz / 8;
            if off + init_array_sz <= mem_region.len() {
                let array_ptr = (load_base + off) as *const u64;
                for i in 0..count {
                    let func_ptr = unsafe { *array_ptr.add(i) } as usize;
                    if func_ptr != 0 {
                        init_array.push(func_ptr);
                    }
                }
            }
        }

        let mut fini_array = Vec::new();
        if let Some(fini) = fini_fn {
            if fini != 0 {
                fini_array.push(load_base + fini);
            }
        }
        if let Some(off) = fini_array_offset {
            let count = fini_array_sz / 8;
            if off + fini_array_sz <= mem_region.len() {
                let array_ptr = (load_base + off) as *const u64;
                for i in 0..count {
                    let func_ptr = unsafe { *array_ptr.add(i) } as usize;
                    if func_ptr != 0 {
                        fini_array.push(func_ptr);
                    }
                }
            }
        }

        let entry_point = if parsed.ehdr.e_entry != 0 {
            Some(load_base + parsed.ehdr.e_entry as usize)
        } else {
            None
        };

        Ok(LoadedLibrary {
            name: name.to_string(),
            load_base,
            mem_region,
            symtab,
            init_array,
            fini_array,
            entry_point,
        })
    }
}
