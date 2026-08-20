//! Pre-JIT and Install-Time Compilation Subsystem.

pub mod callgraph;
pub mod daemon;
pub mod profile;

pub use callgraph::{CallgraphAnalyzer, StartupCallNode};
pub use daemon::PreJitDaemon;
pub use profile::{BaselineProfileParser, BaselineProfileSummary};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase3StartupContract {
    pub library: String,
    pub init_routines: Vec<usize>,
    pub jni_on_load: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase3Readiness {
    pub native_startup_contracts: usize,
    pub manifest_available: bool,
    pub art_available: bool,
    pub binder_available: bool,
    pub managers_available: bool,
}

impl Phase3Readiness {
    pub fn can_start_art(&self) -> bool {
        self.native_startup_contracts > 0
            && self.manifest_available
            && self.art_available
            && self.binder_available
            && self.managers_available
    }
}

impl Phase3StartupContract {
    pub fn is_ready(&self) -> bool {
        !self.init_routines.is_empty() || self.jni_on_load.is_some()
    }
}
