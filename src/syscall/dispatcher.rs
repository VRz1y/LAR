//! AArch64 Syscall Dispatcher, Virtualization, and Seccomp Handling.
//!
//! Maps ARM64 syscall numbers (passed in x8) and arguments (x0-x5),
//! routes virtual procfs requests, enforces 16KB memory page alignment,
//! and handles direct `svc #0` fallback.

use crate::arch::context::Arm64CpuContext;
use crate::memory::mmap::{MemoryRegion, ProtFlags};
use crate::memory::page::{align_down_16k, align_up_16k};
use crate::syscall::procfs::VirtualProcFs;
use crate::syscall::table::*;
use std::ffi::CStr;
use std::sync::Arc;

/// Syscall Execution Errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyscallError {
    InvalidMemoryAccess(usize),
    UnsupportedSyscall(u32),
    IoError(i32),
}

/// Syscall Dispatcher managing guest syscall execution.
pub struct SyscallDispatcher {
    procfs: Arc<VirtualProcFs>,
}

impl Default for SyscallDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SyscallDispatcher {
    pub fn new() -> Self {
        Self {
            procfs: Arc::new(VirtualProcFs::new()),
        }
    }

    pub fn with_procfs(procfs: Arc<VirtualProcFs>) -> Self {
        Self { procfs }
    }

    pub fn procfs(&self) -> &Arc<VirtualProcFs> {
        &self.procfs
    }

    /// Dispatches an AArch64 syscall from the given CPU context.
    /// In AArch64 Linux ABI:
    /// - `x8` holds syscall number
    /// - `x0..x5` hold arguments 1 through 6
    /// - `x0` receives the return code (negative for errno, e.g. -EFAULT)
    pub fn dispatch(&self, ctx: &mut Arm64CpuContext) {
        let nr = ctx.regs[8] as u32;
        let a0 = ctx.regs[0];
        let a1 = ctx.regs[1];
        let a2 = ctx.regs[2];
        let a3 = ctx.regs[3];
        let a4 = ctx.regs[4];
        let a5 = ctx.regs[5];

        let ret = self.handle_syscall(nr, a0, a1, a2, a3, a4, a5);
        ctx.set_return(ret as u64);
    }

    fn handle_syscall(
        &self,
        nr: u32,
        a0: u64,
        a1: u64,
        a2: u64,
        a3: u64,
        a4: u64,
        a5: u64,
    ) -> i64 {
        match nr {
            ARM64_NR_GETPID => unsafe { libc::getpid() as i64 },
            ARM64_NR_GETPPID => unsafe { libc::getppid() as i64 },
            ARM64_NR_GETUID => unsafe { libc::getuid() as i64 },
            ARM64_NR_GETEUID => unsafe { libc::geteuid() as i64 },
            ARM64_NR_GETGID => unsafe { libc::getgid() as i64 },
            ARM64_NR_GETEGID => unsafe { libc::getegid() as i64 },
            ARM64_NR_GETTID => {
                #[cfg(target_os = "linux")]
                unsafe {
                    libc::syscall(libc::SYS_gettid) as i64
                }
                #[cfg(not(target_os = "linux"))]
                unsafe {
                    libc::getpid() as i64
                }
            }

            ARM64_NR_WRITE => {
                let fd = a0 as i32;
                let buf = a1 as *const libc::c_void;
                let count = a2 as usize;
                if buf.is_null() && count > 0 {
                    return -libc::EFAULT as i64;
                }
                let res = unsafe { libc::write(fd, buf, count) };
                if res < 0 {
                    -unsafe { *libc::__errno_location() as i64 }
                } else {
                    res as i64
                }
            }

            ARM64_NR_READ => {
                let fd = a0 as i32;
                let buf = a1 as *mut u8;
                let count = a2 as usize;

                if buf.is_null() && count > 0 {
                    return -libc::EFAULT as i64;
                }

                // Check if reading from virtual procfs FD
                if self.procfs.is_virtual_fd(fd) {
                    let dest_slice = unsafe { std::slice::from_raw_parts_mut(buf, count) };
                    if let Some(n) = self.procfs.read(fd, dest_slice) {
                        return n as i64;
                    } else {
                        return -libc::EBADF as i64;
                    }
                }

                let res = unsafe { libc::read(fd, buf as *mut libc::c_void, count) };
                if res < 0 {
                    -unsafe { *libc::__errno_location() as i64 }
                } else {
                    res as i64
                }
            }

            ARM64_NR_OPENAT => {
                let dfd = a0 as i32;
                let path_ptr = a1 as *const libc::c_char;
                let flags = a2 as i32;
                let mode = a3 as libc::mode_t;

                if path_ptr.is_null() {
                    return -libc::EFAULT as i64;
                }

                let path_str = unsafe {
                    match CStr::from_ptr(path_ptr).to_str() {
                        Ok(s) => s,
                        Err(_) => return -libc::EINVAL as i64,
                    }
                };

                // Virtual procfs interception
                if self.procfs.is_virtual_path(path_str) {
                    if let Some(vfd) = self.procfs.open(path_str) {
                        return vfd as i64;
                    } else {
                        return -libc::ENOENT as i64;
                    }
                }

                let res = unsafe { libc::openat(dfd, path_ptr, flags, mode) };
                if res < 0 {
                    -unsafe { *libc::__errno_location() as i64 }
                } else {
                    res as i64
                }
            }

            ARM64_NR_CLOSE => {
                let fd = a0 as i32;
                if self.procfs.is_virtual_fd(fd) {
                    self.procfs.close(fd);
                    return 0;
                }
                let res = unsafe { libc::close(fd) };
                if res < 0 {
                    -unsafe { *libc::__errno_location() as i64 }
                } else {
                    0
                }
            }

            ARM64_NR_MMAP => {
                let addr = a0 as usize;
                let len = a1 as usize;
                let prot = a2 as i32;
                let flags = a3 as i32;
                let fd = a4 as i32;
                let offset = a5 as libc::off_t;

                let aligned_len = align_up_16k(len);

                // If addr == 0 and anonymous mapping, guarantee 16KB alignment
                if addr == 0 && (flags & libc::MAP_ANONYMOUS) != 0 {
                    match MemoryRegion::allocate_16k(aligned_len, ProtFlags(prot)) {
                        Ok(region) => {
                            let ptr = region.as_ptr() as usize;
                            std::mem::forget(region); // Leave mapped in process
                            return ptr as i64;
                        }
                        Err(_) => {
                            return -unsafe { *libc::__errno_location() as i64 };
                        }
                    }
                }

                let aligned_addr = if addr != 0 {
                    align_down_16k(addr)
                } else {
                    0
                };

                let ptr = unsafe {
                    libc::mmap(
                        aligned_addr as *mut libc::c_void,
                        aligned_len,
                        prot,
                        flags,
                        fd,
                        offset,
                    )
                };

                if ptr == libc::MAP_FAILED {
                    -unsafe { *libc::__errno_location() as i64 }
                } else {
                    ptr as usize as i64
                }
            }

            ARM64_NR_MUNMAP => {
                let addr = a0 as usize;
                let len = a1 as usize;
                let aligned_addr = align_down_16k(addr);
                let aligned_len = align_up_16k(len);

                let res = unsafe { libc::munmap(aligned_addr as *mut libc::c_void, aligned_len) };
                if res < 0 {
                    -unsafe { *libc::__errno_location() as i64 }
                } else {
                    0
                }
            }

            ARM64_NR_MPROTECT => {
                let addr = a0 as usize;
                let len = a1 as usize;
                let prot = a2 as i32;
                let aligned_addr = align_down_16k(addr);
                let aligned_len = align_up_16k(len);

                let res = unsafe { libc::mprotect(aligned_addr as *mut libc::c_void, aligned_len, prot) };
                if res < 0 {
                    -unsafe { *libc::__errno_location() as i64 }
                } else {
                    0
                }
            }

            ARM64_NR_CLOCK_GETTIME => {
                let clk_id = a0 as libc::clockid_t;
                let tp = a1 as *mut libc::timespec;
                if tp.is_null() {
                    return -libc::EFAULT as i64;
                }
                let res = unsafe { libc::clock_gettime(clk_id, tp) };
                if res < 0 {
                    -unsafe { *libc::__errno_location() as i64 }
                } else {
                    0
                }
            }

            ARM64_NR_SCHED_YIELD => {
                unsafe { libc::sched_yield() as i64 }
            }

            ARM64_NR_EXIT => {
                // Exit thread / return 0
                0
            }

            ARM64_NR_EXIT_GROUP => {
                0
            }

            _ => {
                // Return -ENOSYS for unimplemented syscalls
                -(libc::ENOSYS as i64)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_getpid() {
        let dispatcher = SyscallDispatcher::new();
        let mut ctx = Arm64CpuContext::new();
        ctx.regs[8] = ARM64_NR_GETPID as u64;

        dispatcher.dispatch(&mut ctx);
        let ret_pid = ctx.get_return() as i64;
        assert!(ret_pid > 0);
        assert_eq!(ret_pid, unsafe { libc::getpid() as i64 });
    }

    #[test]
    fn test_syscall_virtual_open_read() {
        let dispatcher = SyscallDispatcher::new();
        let mut ctx = Arm64CpuContext::new();

        // 1. Open /proc/cpuinfo
        let path = b"/proc/cpuinfo\0";
        ctx.regs[8] = ARM64_NR_OPENAT as u64;
        ctx.regs[0] = libc::AT_FDCWD as u64;
        ctx.regs[1] = path.as_ptr() as u64;
        ctx.regs[2] = libc::O_RDONLY as u64;

        dispatcher.dispatch(&mut ctx);
        let fd = ctx.get_return() as i32;
        assert!(fd >= 0x7000_0000); // Virtual FD

        // 2. Read from virtual FD
        let mut buf = [0u8; 128];
        ctx.regs[8] = ARM64_NR_READ as u64;
        ctx.regs[0] = fd as u64;
        ctx.regs[1] = buf.as_mut_ptr() as u64;
        ctx.regs[2] = buf.len() as u64;

        dispatcher.dispatch(&mut ctx);
        let bytes_read = ctx.get_return() as i64;
        assert!(bytes_read > 0);
        let str_out = std::str::from_utf8(&buf[..bytes_read as usize]).unwrap();
        assert!(str_out.contains("Qualcomm") || str_out.contains("processor"));

        // 3. Close virtual FD
        ctx.regs[8] = ARM64_NR_CLOSE as u64;
        ctx.regs[0] = fd as u64;
        dispatcher.dispatch(&mut ctx);
        assert_eq!(ctx.get_return(), 0);
    }
}
