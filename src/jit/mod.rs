//! JIT Translation Engine for LAR.
//!
//! Coordinates Tier-0 fast interpretation, Tier-1 multi-target native JIT compilation
//! (x86_64 and RISC-V), AArch64 passthrough, and execution caching.

pub mod backend_riscv;
pub mod backend_x86;
pub mod cache;
pub mod decoder;
pub mod ir;
pub mod tier0;

pub use backend_riscv::RiscvBackend;
pub use backend_x86::X86Backend;
pub use cache::{
    CacheError, CompiledBlock, MmapExecutionCache, hash_arm64_block, hash_arm64_block_with_context,
};
pub use decoder::{Arm64Decoder, Arm64Inst, Arm64Op, ConditionCode};
pub use ir::{IrBlock, IrInstruction, IrOpcode, IrOperand, IrReg, UnsupportedInstruction};
pub use tier0::Tier0FastJit;

use crate::arch::{Arm64CpuContext, ExecutionMode, HostArch};
use crate::jit::backend_x86::X86ContextFn;
use crate::jit::cache::hash_arm64_block_with_context_and_base;
use crate::memory::mmap::MemoryRegion;
use std::collections::HashMap;

/// High-Level JIT Coordinator.
pub struct JitEngine {
    pub host_arch: HostArch,
    pub execution_mode: ExecutionMode,
    pub cache: Option<MmapExecutionCache>,
    pub live_blocks: HashMap<u64, MemoryRegion>,
    cache_load_base: u64,
}

impl Default for JitEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl JitEngine {
    pub fn new() -> Self {
        let host_arch = HostArch::current();
        let execution_mode = host_arch.execution_mode();

        Self {
            host_arch,
            execution_mode,
            cache: None,
            live_blocks: HashMap::new(),
            cache_load_base: 0,
        }
    }

    /// Attaches an on-disk MMAP Execution Cache to this JIT engine.
    pub fn set_cache(&mut self, cache: MmapExecutionCache) {
        self.cache = Some(cache);
    }

    pub fn load_cache<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), CacheError> {
        self.load_cache_for_base(path, 0)
    }

    pub fn load_cache_for_base<P: AsRef<std::path::Path>>(
        &mut self,
        path: P,
        load_base: u64,
    ) -> Result<(), CacheError> {
        let cache = MmapExecutionCache::load_from_file(path, self.host_arch)?;
        self.cache_load_base = load_base;
        self.set_cache(cache);
        Ok(())
    }

    pub fn has_cached_block(&self, ctx: &Arm64CpuContext, opcodes: &[u32]) -> bool {
        let context_hash =
            hash_arm64_block_with_context_and_base(self.cache_load_base, ctx, opcodes);
        self.cache
            .as_ref()
            .is_some_and(|cache| cache.lookup_block(context_hash).is_some())
    }

    pub fn has_cached_block_for_library(
        &self,
        load_base: u64,
        ctx: &Arm64CpuContext,
        opcodes: &[u32],
    ) -> bool {
        let context_hash = hash_arm64_block_with_context_and_base(load_base, ctx, opcodes);
        self.cache
            .as_ref()
            .is_some_and(|cache| cache.lookup_block(context_hash).is_some())
    }

    /// Executes a continuous sequence of ARM64 opcodes on the given CPU context.
    pub fn execute(&mut self, ctx: &mut Arm64CpuContext, opcodes: &[u32]) {
        if self.execution_mode == ExecutionMode::Direct {
            // Native ARM64 execution
            Tier0FastJit::execute_block(ctx, opcodes, opcodes.len());
            return;
        }

        let context_hash =
            hash_arm64_block_with_context_and_base(self.cache_load_base, ctx, opcodes);

        #[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
        if let Some(cache) = &self.cache
            && let Some(ptr) = cache.lookup_block(context_hash)
        {
            #[cfg(target_arch = "x86_64")]
            {
                let func: X86ContextFn = unsafe { std::mem::transmute(ptr) };
                unsafe { func(ctx as *mut Arm64CpuContext) };
            }
            #[cfg(target_arch = "riscv64")]
            {
                let func: unsafe extern "C" fn(*mut Arm64CpuContext) =
                    unsafe { std::mem::transmute(ptr) };
                unsafe { func(ctx as *mut Arm64CpuContext) };
            }
            return;
        }

        #[cfg(target_arch = "x86_64")]
        {
            let block_hash = hash_arm64_block(ctx.pc, opcodes);
            if let Some(region) = self.live_blocks.remove(&block_hash) {
                let func: X86ContextFn = unsafe { std::mem::transmute(region.as_ptr()) };
                unsafe { func(ctx as *mut Arm64CpuContext) };
                self.live_blocks.insert(block_hash, region);
                return;
            }

            let mut ir_block = IrBlock::new(ctx.pc);
            for &raw in opcodes {
                ir_block.translate_arm64_inst(&Arm64Decoder::decode(raw));
            }
            if let Ok(region) = X86Backend::emit_context_executable(&ir_block) {
                let func: X86ContextFn = unsafe { std::mem::transmute(region.as_ptr()) };
                unsafe { func(ctx as *mut Arm64CpuContext) };
                self.live_blocks.insert(block_hash, region);
                return;
            }
        }
        Tier0FastJit::execute_block(ctx, opcodes, opcodes.len());
    }
}
