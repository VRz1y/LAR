//! Syscall subsystem: AArch64 syscall table, virtual procfs, and syscall dispatcher.

pub mod dispatcher;
pub mod procfs;
#[cfg(target_os = "linux")]
pub mod seccomp_notify;
pub mod table;

pub use dispatcher::{SyscallDispatcher, SyscallError};
pub use procfs::{VirtualFile, VirtualProcFs};
#[cfg(target_os = "linux")]
pub use seccomp_notify::{
    SeccompNotifyConfig, SeccompNotifyError, SeccompNotifyListener, seccomp_notify_supported,
};
pub use table::*;
