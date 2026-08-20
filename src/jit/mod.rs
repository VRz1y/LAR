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

/// Error returned when an IR hook cannot transform a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrHookError(pub String);

impl std::fmt::Display for IrHookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IrHookError {}

/// Transforms a translated IR block before it is handed to a native backend.
pub trait IrHook {
    fn apply(&self, block: &mut IrBlock) -> Result<(), IrHookError>;
}

/// High-Level JIT Coordinator.
pub struct JitEngine {
    pub host_arch: HostArch,
    pub execution_mode: ExecutionMode,
    pub cache: Option<MmapExecutionCache>,
    pub live_blocks: HashMap<u64, MemoryRegion>,
    hooks: Vec<Box<dyn IrHook + Send + Sync>>,
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
            hooks: Vec::new(),
            cache_load_base: 0,
        }
    }

    /// Attaches an on-disk MMAP Execution Cache to this JIT engine.
    pub fn set_cache(&mut self, cache: MmapExecutionCache) {
        self.cache = Some(cache);
    }

    pub fn add_ir_hook<H>(&mut self, hook: H)
    where
        H: IrHook + Send + Sync + 'static,
    {
        self.hooks.push(Box::new(hook));
    }

    pub fn clear_ir_hooks(&mut self) {
        self.hooks.clear();
    }

    pub fn has_ir_hooks(&self) -> bool {
        !self.hooks.is_empty()
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
        if self.has_ir_hooks() {
            return false;
        }
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
        if self.has_ir_hooks() {
            return false;
        }
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
        if self.hooks.is_empty()
            && let Some(cache) = &self.cache
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
            if self.hooks.is_empty()
                && let Some(region) = self.live_blocks.remove(&block_hash)
            {
                let func: X86ContextFn = unsafe { std::mem::transmute(region.as_ptr()) };
                unsafe { func(ctx as *mut Arm64CpuContext) };
                self.live_blocks.insert(block_hash, region);
                return;
            }

            let mut ir_block = IrBlock::new(ctx.pc);
            for &raw in opcodes {
                ir_block.translate_arm64_inst(&Arm64Decoder::decode(raw));
            }
            if self
                .hooks
                .iter()
                .any(|hook| hook.apply(&mut ir_block).is_err())
            {
                Tier0FastJit::execute_block(ctx, opcodes, opcodes.len());
                return;
            }
            if let Ok(region) = X86Backend::emit_context_executable(&ir_block) {
                let func: X86ContextFn = unsafe { std::mem::transmute(region.as_ptr()) };
                unsafe { func(ctx as *mut Arm64CpuContext) };
                if self.hooks.is_empty() {
                    self.live_blocks.insert(block_hash, region);
                }
                return;
            }
        }
        Tier0FastJit::execute_block(ctx, opcodes, opcodes.len());
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct ReplaceMovImmediate(i64);

    impl IrHook for ReplaceMovImmediate {
        fn apply(&self, block: &mut IrBlock) -> Result<(), IrHookError> {
            let mov = block
                .instructions
                .iter_mut()
                .find(|inst| inst.opcode == IrOpcode::Mov)
                .ok_or_else(|| IrHookError("missing MOV".into()))?;
            mov.src1 = IrOperand::Imm(self.0);
            Ok(())
        }
    }

    struct FailingHook(Arc<AtomicUsize>);

    impl IrHook for FailingHook {
        fn apply(&self, _block: &mut IrBlock) -> Result<(), IrHookError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Err(IrHookError("hook failed".into()))
        }
    }

    #[test]
    fn ir_hook_runs_before_x86_backend_and_bypasses_live_cache() {
        let opcodes = [0xd2800140]; // mov x0, #10
        let mut engine = JitEngine::new();
        let mut original = Arm64CpuContext::new();
        original.pc = 0x1000;
        engine.execute(&mut original, &opcodes);
        assert_eq!(original.regs[0], 10);
        assert_eq!(engine.live_blocks.len(), 1);

        engine.add_ir_hook(ReplaceMovImmediate(77));
        let mut hooked = Arm64CpuContext::new();
        hooked.pc = 0x1000;
        engine.execute(&mut hooked, &opcodes);

        assert_eq!(hooked.regs[0], 77);
        assert_eq!(engine.live_blocks.len(), 1);
    }

    #[test]
    fn ir_hook_bypasses_disk_cache() {
        let opcodes = [0xd2800140]; // mov x0, #10
        let mut ctx = Arm64CpuContext::new();
        ctx.pc = 0x2000;
        let hash = hash_arm64_block_with_context(&ctx, &opcodes);

        let mut cached_ir = IrBlock::new(ctx.pc);
        cached_ir.push(IrInstruction::new(
            IrOpcode::Mov,
            Some(IrReg::X(0)),
            IrOperand::Imm(99),
            None,
        ));
        let cache_path =
            std::env::temp_dir().join(format!("lar_ir_hook_cache_{}.larcache", std::process::id()));
        let cached = CompiledBlock {
            block_hash: hash,
            guest_pc: ctx.pc,
            machine_code: X86Backend::compile_context_block(&cached_ir).unwrap(),
        };
        MmapExecutionCache::create_and_save(&cache_path, HostArch::X86_64, &[cached]).unwrap();

        let mut engine = JitEngine::new();
        engine.load_cache(&cache_path).unwrap();
        assert!(engine.has_cached_block(&ctx, &opcodes));
        engine.add_ir_hook(ReplaceMovImmediate(77));
        assert!(!engine.has_cached_block(&ctx, &opcodes));
        engine.execute(&mut ctx, &opcodes);

        assert_eq!(ctx.regs[0], 77);
        let _ = std::fs::remove_file(cache_path);
    }

    #[test]
    fn ir_hook_error_falls_back_to_tier0() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut engine = JitEngine::new();
        engine.add_ir_hook(FailingHook(Arc::clone(&calls)));
        let mut ctx = Arm64CpuContext::new();

        engine.execute(&mut ctx, &[0xd2800140]);

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(ctx.regs[0], 10);
        assert!(engine.live_blocks.is_empty());
    }
}
