//! Architecture definitions, register contexts, and call bridges for LAR.

pub mod bridge;
pub mod context;
pub mod trampoline;

pub use bridge::{CallBridge, GuestBridgeHandler};
pub use context::Arm64CpuContext;
pub use trampoline::{Arm64ContextHandler, Arm64ContextTrampoline};

use std::fmt;

/// Target host architectures supported by LAR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostArch {
    /// 64-bit ARM (AArch64 / ARMv8-A / ARMv9).
    Arm64,
    /// 64-bit x86 (AMD64 / Intel 64).
    X86_64,
    /// 64-bit RISC-V (RV64GC / RV64GCV).
    Riscv64,
    /// Unknown or unsupported host architecture.
    Unknown,
}

impl HostArch {
    /// Detects the host architecture at runtime.
    pub const fn current() -> Self {
        #[cfg(target_arch = "aarch64")]
        {
            Self::Arm64
        }
        #[cfg(target_arch = "x86_64")]
        {
            Self::X86_64
        }
        #[cfg(target_arch = "riscv64")]
        {
            Self::Riscv64
        }
        #[cfg(not(any(
            target_arch = "aarch64",
            target_arch = "x86_64",
            target_arch = "riscv64"
        )))]
        {
            Self::Unknown
        }
    }

    /// Determines the execution mode for running ARM64 guest code on this host.
    pub const fn execution_mode(&self) -> ExecutionMode {
        match self {
            Self::Arm64 => ExecutionMode::Direct,
            Self::X86_64 | Self::Riscv64 => ExecutionMode::ForeignBridge,
            Self::Unknown => ExecutionMode::ForeignBridge,
        }
    }
}

impl fmt::Display for HostArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arm64 => write!(f, "aarch64 (ARM64)"),
            Self::X86_64 => write!(f, "x86_64 (AMD64)"),
            Self::Riscv64 => write!(f, "riscv64 (RV64)"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Execution mode for running guest code on the host system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Direct native execution (Host is ARM64).
    /// Bionic symbols are replaced with glibc/musl shims, code executes natively.
    Direct,
    /// Foreign execution via call bridges, ABI shims, and JIT translation (Host is x86_64 / RISC-V).
    ForeignBridge,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_arch_detection() {
        let arch = HostArch::current();
        #[cfg(target_arch = "x86_64")]
        {
            assert_eq!(arch, HostArch::X86_64);
            assert_eq!(arch.execution_mode(), ExecutionMode::ForeignBridge);
        }
        #[cfg(target_arch = "aarch64")]
        {
            assert_eq!(arch, HostArch::Arm64);
            assert_eq!(arch.execution_mode(), ExecutionMode::Direct);
        }
        #[cfg(target_arch = "riscv64")]
        {
            assert_eq!(arch, HostArch::Riscv64);
            assert_eq!(arch.execution_mode(), ExecutionMode::ForeignBridge);
        }
    }
}
