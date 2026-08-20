use crate::memory::mmap::{MemoryError, MemoryRegion, ProtFlags};

const STP_X29_X30_PRE_INDEX_16: u32 = 0xa9bf7bfd;
const LDR_X16_LITERAL_20: u32 = 0x580000b0;
const BLR_X16: u32 = 0xd63f0200;
const LDP_X29_X30_POST_INDEX_16: u32 = 0xa8c17bfd;
const RET: u32 = 0xd65f03c0;

pub type Arm64ContextHandler = unsafe extern "C" fn(*mut crate::arch::Arm64CpuContext);

#[derive(Debug)]
pub struct Arm64ContextTrampoline {
    region: MemoryRegion,
    code_len: usize,
}

impl Arm64ContextTrampoline {
    pub fn emit(handler: Arm64ContextHandler) -> Result<Self, MemoryError> {
        Self::emit_address(handler as usize)
    }

    pub fn emit_address(handler: usize) -> Result<Self, MemoryError> {
        let code = Self::machine_code(handler);
        let mut region = MemoryRegion::allocate_16k(code.len(), ProtFlags::READ_WRITE)?;
        region.write_at(0, &code)?;
        region.protect(ProtFlags::READ_EXEC)?;
        flush_instruction_cache(region.as_ptr(), code.len());
        Ok(Self {
            region,
            code_len: code.len(),
        })
    }

    pub fn machine_code(handler: usize) -> Vec<u8> {
        let mut code = Vec::with_capacity(32);
        code.extend_from_slice(&STP_X29_X30_PRE_INDEX_16.to_le_bytes());
        code.extend_from_slice(&LDR_X16_LITERAL_20.to_le_bytes());
        code.extend_from_slice(&BLR_X16.to_le_bytes());
        code.extend_from_slice(&LDP_X29_X30_POST_INDEX_16.to_le_bytes());
        code.extend_from_slice(&RET.to_le_bytes());
        code.extend_from_slice(&0xd503201fu32.to_le_bytes());
        code.extend_from_slice(&(handler as u64).to_le_bytes());
        code
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.region.as_ptr()
    }

    pub fn code_len(&self) -> usize {
        self.code_len
    }

    pub fn protection(&self) -> ProtFlags {
        self.region.prot()
    }
}

#[cfg(target_arch = "aarch64")]
fn flush_instruction_cache(start: *const u8, len: usize) {
    use std::arch::asm;

    let ctr: usize;
    unsafe {
        asm!("mrs {ctr}, ctr_el0", ctr = out(reg) ctr, options(nomem, nostack, preserves_flags))
    };
    let dcache_line = 4usize << ((ctr >> 16) & 0xf);
    let icache_line = 4usize << (ctr & 0xf);
    let end = start as usize + len;
    let mut addr = (start as usize) & !(dcache_line - 1);
    while addr < end {
        unsafe { asm!("dc cvau, {addr}", addr = in(reg) addr, options(nostack, preserves_flags)) };
        addr += dcache_line;
    }
    unsafe { asm!("dsb ish", options(nostack, preserves_flags)) };
    addr = (start as usize) & !(icache_line - 1);
    while addr < end {
        unsafe { asm!("ic ivau, {addr}", addr = in(reg) addr, options(nostack, preserves_flags)) };
        addr += icache_line;
    }
    unsafe { asm!("dsb ish", "isb", options(nostack, preserves_flags)) };
}

#[cfg(not(target_arch = "aarch64"))]
fn flush_instruction_cache(_start: *const u8, _len: usize) {}
