//! Unit and Integration Tests for AArch64 Syscall Dispatcher and ProcFS Virtualization.

use lar::arch::Arm64CpuContext;
use lar::memory::is_16k_aligned;
use lar::syscall::*;
use std::ffi::CString;

#[test]
fn test_syscall_name_mapping() {
    assert_eq!(arm64_syscall_name(ARM64_NR_READ), "read");
    assert_eq!(arm64_syscall_name(ARM64_NR_WRITE), "write");
    assert_eq!(arm64_syscall_name(ARM64_NR_OPENAT), "openat");
    assert_eq!(arm64_syscall_name(ARM64_NR_MMAP), "mmap");
    assert_eq!(arm64_syscall_name(ARM64_NR_GETPID), "getpid");
}

#[test]
fn test_syscall_identity_calls() {
    let dispatcher = SyscallDispatcher::new();
    let mut ctx = Arm64CpuContext::new();

    // getpid
    ctx.regs[8] = ARM64_NR_GETPID as u64;
    dispatcher.dispatch(&mut ctx);
    assert_eq!(ctx.get_return() as i64, unsafe { libc::getpid() as i64 });

    // getuid
    ctx.regs[8] = ARM64_NR_GETUID as u64;
    dispatcher.dispatch(&mut ctx);
    assert_eq!(ctx.get_return() as i64, unsafe { libc::getuid() as i64 });

    // getgid
    ctx.regs[8] = ARM64_NR_GETGID as u64;
    dispatcher.dispatch(&mut ctx);
    assert_eq!(ctx.get_return() as i64, unsafe { libc::getgid() as i64 });
}

#[test]
fn test_syscall_mmap_16k_alignment() {
    let dispatcher = SyscallDispatcher::new();
    let mut ctx = Arm64CpuContext::new();

    // mmap(NULL, 1000, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)
    ctx.regs[8] = ARM64_NR_MMAP as u64;
    ctx.regs[0] = 0;
    ctx.regs[1] = 1000;
    ctx.regs[2] = (libc::PROT_READ | libc::PROT_WRITE) as u64;
    ctx.regs[3] = (libc::MAP_PRIVATE | libc::MAP_ANONYMOUS) as u64;
    ctx.regs[4] = !0u64; // -1
    ctx.regs[5] = 0;

    dispatcher.dispatch(&mut ctx);
    let mapped_addr = ctx.get_return() as usize;
    assert!(mapped_addr > 0);
    assert!(is_16k_aligned(mapped_addr), "mmap must return a 16KB aligned address");

    // Test mprotect
    ctx.regs[8] = ARM64_NR_MPROTECT as u64;
    ctx.regs[0] = mapped_addr as u64;
    ctx.regs[1] = 1000;
    ctx.regs[2] = libc::PROT_READ as u64;
    dispatcher.dispatch(&mut ctx);
    assert_eq!(ctx.get_return(), 0);

    // Test munmap
    ctx.regs[8] = ARM64_NR_MUNMAP as u64;
    ctx.regs[0] = mapped_addr as u64;
    ctx.regs[1] = 1000;
    dispatcher.dispatch(&mut ctx);
    assert_eq!(ctx.get_return(), 0);
}

#[test]
fn test_virtual_procfs_routing() {
    let dispatcher = SyscallDispatcher::new();
    let mut ctx = Arm64CpuContext::new();

    let paths = [
        "/proc/cpuinfo",
        "/proc/version",
        "/proc/self/cmdline",
    ];

    for path_str in paths {
        let c_path = CString::new(path_str).unwrap();

        // 1. Open
        ctx.regs[8] = ARM64_NR_OPENAT as u64;
        ctx.regs[0] = libc::AT_FDCWD as u64;
        ctx.regs[1] = c_path.as_ptr() as u64;
        ctx.regs[2] = libc::O_RDONLY as u64;

        dispatcher.dispatch(&mut ctx);
        let fd = ctx.get_return() as i32;
        assert!(fd >= 0x7000_0000, "Expected virtual FD for {}", path_str);

        // 2. Read
        let mut buf = [0u8; 256];
        ctx.regs[8] = ARM64_NR_READ as u64;
        ctx.regs[0] = fd as u64;
        ctx.regs[1] = buf.as_mut_ptr() as u64;
        ctx.regs[2] = buf.len() as u64;

        dispatcher.dispatch(&mut ctx);
        let read_n = ctx.get_return() as i64;
        assert!(read_n > 0, "Expected positive read for {}", path_str);

        // 3. Close
        ctx.regs[8] = ARM64_NR_CLOSE as u64;
        ctx.regs[0] = fd as u64;
        dispatcher.dispatch(&mut ctx);
        assert_eq!(ctx.get_return(), 0);
    }
}
