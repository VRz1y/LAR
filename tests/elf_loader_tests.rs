//! Synthetic ARM64 Shared Library Generator and Linker Integration Tests.

use lar::linker::*;
use lar::memory::is_16k_aligned;

/// Helper function to build a valid synthetic 64-bit ARM64 ELF shared library in memory.
fn build_synthetic_arm64_so() -> Vec<u8> {
    let mut elf = vec![0u8; 0x2000];

    // 1. ELF Header (64 bytes)
    // 0x00: Ident
    elf[0..4].copy_from_slice(&elf::ELFMAG);
    elf[4] = elf::ELFCLASS64;
    elf[5] = elf::ELFDATA2LSB;
    elf[6] = elf::EV_CURRENT;

    // 0x10: e_type (ET_DYN = 3)
    elf[16..18].copy_from_slice(&elf::ET_DYN.to_le_bytes());
    // 0x12: e_machine (EM_AARCH64 = 183)
    elf[18..20].copy_from_slice(&elf::EM_AARCH64.to_le_bytes());
    // 0x14: e_version (1)
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    // 0x18: e_entry (0x1000)
    elf[24..32].copy_from_slice(&0x1000u64.to_le_bytes());
    // 0x20: e_phoff (64)
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    // 0x28: e_shoff (0)
    elf[40..48].copy_from_slice(&0u64.to_le_bytes());
    // 0x30: e_flags (0)
    elf[48..52].copy_from_slice(&0u32.to_le_bytes());
    // 0x34: e_ehsize (64)
    elf[52..54].copy_from_slice(&64u16.to_le_bytes());
    // 0x36: e_phentsize (56)
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    // 0x38: e_phnum (3: PT_LOAD text, PT_LOAD data, PT_DYNAMIC)
    elf[56..58].copy_from_slice(&3u16.to_le_bytes());

    // 2. Program Headers (3 * 56 = 168 bytes at offset 64..232)

    // Phdr 0: PT_LOAD (Text Segment - RX)
    // vaddr: 0x0000, offset: 0, filesz: 0x1000, memsz: 0x1000, flags: PF_R | PF_X
    let mut off = 64;
    elf[off..off + 4].copy_from_slice(&elf::PT_LOAD.to_le_bytes());
    elf[off + 4..off + 8].copy_from_slice(&(elf::PF_R | elf::PF_X).to_le_bytes());
    elf[off + 8..off + 16].copy_from_slice(&0u64.to_le_bytes()); // p_offset
    elf[off + 16..off + 24].copy_from_slice(&0u64.to_le_bytes()); // p_vaddr
    elf[off + 24..off + 32].copy_from_slice(&0u64.to_le_bytes()); // p_paddr
    elf[off + 32..off + 40].copy_from_slice(&0x1000u64.to_le_bytes()); // p_filesz
    elf[off + 40..off + 48].copy_from_slice(&0x1000u64.to_le_bytes()); // p_memsz
    elf[off + 48..off + 56].copy_from_slice(&0x4000u64.to_le_bytes()); // p_align (16KB)

    // Phdr 1: PT_LOAD (Data / Dynamic Segment - RW)
    // vaddr: 0x4000, offset: 0x1000, filesz: 0x1000, memsz: 0x2000, flags: PF_R | PF_W
    off += 56;
    elf[off..off + 4].copy_from_slice(&elf::PT_LOAD.to_le_bytes());
    elf[off + 4..off + 8].copy_from_slice(&(elf::PF_R | elf::PF_W).to_le_bytes());
    elf[off + 8..off + 16].copy_from_slice(&0x1000u64.to_le_bytes()); // p_offset
    elf[off + 16..off + 24].copy_from_slice(&0x4000u64.to_le_bytes()); // p_vaddr
    elf[off + 24..off + 32].copy_from_slice(&0x4000u64.to_le_bytes()); // p_paddr
    elf[off + 32..off + 40].copy_from_slice(&0x1000u64.to_le_bytes()); // p_filesz
    elf[off + 40..off + 48].copy_from_slice(&0x2000u64.to_le_bytes()); // p_memsz
    elf[off + 48..off + 56].copy_from_slice(&0x4000u64.to_le_bytes()); // p_align (16KB)

    // Phdr 2: PT_DYNAMIC
    // vaddr: 0x4000, offset: 0x1000, filesz: 0x200, memsz: 0x200, flags: PF_R | PF_W
    off += 56;
    elf[off..off + 4].copy_from_slice(&elf::PT_DYNAMIC.to_le_bytes());
    elf[off + 4..off + 8].copy_from_slice(&(elf::PF_R | elf::PF_W).to_le_bytes());
    elf[off + 8..off + 16].copy_from_slice(&0x1000u64.to_le_bytes()); // p_offset
    elf[off + 16..off + 24].copy_from_slice(&0x4000u64.to_le_bytes()); // p_vaddr
    elf[off + 24..off + 32].copy_from_slice(&0x4000u64.to_le_bytes()); // p_paddr
    elf[off + 32..off + 40].copy_from_slice(&0x200u64.to_le_bytes()); // p_filesz
    elf[off + 40..off + 48].copy_from_slice(&0x200u64.to_le_bytes()); // p_memsz
    elf[off + 48..off + 56].copy_from_slice(&8u64.to_le_bytes()); // p_align

    // 3. Data Segment content at file offset 0x1000 (vaddr 0x4000)
    // Structure:
    // 0x1000: Dynamic table entries (DT_*)
    // 0x1200: String Table
    // 0x1300: Symbol Table
    // 0x1400: Relocation Table (RELA)

    // String Table: "\0native_calculate\0malloc\0__android_log_print\0"
    let strtab = b"\0native_calculate\0malloc\0__android_log_print\0";
    let strtab_vaddr = 0x4200u64;
    let strtab_sz = strtab.len() as u64;

    // Symbol Table:
    // Sym 0: STN_UNDEF
    // Sym 1: native_calculate (Global, Func, vaddr: 0x0500, size: 32)
    // Sym 2: malloc (Global Undefined - Import from Bionic)
    // Sym 3: __android_log_print (Global Undefined - Import from Bionic)
    let symtab_vaddr = 0x4300u64;
    let mut symtab_bytes = Vec::new();
    // Sym 0
    symtab_bytes.extend_from_slice(&[0u8; 24]);
    // Sym 1: native_calculate
    symtab_bytes.extend_from_slice(&1u32.to_le_bytes()); // st_name = offset 1
    symtab_bytes.push((elf::STB_GLOBAL << 4) | elf::STT_FUNC); // st_info
    symtab_bytes.push(0); // st_other
    symtab_bytes.extend_from_slice(&1u16.to_le_bytes()); // st_shndx
    symtab_bytes.extend_from_slice(&0x0500u64.to_le_bytes()); // st_value
    symtab_bytes.extend_from_slice(&32u64.to_le_bytes()); // st_size

    // Sym 2: malloc (st_name = 18)
    symtab_bytes.extend_from_slice(&18u32.to_le_bytes());
    symtab_bytes.push((elf::STB_GLOBAL << 4) | elf::STT_FUNC);
    symtab_bytes.push(0);
    symtab_bytes.extend_from_slice(&0u16.to_le_bytes()); // UNDEF
    symtab_bytes.extend_from_slice(&0u64.to_le_bytes());
    symtab_bytes.extend_from_slice(&0u64.to_le_bytes());

    // Sym 3: __android_log_print (st_name = 25)
    symtab_bytes.extend_from_slice(&25u32.to_le_bytes());
    symtab_bytes.push((elf::STB_GLOBAL << 4) | elf::STT_FUNC);
    symtab_bytes.push(0);
    symtab_bytes.extend_from_slice(&0u16.to_le_bytes()); // UNDEF
    symtab_bytes.extend_from_slice(&0u64.to_le_bytes());
    symtab_bytes.extend_from_slice(&0u64.to_le_bytes());

    // Relocation Table (RELA):
    // Rela 0: R_AARCH64_RELATIVE at vaddr 0x4500, addend: 0x0500
    // Rela 1: R_AARCH64_GLOB_DAT for Sym 2 (malloc) at vaddr 0x4508
    let rela_vaddr = 0x4400u64;
    let mut rela_bytes = Vec::new();

    // Rela 0: Relative
    rela_bytes.extend_from_slice(&0x4500u64.to_le_bytes()); // r_offset
    let r_info_0 = elf::R_AARCH64_RELATIVE as u64;
    rela_bytes.extend_from_slice(&r_info_0.to_le_bytes());
    rela_bytes.extend_from_slice(&0x0500i64.to_le_bytes()); // r_addend

    // Rela 1: Glob Dat (malloc -> sym 2)
    rela_bytes.extend_from_slice(&0x4508u64.to_le_bytes()); // r_offset
    let r_info_1 = ((2u64) << 32) | (elf::R_AARCH64_GLOB_DAT as u64);
    rela_bytes.extend_from_slice(&r_info_1.to_le_bytes());
    rela_bytes.extend_from_slice(&0i64.to_le_bytes()); // r_addend

    let rela_sz = rela_bytes.len() as u64;

    // Dynamic Entries:
    let mut dyn_bytes = Vec::new();
    let mut add_dyn = |tag: u64, val: u64| {
        dyn_bytes.extend_from_slice(&tag.to_le_bytes());
        dyn_bytes.extend_from_slice(&val.to_le_bytes());
    };

    add_dyn(elf::DT_STRTAB, strtab_vaddr);
    add_dyn(elf::DT_STRSZ, strtab_sz);
    add_dyn(elf::DT_SYMTAB, symtab_vaddr);
    add_dyn(elf::DT_SYMENT, 24);
    add_dyn(elf::DT_RELA, rela_vaddr);
    add_dyn(elf::DT_RELASZ, rela_sz);
    add_dyn(elf::DT_RELAENT, 24);
    add_dyn(elf::DT_INIT, 0x0500);
    add_dyn(elf::DT_NULL, 0);

    // Assemble Data segment into buffer
    let data_base_offset = 0x1000;
    // Copy Dynamic section
    elf[data_base_offset..data_base_offset + dyn_bytes.len()].copy_from_slice(&dyn_bytes);
    // Copy String table at offset 0x1200 (vaddr 0x4200)
    let strtab_offset = data_base_offset + 0x200;
    elf[strtab_offset..strtab_offset + strtab.len()].copy_from_slice(strtab);
    // Copy Symbol table at offset 0x1300 (vaddr 0x4300)
    let symtab_offset = data_base_offset + 0x300;
    elf[symtab_offset..symtab_offset + symtab_bytes.len()].copy_from_slice(&symtab_bytes);
    // Copy Relocation table at offset 0x1400 (vaddr 0x4400)
    let rela_offset = data_base_offset + 0x400;
    elf[rela_offset..rela_offset + rela_bytes.len()].copy_from_slice(&rela_bytes);

    elf
}

#[test]
fn test_load_and_link_synthetic_arm64_library() {
    let elf_data = build_synthetic_arm64_so();
    let mut registry = SymbolRegistry::new();
    lar::bionic::register_bionic_shims(&mut registry);

    let loaded = ElfLoader::load_from_memory("libsample.so", &elf_data, &mut registry)
        .expect("Failed to load synthetic ARM64 library");

    assert_eq!(loaded.name, "libsample.so");
    assert!(is_16k_aligned(loaded.load_base));

    // Verify exported symbol resolution
    let calc_sym = loaded.lookup_symbol("native_calculate");
    assert!(calc_sym.is_some());
    assert_eq!(calc_sym.unwrap(), loaded.load_base + 0x0500);

    // Verify DT_INIT was parsed
    assert_eq!(loaded.init_array.len(), 1);
    assert_eq!(loaded.init_array[0], loaded.load_base + 0x0500);

    // Verify Relocation patching in memory
    // 1. R_AARCH64_RELATIVE at load_base + 0x4500 should equal (load_base + 0x0500)
    let rel_slot_ptr = (loaded.load_base + 0x4500) as *const u64;
    let rel_slot_val = unsafe { *rel_slot_ptr };
    assert_eq!(rel_slot_val, (loaded.load_base + 0x0500) as u64);

    // 2. R_AARCH64_GLOB_DAT at load_base + 0x4508 should equal bionic malloc address
    let glob_slot_ptr = (loaded.load_base + 0x4508) as *const u64;
    let glob_slot_val = unsafe { *glob_slot_ptr };
    let bionic_malloc_addr = registry.resolve("malloc").unwrap();
    assert_eq!(glob_slot_val, bionic_malloc_addr as u64);
}
