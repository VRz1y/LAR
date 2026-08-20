//! Linker subsystem for LAR: ELF parsing, dynamic linking, relocation resolution, and symbol tables.

pub mod elf;
pub mod loader;
pub mod symbols;

pub use elf::{Elf64Dyn, Elf64Ehdr, Elf64Phdr, Elf64Rela, Elf64Sym, ElfError, ParsedElf};
pub use loader::{
    ElfLoader, InitRoutine, InitRoutineKind, LibraryLifecycle, LoadedLibrary, LoaderError,
};
pub use symbols::{DynamicSymbolTable, Symbol, SymbolRegistry};
