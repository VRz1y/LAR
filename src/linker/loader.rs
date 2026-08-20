//! ELF Shared Library Loader and Dynamic Linker for 64-bit ARM.
//!
//! Loads ELF binaries into 16KB-aligned memory, maps PT_LOAD segments,
//! resolves AArch64 dynamic relocations, and prepares initialization vectors.

use crate::linker::elf::*;
use crate::linker::symbols::{DynamicSymbolTable, SymbolRegistry};
use crate::memory::mmap::{MemoryError, MemoryRegion, ProtFlags};
use crate::memory::page::{PAGE_SIZE_16K, align_down_16k, align_up_16k, is_16k_aligned};
use std::cell::Cell;
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
    InvalidStartupAddress(usize),
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
            Self::InvalidStartupAddress(address) => {
                write!(f, "Invalid startup address: 0x{:x}", address)
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
    pub init: Option<usize>,
    pub jni_on_load: Option<usize>,
    pub init_routines: Vec<InitRoutine>,
    lifecycle: Cell<LibraryLifecycle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitRoutineKind {
    DtInit,
    DtInitArray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitRoutine {
    pub address: usize,
    pub kind: InitRoutineKind,
    pub order: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryLifecycle {
    Loaded,
    StartupPrepared,
    Initialized,
    Finalized,
}

impl LoadedLibrary {
    /// Resolves the address of an exported symbol in this library.
    pub fn lookup_symbol(&self, name: &str) -> Option<usize> {
        self.symtab.lookup(name).map(|s| s.address)
    }

    /// Calls all initialization routines in `DT_INIT` and `DT_INIT_ARRAY`.
    ///
    /// # Safety
    /// Every recorded routine address must point to a valid function with the C ABI.
    pub unsafe fn call_init_routines(&self) {
        if self.lifecycle.get() != LibraryLifecycle::StartupPrepared {
            return;
        }
        for routine in &self.init_routines {
            let init_fn = routine.address;
            if init_fn != 0 {
                let func: extern "C" fn() = unsafe { std::mem::transmute(init_fn) };
                func();
            }
        }
        self.lifecycle.set(LibraryLifecycle::Initialized);
    }

    pub fn lifecycle(&self) -> LibraryLifecycle {
        self.lifecycle.get()
    }

    pub fn prepare_startup(&self) -> bool {
        if self.lifecycle.get() == LibraryLifecycle::Loaded {
            self.lifecycle.set(LibraryLifecycle::StartupPrepared);
            true
        } else {
            false
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
            .field("jni_on_load", &self.jni_on_load)
            .field("lifecycle", &self.lifecycle())
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
            return Err(LoaderError::Elf(ElfError::CorruptedData(
                "No PT_LOAD segments found",
            )));
        }

        let image_size = parsed
            .vaddr_max
            .checked_sub(parsed.vaddr_min)
            .ok_or(LoaderError::Elf(ElfError::CorruptedData(
                "PT_LOAD virtual address range underflows",
            )))?;
        let image_size = usize::try_from(image_size).map_err(|_| {
            LoaderError::Elf(ElfError::CorruptedData("PT_LOAD image size is too large"))
        })?;
        let total_size = image_size
            .checked_add(PAGE_SIZE_16K - 1)
            .map(|value| value & !(PAGE_SIZE_16K - 1))
            .ok_or(LoaderError::Elf(ElfError::CorruptedData(
                "PT_LOAD image size overflows",
            )))?
            .max(PAGE_SIZE_16K);

        let mut mem_region = MemoryRegion::allocate_16k(total_size, ProtFlags::READ_WRITE)?;
        let mapped_base = mem_region.as_ptr() as usize;
        let vaddr_min = usize::try_from(parsed.vaddr_min).map_err(|_| {
            LoaderError::Elf(ElfError::CorruptedData(
                "PT_LOAD virtual address is too large",
            ))
        })?;
        let load_base =
            mapped_base
                .checked_sub(vaddr_min)
                .ok_or(LoaderError::Elf(ElfError::CorruptedData(
                    "Load bias underflows",
                )))?;
        let va_to_offset = |va: u64| -> Result<usize, LoaderError> {
            let offset = va.checked_sub(parsed.vaddr_min).ok_or(LoaderError::Elf(
                ElfError::CorruptedData("Virtual address precedes PT_LOAD range"),
            ))?;
            usize::try_from(offset).map_err(|_| {
                LoaderError::Elf(ElfError::CorruptedData("Virtual address is too large"))
            })
        };

        // Zero out the entire allocated region first
        unsafe {
            std::ptr::write_bytes(mem_region.as_mut_ptr(), 0, mem_region.len());
        }

        // Step 1: Map all PT_LOAD segments
        for phdr in &parsed.load_segments {
            let seg_offset = usize::try_from(phdr.p_offset).map_err(|_| {
                LoaderError::Elf(ElfError::CorruptedData("PT_LOAD file offset is too large"))
            })?;
            let seg_filesz = usize::try_from(phdr.p_filesz).map_err(|_| {
                LoaderError::Elf(ElfError::CorruptedData("PT_LOAD file size is too large"))
            })?;
            let seg_memsz = usize::try_from(phdr.p_memsz).map_err(|_| {
                LoaderError::Elf(ElfError::CorruptedData("PT_LOAD memory size is too large"))
            })?;
            if seg_filesz > seg_memsz {
                return Err(LoaderError::Elf(ElfError::CorruptedData(
                    "PT_LOAD file size exceeds memory size",
                )));
            }
            let dest_offset = va_to_offset(phdr.p_vaddr)?;
            let dest_end = dest_offset.checked_add(seg_memsz).ok_or(LoaderError::Elf(
                ElfError::CorruptedData("PT_LOAD memory range overflows"),
            ))?;
            if dest_end > mem_region.len() {
                return Err(LoaderError::Elf(ElfError::CorruptedData(
                    "PT_LOAD segment exceeds mapped bounds",
                )));
            }

            if seg_filesz > 0 {
                let file_end = seg_offset.checked_add(seg_filesz).ok_or(LoaderError::Elf(
                    ElfError::CorruptedData("PT_LOAD file range overflows"),
                ))?;
                if file_end > elf_bytes.len() {
                    return Err(LoaderError::Elf(ElfError::BufferTooSmall {
                        expected: file_end,
                        found: elf_bytes.len(),
                    }));
                }

                unsafe {
                    std::ptr::copy_nonoverlapping(
                        elf_bytes[seg_offset..file_end].as_ptr(),
                        mem_region.as_mut_ptr().add(dest_offset),
                        seg_filesz,
                    );
                }
            }
        }

        // Step 1b: apply PT_LOAD permissions after relocations are complete.

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
                DT_STRTAB => strtab_offset = Some(va_to_offset(dyn_entry.d_val)?),
                DT_STRSZ => {
                    strtab_sz = usize::try_from(dyn_entry.d_val).map_err(|_| {
                        LoaderError::Elf(ElfError::CorruptedData("String table size is too large"))
                    })?
                }
                DT_SYMTAB => symtab_offset = Some(va_to_offset(dyn_entry.d_val)?),
                DT_RELA => rela_offset = Some(va_to_offset(dyn_entry.d_val)?),
                DT_RELASZ => {
                    rela_sz = usize::try_from(dyn_entry.d_val).map_err(|_| {
                        LoaderError::Elf(ElfError::CorruptedData(
                            "Relocation table size is too large",
                        ))
                    })?
                }
                DT_JMPREL => jmprel_offset = Some(va_to_offset(dyn_entry.d_val)?),
                DT_PLTRELSZ => {
                    pltrel_sz = usize::try_from(dyn_entry.d_val).map_err(|_| {
                        LoaderError::Elf(ElfError::CorruptedData(
                            "PLT relocation size is too large",
                        ))
                    })?
                }
                DT_INIT => init_fn = Some(va_to_offset(dyn_entry.d_val)?),
                DT_INIT_ARRAY => init_array_offset = Some(va_to_offset(dyn_entry.d_val)?),
                DT_INIT_ARRAYSZ => {
                    init_array_sz = usize::try_from(dyn_entry.d_val).map_err(|_| {
                        LoaderError::Elf(ElfError::CorruptedData("Init array size is too large"))
                    })?
                }
                DT_FINI => fini_fn = Some(va_to_offset(dyn_entry.d_val)?),
                DT_FINI_ARRAY => fini_array_offset = Some(va_to_offset(dyn_entry.d_val)?),
                DT_FINI_ARRAYSZ => {
                    fini_array_sz = usize::try_from(dyn_entry.d_val).map_err(|_| {
                        LoaderError::Elf(ElfError::CorruptedData("Fini array size is too large"))
                    })?
                }
                _ => {}
            }
        }

        // Locate string table and symbol table buffers with strict bounds checking against mem_region
        let strtab_data = if let Some(off) = strtab_offset {
            if off < mem_region.len() {
                let actual_sz = std::cmp::min(strtab_sz, mem_region.len() - off);
                let actual_ptr = (mapped_base + off) as *const u8;
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
                let actual_ptr = (mapped_base + off) as *const u8;
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
            let table_end = rela_vaddr.checked_add(total_sz).ok_or(LoaderError::Elf(
                ElfError::CorruptedData("Relocation table range overflows"),
            ))?;
            if table_end > mem_region.len() {
                return Err(LoaderError::Elf(ElfError::CorruptedData(
                    "Relocation table exceeds mapped bounds",
                )));
            }
            if !total_sz.is_multiple_of(entry_size) {
                return Err(LoaderError::Elf(ElfError::CorruptedData(
                    "Relocation table has a partial entry",
                )));
            }
            let num_relas = total_sz / entry_size;
            let rela_ptr = (mapped_base + rela_vaddr) as *const u8;

            for i in 0..num_relas {
                let start = i * entry_size;
                let rela_bytes =
                    unsafe { std::slice::from_raw_parts(rela_ptr.add(start), entry_size) };
                let rela = Elf64Rela::parse(rela_bytes)?;

                let target_offset = va_to_offset(rela.r_offset)?;
                let target_end =
                    target_offset
                        .checked_add(8)
                        .ok_or(LoaderError::RelocationFailed {
                            offset: target_offset,
                            msg: "Relocation target address overflows".to_string(),
                        })?;
                if target_end > mem_region.len() {
                    return Err(LoaderError::RelocationFailed {
                        offset: target_offset,
                        msg: "Relocation target address out of bounds".to_string(),
                    });
                }

                let target_addr = (mapped_base + target_offset) as *mut u64;
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
                        } else if sym.is_some_and(|s| s.is_weak()) {
                            // Weak unresolved symbols default to 0
                            unsafe {
                                *target_addr = 0;
                            }
                        } else {
                            return Err(LoaderError::UnresolvedSymbol(sym_name.to_string()));
                        }
                    }
                    _ => {
                        return Err(LoaderError::UnsupportedRelocation(r_type));
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
        let mut init_routines = Vec::new();
        if let Some(init) = init_fn
            && init != 0
        {
            let address = mapped_base
                .checked_add(init)
                .ok_or(LoaderError::InvalidStartupAddress(init))?;
            if address < mapped_base || address >= mapped_base + mem_region.len() {
                return Err(LoaderError::InvalidStartupAddress(address));
            }
            init_array.push(address);
            init_routines.push(InitRoutine {
                address,
                kind: InitRoutineKind::DtInit,
                order: 0,
            });
        }
        if let Some(off) = init_array_offset {
            let count = init_array_sz / 8;
            if off + init_array_sz <= mem_region.len() {
                let array_ptr = (mapped_base + off) as *const u64;
                for i in 0..count {
                    let func_ptr = unsafe { *array_ptr.add(i) } as usize;
                    if func_ptr != 0 {
                        let address = mapped_base
                            .checked_add(func_ptr)
                            .ok_or(LoaderError::InvalidStartupAddress(func_ptr))?;
                        if address < mapped_base || address >= mapped_base + mem_region.len() {
                            return Err(LoaderError::InvalidStartupAddress(address));
                        }
                        init_array.push(address);
                        init_routines.push(InitRoutine {
                            address,
                            kind: InitRoutineKind::DtInitArray,
                            order: i,
                        });
                    }
                }
            }
        }

        let mut fini_array = Vec::new();
        if let Some(fini) = fini_fn
            && fini != 0
        {
            fini_array.push(load_base + fini);
        }
        if let Some(off) = fini_array_offset {
            let count = fini_array_sz / 8;
            if off + fini_array_sz <= mem_region.len() {
                let array_ptr = (mapped_base + off) as *const u64;
                for i in 0..count {
                    let func_ptr = unsafe { *array_ptr.add(i) } as usize;
                    if func_ptr != 0 {
                        fini_array.push(func_ptr);
                    }
                }
            }
        }

        let fini_array = fini_array;
        let _ = fini_array;

        let entry_point = if parsed.ehdr.e_entry != 0 {
            let entry_offset = va_to_offset(parsed.ehdr.e_entry)?;
            Some(
                mapped_base
                    .checked_add(entry_offset)
                    .ok_or(LoaderError::InvalidStartupAddress(entry_offset))?,
            )
        } else {
            None
        };

        for phdr in &parsed.load_segments {
            let start = align_down_16k(va_to_offset(phdr.p_vaddr)?);
            let end = align_up_16k(va_to_offset(
                phdr.p_vaddr
                    .checked_add(phdr.p_memsz)
                    .ok_or(LoaderError::Elf(ElfError::CorruptedData(
                        "PT_LOAD virtual range overflows",
                    )))?,
            )?);
            if end > mem_region.len() || start >= end {
                return Err(LoaderError::Elf(ElfError::CorruptedData(
                    "PT_LOAD protection range exceeds mapped bounds",
                )));
            }
            let prot = ProtFlags(
                (if phdr.is_readable() {
                    libc::PROT_READ
                } else {
                    0
                }) | (if phdr.is_writable() {
                    libc::PROT_WRITE
                } else {
                    0
                }) | (if (phdr.p_flags & PF_X) != 0 {
                    libc::PROT_EXEC
                } else {
                    0
                }),
            );
            let ret = unsafe {
                libc::mprotect(
                    (mapped_base + start) as *mut libc::c_void,
                    end - start,
                    prot.0,
                )
            };
            if ret != 0 {
                let errno = unsafe { *libc::__errno_location() };
                return Err(LoaderError::Memory(MemoryError::ProtectionFailed {
                    addr: mapped_base + start,
                    size: end - start,
                    errno,
                }));
            }
        }

        let jni_on_load = symtab.lookup("JNI_OnLoad").map(|symbol| symbol.address);

        Ok(LoadedLibrary {
            name: name.to_string(),
            load_base,
            mem_region,
            symtab,
            init_array,
            fini_array,
            entry_point,
            init: init_fn.map(|value| load_base + value),
            jni_on_load,
            init_routines,
            lifecycle: Cell::new(LibraryLifecycle::Loaded),
        })
    }
}
