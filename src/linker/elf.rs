//! Zero-dependency 64-bit ELF (AArch64) Parser and Data Structures.
//!
//! Parses ELF headers, program headers, dynamic sections, symbol tables,
//! and relocation entries for 64-bit ARM (`arm64-v8a`) binaries.

use std::fmt;

// ELF Magic & Identifiers
pub const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
pub const ELFCLASS64: u8 = 2;
pub const ELFDATA2LSB: u8 = 1;
pub const EV_CURRENT: u8 = 1;
pub const EM_AARCH64: u16 = 183;
pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;

// Program Header Types
pub const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_SHLIB: u32 = 5;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;
pub const PT_GNU_EH_FRAME: u32 = 0x6474e550;
pub const PT_GNU_STACK: u32 = 0x6474e551;
pub const PT_GNU_RELRO: u32 = 0x6474e552;

// Program Header Flags
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

// Dynamic Section Tags
pub const DT_NULL: u64 = 0;
pub const DT_NEEDED: u64 = 1;
pub const DT_PLTRELSZ: u64 = 2;
pub const DT_PLTGOT: u64 = 3;
pub const DT_HASH: u64 = 4;
pub const DT_STRTAB: u64 = 5;
pub const DT_SYMTAB: u64 = 6;
pub const DT_RELA: u64 = 7;
pub const DT_RELASZ: u64 = 8;
pub const DT_RELAENT: u64 = 9;
pub const DT_STRSZ: u64 = 10;
pub const DT_SYMENT: u64 = 11;
pub const DT_INIT: u64 = 12;
pub const DT_FINI: u64 = 13;
pub const DT_SONAME: u64 = 14;
pub const DT_RPATH: u64 = 15;
pub const DT_SYMBOLIC: u64 = 16;
pub const DT_REL: u64 = 17;
pub const DT_PLTREL: u64 = 20;
pub const DT_JMPREL: u64 = 23;
pub const DT_INIT_ARRAY: u64 = 25;
pub const DT_FINI_ARRAY: u64 = 26;
pub const DT_INIT_ARRAYSZ: u64 = 27;
pub const DT_FINI_ARRAYSZ: u64 = 28;
pub const DT_FLAGS: u64 = 30;
pub const DT_GNU_HASH: u64 = 0x6ffffef5;
pub const DT_RELACOUNT: u64 = 0x6ffffff9;
pub const DT_FLAGS_1: u64 = 0x6ffffffb;

// Symbol Bindings
pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;

// Symbol Types
pub const STT_NOTYPE: u8 = 0;
pub const STT_OBJECT: u8 = 1;
pub const STT_FUNC: u8 = 2;
pub const STT_SECTION: u8 = 3;
pub const STT_FILE: u8 = 4;
pub const STT_COMMON: u8 = 5;
pub const STT_TLS: u8 = 6;

// AArch64 Relocation Types
pub const R_AARCH64_NONE: u32 = 0;
pub const R_AARCH64_ABS64: u32 = 257;
pub const R_AARCH64_COPY: u32 = 1024;
pub const R_AARCH64_GLOB_DAT: u32 = 1025;
pub const R_AARCH64_JUMP_SLOT: u32 = 1026;
pub const R_AARCH64_RELATIVE: u32 = 1027;
pub const R_AARCH64_TLS_TPREL64: u32 = 1030;
pub const R_AARCH64_TLSDESC: u32 = 1031;

/// ELF Parsing Errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    BufferTooSmall { expected: usize, found: usize },
    InvalidMagic,
    UnsupportedClass(u8),
    UnsupportedDataEncoding(u8),
    UnsupportedMachine(u16),
    UnsupportedType(u16),
    InvalidHeaderSize,
    InvalidDynamicSection,
    StringTableNotFound,
    SymbolTableNotFound,
    CorruptedData(&'static str),
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooSmall { expected, found } => {
                write!(
                    f,
                    "ELF buffer too small: expected {} bytes, found {}",
                    expected, found
                )
            }
            Self::InvalidMagic => write!(f, "Invalid ELF magic bytes"),
            Self::UnsupportedClass(c) => {
                write!(f, "Unsupported ELF class: {} (expected 64-bit)", c)
            }
            Self::UnsupportedDataEncoding(d) => {
                write!(f, "Unsupported endianness: {} (expected 2LSB)", d)
            }
            Self::UnsupportedMachine(m) => {
                write!(f, "Unsupported machine type: {} (expected AArch64)", m)
            }
            Self::UnsupportedType(t) => {
                write!(f, "Unsupported ELF type: {} (expected ET_DYN / ET_EXEC)", t)
            }
            Self::InvalidHeaderSize => write!(f, "Invalid ELF header size"),
            Self::InvalidDynamicSection => write!(f, "Corrupted or missing ELF dynamic section"),
            Self::StringTableNotFound => write!(f, "ELF dynamic string table not found"),
            Self::SymbolTableNotFound => write!(f, "ELF dynamic symbol table not found"),
            Self::CorruptedData(msg) => write!(f, "Corrupted ELF data: {}", msg),
        }
    }
}

impl std::error::Error for ElfError {}

/// 64-bit ELF File Header (`Elf64_Ehdr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Elf64Ehdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

impl Elf64Ehdr {
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        if bytes.len() < 64 {
            return Err(ElfError::BufferTooSmall {
                expected: 64,
                found: bytes.len(),
            });
        }
        if bytes[0..4] != ELFMAG {
            return Err(ElfError::InvalidMagic);
        }
        if bytes[4] != ELFCLASS64 {
            return Err(ElfError::UnsupportedClass(bytes[4]));
        }
        if bytes[5] != ELFDATA2LSB {
            return Err(ElfError::UnsupportedDataEncoding(bytes[5]));
        }

        let e_type = u16::from_le_bytes(bytes[16..18].try_into().unwrap());
        let e_machine = u16::from_le_bytes(bytes[18..20].try_into().unwrap());
        let e_version = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        let e_entry = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        let e_phoff = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let e_shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        let e_flags = u32::from_le_bytes(bytes[48..52].try_into().unwrap());
        let e_ehsize = u16::from_le_bytes(bytes[52..54].try_into().unwrap());
        let e_phentsize = u16::from_le_bytes(bytes[54..56].try_into().unwrap());
        let e_phnum = u16::from_le_bytes(bytes[56..58].try_into().unwrap());
        let e_shentsize = u16::from_le_bytes(bytes[58..60].try_into().unwrap());
        let e_shnum = u16::from_le_bytes(bytes[60..62].try_into().unwrap());
        let e_shstrndx = u16::from_le_bytes(bytes[62..64].try_into().unwrap());

        if e_machine != EM_AARCH64 {
            return Err(ElfError::UnsupportedMachine(e_machine));
        }
        if e_type != ET_DYN && e_type != ET_EXEC {
            return Err(ElfError::UnsupportedType(e_type));
        }

        let mut e_ident = [0u8; 16];
        e_ident.copy_from_slice(&bytes[0..16]);

        Ok(Self {
            e_ident,
            e_type,
            e_machine,
            e_version,
            e_entry,
            e_phoff,
            e_shoff,
            e_flags,
            e_ehsize,
            e_phentsize,
            e_phnum,
            e_shentsize,
            e_shnum,
            e_shstrndx,
        })
    }
}

/// 64-bit ELF Program Header (`Elf64_Phdr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

impl Elf64Phdr {
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        if bytes.len() < 56 {
            return Err(ElfError::BufferTooSmall {
                expected: 56,
                found: bytes.len(),
            });
        }
        Ok(Self {
            p_type: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            p_flags: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            p_offset: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            p_vaddr: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            p_paddr: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
            p_filesz: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
            p_memsz: u64::from_le_bytes(bytes[40..48].try_into().unwrap()),
            p_align: u64::from_le_bytes(bytes[48..56].try_into().unwrap()),
        })
    }

    #[inline]
    pub fn is_load(&self) -> bool {
        self.p_type == PT_LOAD
    }

    #[inline]
    pub fn is_dynamic(&self) -> bool {
        self.p_type == PT_DYNAMIC
    }

    #[inline]
    pub fn is_readable(&self) -> bool {
        (self.p_flags & PF_R) != 0
    }

    #[inline]
    pub fn is_writable(&self) -> bool {
        (self.p_flags & PF_W) != 0
    }

    #[inline]
    pub fn is_executable(&self) -> bool {
        (self.p_flags & PF_X) != 0
    }
}

/// 64-bit ELF Dynamic Entry (`Elf64_Dyn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Elf64Dyn {
    pub d_tag: u64,
    pub d_val: u64,
}

impl Elf64Dyn {
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        if bytes.len() < 16 {
            return Err(ElfError::BufferTooSmall {
                expected: 16,
                found: bytes.len(),
            });
        }
        Ok(Self {
            d_tag: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            d_val: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        })
    }
}

/// 64-bit ELF Symbol (`Elf64_Sym`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Elf64Sym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

impl Elf64Sym {
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        if bytes.len() < 24 {
            return Err(ElfError::BufferTooSmall {
                expected: 24,
                found: bytes.len(),
            });
        }
        Ok(Self {
            st_name: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            st_info: bytes[4],
            st_other: bytes[5],
            st_shndx: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            st_value: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            st_size: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        })
    }

    #[inline]
    pub fn binding(&self) -> u8 {
        self.st_info >> 4
    }

    #[inline]
    pub fn sym_type(&self) -> u8 {
        self.st_info & 0xf
    }

    #[inline]
    pub fn is_global(&self) -> bool {
        self.binding() == STB_GLOBAL
    }

    #[inline]
    pub fn is_weak(&self) -> bool {
        self.binding() == STB_WEAK
    }

    #[inline]
    pub fn is_defined(&self) -> bool {
        self.st_shndx != 0
    }
}

/// 64-bit ELF Relocation with Addend (`Elf64_Rela`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

impl Elf64Rela {
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        if bytes.len() < 24 {
            return Err(ElfError::BufferTooSmall {
                expected: 24,
                found: bytes.len(),
            });
        }
        Ok(Self {
            r_offset: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            r_info: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            r_addend: i64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        })
    }

    #[inline]
    pub fn r_sym(&self) -> u32 {
        (self.r_info >> 32) as u32
    }

    #[inline]
    pub fn r_type(&self) -> u32 {
        (self.r_info & 0xffffffff) as u32
    }
}

/// Fully parsed ELF64 representation in memory.
#[derive(Debug, Clone)]
pub struct ParsedElf {
    pub ehdr: Elf64Ehdr,
    pub phdrs: Vec<Elf64Phdr>,
    pub dynamic_entries: Vec<Elf64Dyn>,
    pub load_segments: Vec<Elf64Phdr>,
    pub vaddr_min: u64,
    pub vaddr_max: u64,
    pub total_memsz: u64,
}

impl ParsedElf {
    /// Parses an ELF64 AArch64 binary from a byte slice.
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        let ehdr = Elf64Ehdr::parse(bytes)?;

        let mut phdrs = Vec::with_capacity(ehdr.e_phnum as usize);
        let phoff = ehdr.e_phoff as usize;
        let phentsize = ehdr.e_phentsize as usize;

        for i in 0..ehdr.e_phnum as usize {
            let entry_offset = i.checked_mul(phentsize).ok_or(ElfError::CorruptedData(
                "Program header table size overflows",
            ))?;
            let start = phoff
                .checked_add(entry_offset)
                .ok_or(ElfError::CorruptedData("Program header offset overflows"))?;
            let end = start
                .checked_add(phentsize)
                .ok_or(ElfError::CorruptedData("Program header range overflows"))?;
            if end > bytes.len() {
                return Err(ElfError::BufferTooSmall {
                    expected: end,
                    found: bytes.len(),
                });
            }
            phdrs.push(Elf64Phdr::parse(&bytes[start..end])?);
        }

        let mut load_segments = Vec::new();
        let mut vaddr_min = u64::MAX;
        let mut vaddr_max = 0u64;

        for phdr in &phdrs {
            if phdr.is_load() {
                load_segments.push(*phdr);
                if phdr.p_vaddr < vaddr_min {
                    vaddr_min = phdr.p_vaddr;
                }
                let end = phdr
                    .p_vaddr
                    .checked_add(phdr.p_memsz)
                    .ok_or(ElfError::CorruptedData(
                        "PT_LOAD virtual address range overflows",
                    ))?;
                if end > vaddr_max {
                    vaddr_max = end;
                }
            }
        }

        if load_segments.is_empty() {
            vaddr_min = 0;
            vaddr_max = 0;
        }

        let total_memsz = vaddr_max.saturating_sub(vaddr_min);

        // Parse Dynamic Section if present
        let mut dynamic_entries = Vec::new();
        for phdr in &phdrs {
            if phdr.is_dynamic() {
                let dyn_offset = phdr.p_offset as usize;
                let dyn_size = phdr.p_filesz as usize;
                let num_entries = dyn_size / 16;
                for i in 0..num_entries {
                    let start = dyn_offset + i * 16;
                    let end = start + 16;
                    if end <= bytes.len() {
                        let dyn_entry = Elf64Dyn::parse(&bytes[start..end])?;
                        if dyn_entry.d_tag == DT_NULL {
                            break;
                        }
                        dynamic_entries.push(dyn_entry);
                    }
                }
                break;
            }
        }

        Ok(Self {
            ehdr,
            phdrs,
            dynamic_entries,
            load_segments,
            vaddr_min,
            vaddr_max,
            total_memsz,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elf_header_validation() {
        let mut buf = vec![0u8; 64];
        buf[0..4].copy_from_slice(&ELFMAG);
        buf[4] = ELFCLASS64;
        buf[5] = ELFDATA2LSB;
        buf[6] = 1; // version
        buf[16..18].copy_from_slice(&ET_DYN.to_le_bytes());
        buf[18..20].copy_from_slice(&EM_AARCH64.to_le_bytes());
        buf[20..24].copy_from_slice(&1u32.to_le_bytes());
        buf[52..54].copy_from_slice(&64u16.to_le_bytes());

        let ehdr = Elf64Ehdr::parse(&buf).unwrap();
        assert_eq!(ehdr.e_machine, EM_AARCH64);
        assert_eq!(ehdr.e_type, ET_DYN);
    }

    #[test]
    fn test_elf_invalid_magic() {
        let buf = [0u8; 64];
        let err = Elf64Ehdr::parse(&buf).unwrap_err();
        assert_eq!(err, ElfError::InvalidMagic);
    }
}
