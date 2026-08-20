//! Synthetic ARM64 Shared Library and APK Generator for Testing.
//!
//! Generates valid ELF64 AArch64 `.so` binaries and packages them into
//! valid `.apk` ZIP archives in memory.

use lar::linker::elf::*;

/// Generates a valid 64-bit ARM64 shared library ELF binary in memory with exported symbols.
pub fn generate_synthetic_arm64_so(lib_name: &str, export_symbol: &str) -> Vec<u8> {
    let mut elf = vec![0u8; 0x2000];
    elf[0x800..0x804].copy_from_slice(&0xd503201fu32.to_le_bytes());
    elf[0x804..0x808].copy_from_slice(&0xd65f03c0u32.to_le_bytes());

    // 1. ELF Header (64 bytes)
    elf[0..4].copy_from_slice(&ELFMAG);
    elf[4] = ELFCLASS64;
    elf[5] = ELFDATA2LSB;
    elf[6] = EV_CURRENT;

    elf[16..18].copy_from_slice(&ET_DYN.to_le_bytes());
    elf[18..20].copy_from_slice(&EM_AARCH64.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&0x1000u64.to_le_bytes());
    elf[32..40].copy_from_slice(&64u64.to_le_bytes()); // phoff
    elf[52..54].copy_from_slice(&64u16.to_le_bytes()); // ehsize
    elf[54..56].copy_from_slice(&56u16.to_le_bytes()); // phentsize
    elf[56..58].copy_from_slice(&3u16.to_le_bytes()); // phnum

    // 2. Program Headers (3 * 56 = 168 bytes at offset 64..232)
    // Phdr 0: PT_LOAD Text (RX)
    let mut off = 64;
    elf[off..off + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    elf[off + 4..off + 8].copy_from_slice(&(PF_R | PF_X).to_le_bytes());
    elf[off + 8..off + 16].copy_from_slice(&0u64.to_le_bytes());
    elf[off + 16..off + 24].copy_from_slice(&0u64.to_le_bytes());
    elf[off + 24..off + 32].copy_from_slice(&0u64.to_le_bytes());
    elf[off + 32..off + 40].copy_from_slice(&0x1000u64.to_le_bytes());
    elf[off + 40..off + 48].copy_from_slice(&0x1000u64.to_le_bytes());
    elf[off + 48..off + 56].copy_from_slice(&0x4000u64.to_le_bytes()); // 16KB align

    // Phdr 1: PT_LOAD Data (RW)
    off += 56;
    elf[off..off + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    elf[off + 4..off + 8].copy_from_slice(&(PF_R | PF_W).to_le_bytes());
    elf[off + 8..off + 16].copy_from_slice(&0x1000u64.to_le_bytes());
    elf[off + 16..off + 24].copy_from_slice(&0x4000u64.to_le_bytes());
    elf[off + 24..off + 32].copy_from_slice(&0x4000u64.to_le_bytes());
    elf[off + 32..off + 40].copy_from_slice(&0x1000u64.to_le_bytes());
    elf[off + 40..off + 48].copy_from_slice(&0x2000u64.to_le_bytes());
    elf[off + 48..off + 56].copy_from_slice(&0x4000u64.to_le_bytes());

    // Phdr 2: PT_DYNAMIC
    off += 56;
    elf[off..off + 4].copy_from_slice(&PT_DYNAMIC.to_le_bytes());
    elf[off + 4..off + 8].copy_from_slice(&(PF_R | PF_W).to_le_bytes());
    elf[off + 8..off + 16].copy_from_slice(&0x1000u64.to_le_bytes());
    elf[off + 16..off + 24].copy_from_slice(&0x4000u64.to_le_bytes());
    elf[off + 24..off + 32].copy_from_slice(&0x4000u64.to_le_bytes());
    elf[off + 32..off + 40].copy_from_slice(&0x200u64.to_le_bytes());
    elf[off + 40..off + 48].copy_from_slice(&0x200u64.to_le_bytes());
    elf[off + 48..off + 56].copy_from_slice(&8u64.to_le_bytes());

    // 3. String Table, Symbol Table, Relocations
    let mut strtab = vec![0u8]; // leading null
    let sym_name_off = strtab.len();
    strtab.extend_from_slice(export_symbol.as_bytes());
    strtab.push(0);

    let malloc_name_off = strtab.len();
    strtab.extend_from_slice(b"malloc\0");

    let soname_off = strtab.len();
    strtab.extend_from_slice(lib_name.as_bytes());
    strtab.push(0);

    let strtab_vaddr = 0x4200u64;
    let strtab_sz = strtab.len() as u64;

    // Symbol Table:
    let symtab_vaddr = 0x4300u64;
    let mut symtab_bytes = Vec::new();
    // Sym 0: UNDEF
    symtab_bytes.extend_from_slice(&[0u8; 24]);

    // Sym 1: Custom Exported Symbol
    symtab_bytes.extend_from_slice(&(sym_name_off as u32).to_le_bytes());
    symtab_bytes.push((STB_GLOBAL << 4) | STT_FUNC);
    symtab_bytes.push(0);
    symtab_bytes.extend_from_slice(&1u16.to_le_bytes()); // section 1
    symtab_bytes.extend_from_slice(&0x0800u64.to_le_bytes()); // func address
    symtab_bytes.extend_from_slice(&64u64.to_le_bytes()); // size

    // Sym 2: malloc (Import from Bionic)
    symtab_bytes.extend_from_slice(&(malloc_name_off as u32).to_le_bytes());
    symtab_bytes.push((STB_GLOBAL << 4) | STT_FUNC);
    symtab_bytes.push(0);
    symtab_bytes.extend_from_slice(&0u16.to_le_bytes()); // UNDEF
    symtab_bytes.extend_from_slice(&0u64.to_le_bytes());
    symtab_bytes.extend_from_slice(&0u64.to_le_bytes());

    // Relocations:
    let rela_vaddr = 0x4400u64;
    let mut rela_bytes = Vec::new();

    // Rela 0: Relative relocation
    rela_bytes.extend_from_slice(&0x4500u64.to_le_bytes());
    let r_info_rel = R_AARCH64_RELATIVE as u64;
    rela_bytes.extend_from_slice(&r_info_rel.to_le_bytes());
    rela_bytes.extend_from_slice(&0x0800i64.to_le_bytes());

    // Rela 1: malloc resolution
    rela_bytes.extend_from_slice(&0x4508u64.to_le_bytes());
    let r_info_glob = ((2u64) << 32) | (R_AARCH64_GLOB_DAT as u64);
    rela_bytes.extend_from_slice(&r_info_glob.to_le_bytes());
    rela_bytes.extend_from_slice(&0i64.to_le_bytes());

    let rela_sz = rela_bytes.len() as u64;

    // Dynamic section:
    let mut dyn_bytes = Vec::new();
    let mut add_dyn = |tag: u64, val: u64| {
        dyn_bytes.extend_from_slice(&tag.to_le_bytes());
        dyn_bytes.extend_from_slice(&val.to_le_bytes());
    };

    add_dyn(DT_SONAME, soname_off as u64);
    add_dyn(DT_STRTAB, strtab_vaddr);
    add_dyn(DT_STRSZ, strtab_sz);
    add_dyn(DT_SYMTAB, symtab_vaddr);
    add_dyn(DT_SYMENT, 24);
    add_dyn(DT_RELA, rela_vaddr);
    add_dyn(DT_RELASZ, rela_sz);
    add_dyn(DT_RELAENT, 24);
    add_dyn(DT_INIT, 0x0800);
    add_dyn(DT_NULL, 0);

    // Copy to memory buffer
    let data_base_offset = 0x1000;
    elf[data_base_offset..data_base_offset + dyn_bytes.len()].copy_from_slice(&dyn_bytes);
    let str_offset = data_base_offset + 0x200;
    elf[str_offset..str_offset + strtab.len()].copy_from_slice(&strtab);
    let sym_offset = data_base_offset + 0x300;
    elf[sym_offset..sym_offset + symtab_bytes.len()].copy_from_slice(&symtab_bytes);
    let rela_offset = data_base_offset + 0x400;
    elf[rela_offset..rela_offset + rela_bytes.len()].copy_from_slice(&rela_bytes);

    elf
}

/// Packages a list of (filename_in_apk, data) into a valid ZIP / APK binary buffer.
pub fn generate_synthetic_apk(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut apk = Vec::new();
    let mut central_dir = Vec::new();
    let mut offsets = Vec::new();

    // 1. Write Local File Headers + File Data
    for (filename, data) in entries {
        let local_header_offset = apk.len() as u32;
        offsets.push(local_header_offset);

        let fname_bytes = filename.as_bytes();
        let fname_len = fname_bytes.len() as u16;
        let data_len = data.len() as u32;

        // Local header (30 bytes)
        apk.extend_from_slice(&0x0403_4b50u32.to_le_bytes()); // magic PK\x03\x04
        apk.extend_from_slice(&20u16.to_le_bytes()); // version needed (2.0)
        apk.extend_from_slice(&0u16.to_le_bytes()); // flags
        apk.extend_from_slice(&0u16.to_le_bytes()); // compression (0 = Stored)
        apk.extend_from_slice(&0u16.to_le_bytes()); // mod time
        apk.extend_from_slice(&0u16.to_le_bytes()); // mod date
        apk.extend_from_slice(&0u32.to_le_bytes()); // crc32
        apk.extend_from_slice(&data_len.to_le_bytes()); // compressed size
        apk.extend_from_slice(&data_len.to_le_bytes()); // uncompressed size
        apk.extend_from_slice(&fname_len.to_le_bytes());
        apk.extend_from_slice(&0u16.to_le_bytes()); // extra len

        apk.extend_from_slice(fname_bytes);
        apk.extend_from_slice(data);
    }

    let central_dir_start = apk.len() as u32;

    // 2. Write Central Directory Headers (46 bytes per entry)
    for (i, (filename, data)) in entries.iter().enumerate() {
        let fname_bytes = filename.as_bytes();
        let fname_len = fname_bytes.len() as u16;
        let data_len = data.len() as u32;
        let local_offset = offsets[i];

        central_dir.extend_from_slice(&0x0201_4b50u32.to_le_bytes()); // magic PK\x01\x02
        central_dir.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central_dir.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // flags
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // compression (0 = Stored)
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // mod time
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // mod date
        central_dir.extend_from_slice(&0u32.to_le_bytes()); // crc32
        central_dir.extend_from_slice(&data_len.to_le_bytes());
        central_dir.extend_from_slice(&data_len.to_le_bytes());
        central_dir.extend_from_slice(&fname_len.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // extra len
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // comment len
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // disk nr
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // internal attr
        central_dir.extend_from_slice(&0u32.to_le_bytes()); // external attr
        central_dir.extend_from_slice(&local_offset.to_le_bytes());

        central_dir.extend_from_slice(fname_bytes);
    }

    let central_dir_len = central_dir.len() as u32;
    apk.extend_from_slice(&central_dir);

    // 3. Write End of Central Directory Record (22 bytes)
    let num_entries = entries.len() as u16;
    apk.extend_from_slice(&0x0605_4b50u32.to_le_bytes()); // magic PK\x05\x06
    apk.extend_from_slice(&0u16.to_le_bytes()); // disk nr
    apk.extend_from_slice(&0u16.to_le_bytes()); // start disk
    apk.extend_from_slice(&num_entries.to_le_bytes()); // entries on this disk
    apk.extend_from_slice(&num_entries.to_le_bytes()); // total entries
    apk.extend_from_slice(&central_dir_len.to_le_bytes());
    apk.extend_from_slice(&central_dir_start.to_le_bytes());
    apk.extend_from_slice(&0u16.to_le_bytes()); // comment len

    apk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_synthetic_so_and_apk() {
        let so_data = generate_synthetic_arm64_so("libgame.so", "Java_com_example_game_nativeInit");
        assert!(!so_data.is_empty());
        assert_eq!(&so_data[0..4], &ELFMAG);

        let apk_data = generate_synthetic_apk(&[("lib/arm64-v8a/libgame.so", &so_data)]);
        assert!(apk_data.len() > so_data.len());
    }
}
