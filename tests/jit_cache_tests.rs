//! Unit Tests for MMAP Execution Cache.

use lar::arch::{Arm64CpuContext, HostArch};
use lar::jit::cache::*;
use lar::memory::is_16k_aligned;

#[test]
fn test_cache_serialization_and_mmap_execution() {
    let temp_dir = std::env::temp_dir();
    let cache_file = temp_dir.join("lar_test_suite.larcache");

    // Block 1: returns 77 (x86_64: mov rax, 77; ret -> 48 c7 c0 4d 00 00 00 c3)
    let block1 = CompiledBlock {
        block_hash: 0xaaaa_bbbb_cccc_dddd,
        guest_pc: 0x0040_0000,
        machine_code: vec![0x48, 0xc7, 0xc0, 77, 0x00, 0x00, 0x00, 0xc3],
    };

    // Block 2: returns 88 (x86_64: mov rax, 88; ret -> 48 c7 c0 58 00 00 00 c3)
    let block2 = CompiledBlock {
        block_hash: 0x1111_2222_3333_4444,
        guest_pc: 0x0040_1000,
        machine_code: vec![0x48, 0xc7, 0xc0, 88, 0x00, 0x00, 0x00, 0xc3],
    };

    MmapExecutionCache::create_and_save(&cache_file, HostArch::X86_64, &[block1, block2])
        .expect("Failed to create execution cache");

    let cache = MmapExecutionCache::load_from_file(&cache_file, HostArch::X86_64)
        .expect("Failed to load execution cache");

    assert!(is_16k_aligned(cache.region.as_ptr() as usize));
    assert_eq!(cache.entries.len(), 2);

    // Call Block 1
    let fn1_ptr = cache
        .lookup_block(0xaaaa_bbbb_cccc_dddd)
        .expect("Block 1 not found");
    let func1: extern "C" fn() -> u64 = unsafe { std::mem::transmute(fn1_ptr) };
    assert_eq!(func1(), 77);

    // Call Block 2
    let fn2_ptr = cache
        .lookup_block(0x1111_2222_3333_4444)
        .expect("Block 2 not found");
    let func2: extern "C" fn() -> u64 = unsafe { std::mem::transmute(fn2_ptr) };
    assert_eq!(func2(), 88);

    let _ = std::fs::remove_file(cache_file);
}

#[test]
fn test_context_hash_changes_with_guest_state() {
    let first = Arm64CpuContext::new();
    let mut second = first;
    second.regs[3] = 42;

    assert_ne!(
        hash_arm64_block_with_context(&first, &[0xd503201f]),
        hash_arm64_block_with_context(&second, &[0xd503201f])
    );
}
