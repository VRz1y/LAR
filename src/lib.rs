//! LAR - Linux Android Runtime
//!
//! High-performance, multi-architecture Android NDK runtime supporting:
//! - 16KB page masking & alignment across all host targets
//! - User-space 64-bit ELF (AArch64) dynamic linker with Bionic runtime shims
//! - Multi-Target JIT Engine (Tier-0 Fast JIT, x86_64, RISC-V, AArch64 Passthrough)
//! - Disk MMAP Execution Cache and Install-Time Pre-JIT Daemon
//! - Syscall virtualization (ProcFS / Seccomp fallback) and Signal dispatching

pub mod aidl;
pub mod api_policy;
pub mod arch;
pub mod art;
pub mod audio;
pub mod bionic;
pub mod dex;
pub mod graphics;
pub mod hidl;
pub mod ipc;
pub mod jit;
pub mod lifecycle;
pub mod linker;
pub mod managers;
pub mod memory;
pub mod prejit;
pub mod signal;
pub mod syscall;

use crate::arch::{ExecutionMode, HostArch};
use crate::art::{AndroidRuntimeBundle, ArtRuntime, RuntimeBundleCatalog};
use crate::audio::AudioRuntime;
use crate::bionic::register_bionic_shims;
use crate::graphics::GraphicsRuntime;
use crate::ipc::BinderRegistry;
use crate::jit::{CacheError, JitEngine};
use crate::lifecycle::{InputDispatcher, RuntimeLifecycle};
use crate::linker::{ElfLoader, LoadedLibrary, LoaderError, SymbolRegistry};
use crate::managers::ApplicationInfo;
use crate::managers::{ActivityManager, PackageManager, WindowManager};
use crate::prejit::{Phase3Readiness, Phase3StartupContract};
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
    pub jit_engine: JitEngine,
    pub graphics: GraphicsRuntime,
    pub audio: AudioRuntime,
    pub loaded_libraries: Vec<LoadedLibrary>,
    pub phase3_startup: Vec<Phase3StartupContract>,
    pub art: ArtRuntime,
    pub binder: BinderRegistry,
    pub activity_manager: ActivityManager,
    pub package_manager: PackageManager,
    pub window_manager: WindowManager,
    pub input_dispatcher: InputDispatcher,
    pub lifecycle: RuntimeLifecycle,
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
        let jit_engine = JitEngine::new();
        let graphics = GraphicsRuntime::new();
        let audio = AudioRuntime::new();
        let art = ArtRuntime::discover();
        let binder = BinderRegistry::new();
        binder.register_core_services();
        let activity_manager = ActivityManager::new();
        let package_manager = PackageManager::new();
        let window_manager = WindowManager::new();
        let input_dispatcher = InputDispatcher::default();

        Self {
            host_arch,
            execution_mode,
            symbol_registry,
            syscall_dispatcher,
            signal_dispatcher,
            jit_engine,
            graphics,
            audio,
            loaded_libraries: Vec::new(),
            phase3_startup: Vec::new(),
            art,
            binder,
            activity_manager,
            package_manager,
            window_manager,
            input_dispatcher,
            lifecycle: RuntimeLifecycle::Created,
        }
    }

    /// Loads an ARM64 ELF shared library from a byte buffer.
    pub fn load_library(
        &mut self,
        name: &str,
        bytes: &[u8],
    ) -> Result<&LoadedLibrary, LoaderError> {
        let loaded = ElfLoader::load_from_memory(name, bytes, &mut self.symbol_registry)?;

        // Update virtual /proc/self/maps with the loaded library segment
        let start = loaded.load_base;
        let end = start + loaded.mem_region.len();
        let name_owned = loaded.name.clone();

        self.loaded_libraries.push(loaded);
        let last_ref = self.loaded_libraries.last().unwrap();

        // Update virtual procfs maps
        let mappings = vec![(start, end, "r-xp", name_owned.as_str())];
        self.syscall_dispatcher
            .procfs()
            .update_virtual_maps(&mappings);

        Ok(last_ref)
    }

    /// Loads an ARM64 ELF shared library from a file path.
    pub fn load_library_file<P: AsRef<Path>>(
        &mut self,
        path: P,
    ) -> Result<&LoadedLibrary, LoaderError> {
        let p = path.as_ref();
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown.so");
        let bytes = fs::read(p)?;
        self.load_library(name, &bytes)
    }

    pub fn load_execution_cache<P: AsRef<Path>>(&mut self, path: P) -> Result<(), CacheError> {
        self.jit_engine.load_cache(path)
    }

    /// Resolves a symbol across all loaded libraries and Bionic shims.
    pub fn resolve_symbol(&self, name: &str) -> Option<usize> {
        self.symbol_registry.resolve(name)
    }

    /// Returns the total number of loaded libraries.
    pub fn loaded_library_count(&self) -> usize {
        self.loaded_libraries.len()
    }

    pub fn prepare_phase3_startup(&mut self) -> &[Phase3StartupContract] {
        self.phase3_startup.clear();
        for library in &self.loaded_libraries {
            library.prepare_startup();
            self.phase3_startup.push(Phase3StartupContract {
                library: library.name.clone(),
                init_routines: library
                    .init_routines
                    .iter()
                    .map(|routine| routine.address)
                    .collect(),
                jni_on_load: library.jni_on_load,
            });
        }
        &self.phase3_startup
    }

    pub fn native_startup_ready(&self) -> bool {
        !self.phase3_startup.is_empty()
            && self
                .phase3_startup
                .iter()
                .all(Phase3StartupContract::is_ready)
    }

    pub fn phase3_readiness(&self) -> Phase3Readiness {
        Phase3Readiness {
            native_startup_contracts: self
                .phase3_startup
                .iter()
                .filter(|contract| contract.is_ready())
                .count(),
            manifest_available: self.package_manager.len() > 0,
            art_available: self.art.is_initialized(),
            binder_available: self.binder.is_available(),
            managers_available: true,
        }
    }

    pub fn is_phase3_ready(&self) -> bool {
        self.phase3_readiness().can_start_art()
    }

    pub fn install_application(&mut self, application: ApplicationInfo) {
        self.package_manager.install_application(application);
    }

    pub fn configure_runtime_bundle(
        &mut self,
        bundle: &AndroidRuntimeBundle,
    ) -> Result<(), crate::art::ArtError> {
        self.art = ArtRuntime::from_bundle(bundle);
        self.art.initialize()
    }

    pub fn configure_runtime_from_catalog(
        &mut self,
        catalog: &RuntimeBundleCatalog,
        min_sdk: Option<u32>,
        target_sdk: Option<u32>,
    ) -> Result<(), crate::art::ArtError> {
        let bundle = catalog.resolve_for_apk(min_sdk, target_sdk)?;
        self.configure_runtime_bundle(&bundle)
    }

    pub fn start_application(&mut self, package: &str) -> Result<u64, StartApplicationError> {
        let application = self
            .package_manager
            .application(package)
            .ok_or(StartApplicationError::UnknownPackage)?
            .clone();
        self.art
            .start_application(&application.package, application.dex_path.as_deref())
            .map_err(StartApplicationError::Art)?;
        let activity = application
            .launcher_activity
            .ok_or(StartApplicationError::MissingLauncher)?;
        let id = self.activity_manager.start(package, activity);
        self.lifecycle = RuntimeLifecycle::Started;
        Ok(id)
    }
}

#[derive(Debug)]
pub enum StartApplicationError {
    UnknownPackage,
    MissingLauncher,
    Art(crate::art::ArtError),
}

impl std::fmt::Display for StartApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "application start error: {:?}", self)
    }
}
impl std::error::Error for StartApplicationError {}
