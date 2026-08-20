//! Opt-in Linux seccomp user-notification listener setup.

use std::fmt;
use std::io;
use std::os::fd::{AsRawFd, RawFd};

const SECCOMP_SET_MODE_FILTER: libc::c_ulong = 1;
const SECCOMP_GET_ACTION_AVAIL: libc::c_ulong = 2;
const SECCOMP_FILTER_FLAG_NEW_LISTENER: libc::c_ulong = 1 << 3;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
const SECCOMP_RET_USER_NOTIF: u32 = 0x7fc0_0000;
const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JMP: u16 = 0x05;
const BPF_JEQ: u16 = 0x10;
const BPF_K: u16 = 0x00;
const BPF_RET: u16 = 0x06;
const AUDIT_ARCH_X86_64: u32 = 0xc000003e;
const AUDIT_ARCH_AARCH64: u32 = 0xc00000b7;
const SECCOMP_DATA_NR_OFFSET: u32 = 0;
const SECCOMP_DATA_ARCH_OFFSET: u32 = 4;

#[derive(Debug)]
pub enum SeccompNotifyError {
    Unsupported,
    InvalidConfig,
    Kernel(io::Error),
}

impl fmt::Display for SeccompNotifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "Linux seccomp user notification is unsupported"),
            Self::InvalidConfig => write!(f, "seccomp notification configuration is invalid"),
            Self::Kernel(error) => write!(f, "seccomp setup failed: {error}"),
        }
    }
}

impl std::error::Error for SeccompNotifyError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeccompNotifyConfig {
    pub syscall_numbers: Vec<i32>,
}

impl SeccompNotifyConfig {
    pub fn new(syscall_numbers: Vec<i32>) -> Self {
        Self { syscall_numbers }
    }
}

#[derive(Debug)]
pub struct SeccompNotifyListener {
    fd: RawFd,
}

impl AsRawFd for SeccompNotifyListener {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Drop for SeccompNotifyListener {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

pub fn seccomp_notify_supported() -> bool {
    let mut action = SECCOMP_RET_USER_NOTIF;
    let result = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            SECCOMP_GET_ACTION_AVAIL,
            0,
            &mut action as *mut u32,
        )
    };
    result == 0
}

impl SeccompNotifyListener {
    /// Installs a seccomp filter for the configured syscalls on the calling thread
    /// and returns the associated user-notification listener.
    pub fn install(config: &SeccompNotifyConfig) -> Result<Self, SeccompNotifyError> {
        if config.syscall_numbers.is_empty()
            || config.syscall_numbers.iter().any(|number| *number < 0)
        {
            return Err(SeccompNotifyError::InvalidConfig);
        }
        if !seccomp_notify_supported() {
            return Err(SeccompNotifyError::Unsupported);
        }

        let arch = match std::env::consts::ARCH {
            "x86_64" => AUDIT_ARCH_X86_64,
            "aarch64" => AUDIT_ARCH_AARCH64,
            _ => return Err(SeccompNotifyError::Unsupported),
        };
        let mut filter = Vec::with_capacity(config.syscall_numbers.len() * 2 + 5);
        filter.push(libc::sock_filter {
            code: BPF_LD | BPF_W | BPF_ABS,
            jt: 0,
            jf: 0,
            k: SECCOMP_DATA_ARCH_OFFSET,
        });
        filter.push(libc::sock_filter {
            code: BPF_JMP | BPF_JEQ | BPF_K,
            jt: 1,
            jf: 0,
            k: arch,
        });
        filter.push(libc::sock_filter {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ERRNO | libc::EPERM as u32,
        });
        filter.push(libc::sock_filter {
            code: BPF_LD | BPF_W | BPF_ABS,
            jt: 0,
            jf: 0,
            k: SECCOMP_DATA_NR_OFFSET,
        });
        for number in &config.syscall_numbers {
            filter.push(libc::sock_filter {
                code: BPF_JMP | BPF_JEQ | BPF_K,
                jt: 0,
                jf: 1,
                k: *number as u32,
            });
            filter.push(libc::sock_filter {
                code: BPF_RET | BPF_K,
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_USER_NOTIF,
            });
        }
        filter.push(libc::sock_filter {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ALLOW,
        });
        let program = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };
        let fd = unsafe {
            libc::syscall(
                libc::SYS_seccomp,
                SECCOMP_SET_MODE_FILTER,
                SECCOMP_FILTER_FLAG_NEW_LISTENER,
                &program as *const libc::sock_fprog,
            ) as libc::c_int
        };
        if fd < 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error(),
                Some(libc::EINVAL) | Some(libc::EOPNOTSUPP) | Some(libc::ENOSYS)
            ) {
                Err(SeccompNotifyError::Unsupported)
            } else {
                Err(SeccompNotifyError::Kernel(error))
            }
        } else {
            Ok(Self { fd })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_configuration_without_installing_filter() {
        let result = SeccompNotifyListener::install(&SeccompNotifyConfig::default());
        assert!(matches!(result, Err(SeccompNotifyError::InvalidConfig)));
    }

    #[test]
    fn rejects_negative_syscall_without_installing_filter() {
        let result = SeccompNotifyListener::install(&SeccompNotifyConfig::new(vec![-1]));
        assert!(matches!(result, Err(SeccompNotifyError::InvalidConfig)));
    }

    #[test]
    fn detection_is_query_only() {
        let _ = seccomp_notify_supported();
    }
}
