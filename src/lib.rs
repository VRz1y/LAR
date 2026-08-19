//! LAR - Linux Android Runtime
//!
//! High-performance, multi-architecture Android NDK runtime supporting:
//! - 16KB page masking & alignment across all host targets
//! - User-space 64-bit ELF (AArch64) dynamic linker with Bionic runtime shims
//! - Syscall virtualization (ProcFS / Seccomp fallback) and Signal dispatching

pub mod arch;
pub mod bionic;
pub mod linker;
pub mod memory;
pub mod signal;
pub mod syscall;

use crate::arch::{ExecutionMode, HostArch};
use crate::bionic::register_bionic_shims;
use crate::linker::{ElfLoader, LoadedLibrary, LoaderError, SymbolRegistry};
use crate::signal::SignalDispatcher;
use crate::syscall::SyscallDispatcher;
use std::fs;
use std::path::Path;

/// Top-level coordinator for the LAR Android execution environment.
pub struct LarRuntime {
    pub host_arch: HostArch,
    pub execution_mode: ExecutionMode,
    pub symbol_registry: SymbolRegistry,
    pub syscall_dispatcher: SyscallDispatcher,
    pub signal_dispatcher: SignalDispatcher,
    pub loaded_libraries: Vec<LoadedLibrary>,
}

impl Default for LarRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl LarRuntime {
    /// Creates a new LAR runtime instance with Bionic shims pre-registered.
    pub fn new() -> Self {
        let host_arch = HostArch::current();
        let execution_mode = host_arch.execution_mode();

        let mut symbol_registry = SymbolRegistry::new();
        register_bionic_shims(&mut symbol_registry);

        let syscall_dispatcher = SyscallDispatcher::new();
        let signal_dispatcher = SignalDispatcher::new();

        Self {
            host_arch,
            execution_mode,
            symbol_registry,
            syscall_dispatcher,
            signal_dispatcher,
            loaded_libraries: Vec::new(),
        }
    }

    /// Loads an ARM64 ELF shared library from a byte buffer.
    pub fn load_library(&mut self, name: &str, bytes: &[u8]) -> Result<&LoadedLibrary, LoaderError> {
        let loaded = ElfLoader::load_from_memory(name, bytes, &mut self.symbol_registry)?;
        
        // Update virtual /proc/self/maps with the loaded library segment
        let start = loaded.load_base;
        let end = start + loaded.mem_region.len();
        let name_owned = loaded.name.clone();
        
        self.loaded_libraries.push(loaded);
        let last_ref = self.loaded_libraries.last().unwrap();

        // Update virtual procfs maps
        let mappings = vec![(start, end, "r-xp", name_owned.as_str())];
        self.syscall_dispatcher.procfs().update_virtual_maps(&mappings);

        Ok(last_ref)
    }

    /// Loads an ARM64 ELF shared library from a file path.
    pub fn load_library_file<P: AsRef<Path>>(&mut self, path: P) -> Result<&LoadedLibrary, LoaderError> {
        let p = path.as_ref();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("unknown.so");
        let bytes = fs::read(p)?;
        self.load_library(name, &bytes)
    }

    /// Resolves a symbol across all loaded libraries and Bionic shims.
    pub fn resolve_symbol(&self, name: &str) -> Option<usize> {
        self.symbol_registry.resolve(name)
    }

    /// Returns the total number of loaded libraries.
    pub fn loaded_library_count(&self) -> usize {
        self.loaded_libraries.len()
    }
}
