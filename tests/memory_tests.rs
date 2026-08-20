//! Comprehensive Unit Tests for LAR 16KB Memory Subsystem.

use lar::memory::*;

#[test]
fn test_page_alignment_constants_and_masks() {
    assert_eq!(PAGE_SIZE_16K, 16384);
    assert_eq!(PAGE_SIZE_4K, 4096);
    assert_eq!(PAGE_OFFSET_MASK_16K, 0x3FFF);
    assert_eq!(PAGE_MASK_16K, !0x3FFF);
}

#[test]
fn test_is_16k_aligned_exhaustive() {
    for i in 0..100 {
        let aligned_addr = i * PAGE_SIZE_16K;
        assert!(is_16k_aligned(aligned_addr));
        if i > 0 {
            assert!(!is_16k_aligned(aligned_addr + 1));
            assert!(!is_16k_aligned(aligned_addr + 0x1000));
            assert!(!is_16k_aligned(aligned_addr + 0x2000));
            assert!(!is_16k_aligned(aligned_addr + 0x3FFF));
        }
    }
}

#[test]
fn test_align_up_and_down_edge_cases() {
    assert_eq!(align_up_16k(0), 0);
    assert_eq!(align_up_16k(1), 0x4000);
    assert_eq!(align_up_16k(0x3FFF), 0x4000);
    assert_eq!(align_up_16k(0x4000), 0x4000);
    assert_eq!(align_up_16k(0x4001), 0x8000);

    assert_eq!(align_down_16k(0), 0);
    assert_eq!(align_down_16k(1), 0);
    assert_eq!(align_down_16k(0x3FFF), 0);
    assert_eq!(align_down_16k(0x4000), 0x4000);
    assert_eq!(align_down_16k(0x4001), 0x4000);
    assert_eq!(align_down_16k(0x7FFF), 0x4000);
    assert_eq!(align_down_16k(0x8000), 0x8000);
}

#[test]
fn test_page_span_calculation() {
    assert_eq!(page_count_16k(0, 0), 0);
    assert_eq!(page_count_16k(0, 1), 1);
    assert_eq!(page_count_16k(0, 0x4000), 1);
    assert_eq!(page_count_16k(0, 0x4001), 2);
    assert_eq!(page_count_16k(0x1000, 0x3000), 1); // 0x1000 to 0x4000 fits in 1 page (0..0x4000)
    assert_eq!(page_count_16k(0x3FFF, 2), 2); // 0x3FFF..0x4001 spans 2 pages
}

#[test]
fn test_memory_region_allocation_and_16k_guarantee() {
    let mut region = MemoryRegion::allocate_16k(100, ProtFlags::READ_WRITE).unwrap();
    let addr = region.as_ptr() as usize;

    assert!(
        is_16k_aligned(addr),
        "Allocated region must be 16KB aligned: 0x{:x}",
        addr
    );
    assert!(region.len() >= PAGE_SIZE_16K);

    let test_bytes = [0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe];
    region.write_at(0x100, &test_bytes).unwrap();
    let read_back = region.read_at(0x100, test_bytes.len()).unwrap();
    assert_eq!(read_back, test_bytes);
}

#[test]
fn test_memory_protection_transitions() {
    let mut region = MemoryRegion::allocate_16k(PAGE_SIZE_16K, ProtFlags::READ_WRITE).unwrap();
    region.write_at(0, &[42, 43, 44]).unwrap();

    // Change to READ-only
    region.protect(ProtFlags::READ).unwrap();
    assert_eq!(region.prot(), ProtFlags::READ);
    assert!(region.as_slice().is_ok());
    assert!(region.as_mut_slice().is_err());

    // Change back to READ_WRITE
    region.protect(ProtFlags::READ_WRITE).unwrap();
    assert_eq!(region.prot(), ProtFlags::READ_WRITE);
    assert!(region.as_mut_slice().is_ok());
}

#[test]
fn test_memory_out_of_bounds_handling() {
    let mut region = MemoryRegion::allocate_16k(PAGE_SIZE_16K, ProtFlags::READ_WRITE).unwrap();
    let len = region.len();

    let res = region.write_at(len, &[1]);
    assert!(matches!(res, Err(MemoryError::OutOfBounds { .. })));

    let res_read = region.read_at(len - 2, 5);
    assert!(matches!(res_read, Err(MemoryError::OutOfBounds { .. })));
}

#[test]
fn test_virtual_memory_manager_multi_alloc() {
    let mut vmm = VirtualMemoryManager::new();
    let p1 = vmm.allocate(0x1000, ProtFlags::READ_WRITE).unwrap();
    let p2 = vmm.allocate(0x8000, ProtFlags::READ_EXEC).unwrap();
    let p3 = vmm.allocate(0x2000, ProtFlags::READ).unwrap();

    assert!(is_16k_aligned(p1 as usize));
    assert!(is_16k_aligned(p2 as usize));
    assert!(is_16k_aligned(p3 as usize));
    assert_eq!(vmm.region_count(), 3);
    assert!(vmm.total_mapped_bytes() >= 0x4000 + 0x8000 + 0x4000);
}
