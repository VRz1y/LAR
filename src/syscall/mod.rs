//! Syscall subsystem: AArch64 syscall table, virtual procfs, and syscall dispatcher.

pub mod dispatcher;
pub mod procfs;
pub mod table;

pub use dispatcher::{SyscallDispatcher, SyscallError};
pub use procfs::{VirtualFile, VirtualProcFs};
pub use table::*;
