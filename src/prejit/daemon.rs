//! Install-Time Headless Pre-JIT Background Worker Daemon (`nice 19`).
//!
//! Executes background pre-compilation during APK installation, generating the startup
//! `.larcache` file so the app launches instantly with zero runtime JIT stutters.

use crate::arch::{Arm64CpuContext, HostArch};
use crate::jit::backend_riscv::RiscvBackend;
use crate::jit::backend_x86::X86Backend;
use crate::jit::cache::{
    CompiledBlock, MmapExecutionCache, hash_arm64_block_with_context_and_base,
};
use crate::jit::decoder::Arm64Decoder;
use crate::jit::ir::IrBlock;
use crate::linker::LoadedLibrary;
use crate::prejit::callgraph::CallgraphAnalyzer;
use std::path::Path;
use std::time::Instant;

/// Background Pre-JIT Worker Daemon.
pub struct PreJitDaemon {
    pub target_arch: HostArch,
}

impl Default for PreJitDaemon {
    fn default() -> Self {
        Self::new()
    }
}

impl PreJitDaemon {
    pub fn new() -> Self {
        Self {
            target_arch: HostArch::current(),
        }
    }

    /// Sets the thread / process priority to lowest priority (`nice 19`) for background install work.
    pub fn apply_install_priority() {
        #[cfg(unix)]
        unsafe {
            libc::setpriority(libc::PRIO_PROCESS, 0, 19);
        }
    }

    /// Pre-compiles startup path for a loaded library and saves `.larcache` to disk.
    pub fn precompile_library<P: AsRef<Path>>(
        &self,
        lib: &LoadedLibrary,
        output_cache_path: P,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let start = Instant::now();
        Self::apply_install_priority();

        // 1. Build startup callgraph
        let call_nodes = CallgraphAnalyzer::build_startup_graph(lib);
        let mut compiled_blocks = Vec::with_capacity(call_nodes.len());

        // 2. Compile each call node to target machine code
        for node in &call_nodes {
            if node.opcodes.is_empty() {
                continue;
            }

            let mut hash_context = Arm64CpuContext::new();
            hash_context.pc = node.address as u64;
            let block_hash = hash_arm64_block_with_context_and_base(
                lib.load_base as u64,
                &hash_context,
                &node.opcodes,
            );

            let mut ir_block = IrBlock::new(node.address as u64);
            let mut supported = true;
            for &raw in &node.opcodes {
                let inst = Arm64Decoder::decode(raw);
                if ir_block.translate_arm64_inst_checked(&inst).is_err() {
                    supported = false;
                    break;
                }
            }
            if !supported {
                continue;
            }

            let machine_code = match self.target_arch {
                HostArch::X86_64 => {
                    X86Backend::compile_context_block(&ir_block).unwrap_or_default()
                }
                HostArch::Riscv64 => RiscvBackend::compile_to_bytes(&ir_block),
                _ => Vec::new(),
            };

            if !machine_code.is_empty() {
                compiled_blocks.push(CompiledBlock {
                    block_hash,
                    guest_pc: node.address as u64,
                    machine_code,
                });
            }
        }

        let num_compiled = compiled_blocks.len();

        // 3. Persist to MMAP execution cache on disk
        if !compiled_blocks.is_empty() {
            MmapExecutionCache::create_and_save(
                output_cache_path,
                self.target_arch,
                &compiled_blocks,
            )?;
        }

        let elapsed = start.elapsed();
        println!(
            "[Pre-JIT Daemon] Pre-compiled {} startup blocks for '{}' in {:.2} ms",
            num_compiled,
            lib.name,
            elapsed.as_secs_f64() * 1000.0
        );

        Ok(num_compiled)
    }

    pub fn precompile_libraries<P: AsRef<Path>>(
        &self,
        libraries: &[&LoadedLibrary],
        output_cache_path: P,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut blocks = Vec::new();
        for lib in libraries {
            let nodes = CallgraphAnalyzer::build_startup_graph(lib);
            for node in nodes {
                if node.opcodes.is_empty() {
                    continue;
                }
                let mut ir_block = IrBlock::new(node.address as u64);
                let mut supported = true;
                for raw in node.opcodes.iter().copied() {
                    if ir_block
                        .translate_arm64_inst_checked(&Arm64Decoder::decode(raw))
                        .is_err()
                    {
                        supported = false;
                        break;
                    }
                }
                if !supported {
                    continue;
                }
                let machine_code = match self.target_arch {
                    HostArch::X86_64 => {
                        X86Backend::compile_context_block(&ir_block).unwrap_or_default()
                    }
                    HostArch::Riscv64 => RiscvBackend::compile_to_bytes(&ir_block),
                    _ => Vec::new(),
                };
                if !machine_code.is_empty() {
                    blocks.push(CompiledBlock {
                        block_hash: {
                            let mut hash_context = Arm64CpuContext::new();
                            hash_context.pc = node.address as u64;
                            hash_arm64_block_with_context_and_base(
                                lib.load_base as u64,
                                &hash_context,
                                &node.opcodes,
                            )
                        },
                        guest_pc: node.address as u64,
                        machine_code,
                    });
                }
            }
        }
        let count = blocks.len();
        if count > 0 {
            MmapExecutionCache::create_and_save(output_cache_path, self.target_arch, &blocks)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prejit_daemon_init() {
        let daemon = PreJitDaemon::new();
        assert_eq!(daemon.target_arch, HostArch::current());
    }
}
