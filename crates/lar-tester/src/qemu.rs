//! QEMU AArch64 Test Runner and Process Orchestrator.
//!
//! Manages running LAR and ARM64 binaries inside `qemu-aarch64` user-space emulation,
//! configuring dynamic linker sysroot paths and capturing execution outputs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Information about QEMU AArch64 emulator on the host system.
#[derive(Debug, Clone)]
pub struct QemuEnvironment {
    pub qemu_path: Option<PathBuf>,
    pub sysroot_path: Option<PathBuf>,
    pub is_available: bool,
}

impl Default for QemuEnvironment {
    fn default() -> Self {
        Self::detect()
    }
}

impl QemuEnvironment {
    /// Detects QEMU aarch64 binary and sysroot libraries on the current host.
    pub fn detect() -> Self {
        let candidate_qemu_names = [
            "qemu-aarch64",
            "qemu-aarch64-static",
            "/usr/bin/qemu-aarch64",
            "/usr/local/bin/qemu-aarch64",
            "/usr/bin/qemu-aarch64-static",
        ];

        let mut qemu_path = None;
        for name in &candidate_qemu_names {
            if let Ok(output) = Command::new(name).arg("--version").output()
                && output.status.success()
            {
                qemu_path = Some(PathBuf::from(name));
                break;
            }
        }

        let candidate_sysroots = [
            "/usr/aarch64-linux-gnu",
            "/usr/gnemul/qemu-aarch64",
            "/usr/arm64-linux-gnu",
            "/opt/android-ndk/sysroot",
        ];

        let mut sysroot_path = None;
        for sys in &candidate_sysroots {
            let p = Path::new(sys);
            if p.exists() && p.is_dir() {
                sysroot_path = Some(p.to_path_buf());
                break;
            }
        }

        let is_available = qemu_path.is_some();

        Self {
            qemu_path,
            sysroot_path,
            is_available,
        }
    }

    /// Runs an ARM64 binary through QEMU emulator.
    pub fn run_binary<P: AsRef<Path>>(
        &self,
        binary_path: P,
        args: &[&str],
    ) -> Result<Output, std::io::Error> {
        let qemu = self.qemu_path.as_ref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "qemu-aarch64 not found on host system",
            )
        })?;

        let mut cmd = Command::new(qemu);

        if let Some(sysroot) = &self.sysroot_path {
            cmd.arg("-L").arg(sysroot);
        }

        cmd.arg(binary_path.as_ref());
        for arg in args {
            cmd.arg(arg);
        }

        cmd.output()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qemu_detection_does_not_panic() {
        let env = QemuEnvironment::detect();
        // Just verify detection completes and yields a valid struct
        assert!(env.qemu_path.is_some() || !env.is_available);
    }
}
