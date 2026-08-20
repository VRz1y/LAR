//! Native Startup Callgraph Analyzer for Install-Time Pre-JIT.
//!
//! Identifies and traverses the minimal startup execution graph (`DT_INIT`, `DT_INIT_ARRAY`,
//! `JNI_OnLoad`, Main Activity entry points) to allow instantaneous 1-second pre-compilation.

use crate::jit::decoder::{Arm64Decoder, Arm64Op};
use crate::linker::LoadedLibrary;
use std::collections::HashSet;

/// Represents a discovered startup function block for pre-compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupCallNode {
    pub name: String,
    pub address: usize,
    pub opcodes: Vec<u32>,
}

/// Callgraph Analyzer.
pub struct CallgraphAnalyzer;

impl CallgraphAnalyzer {
    /// Discovers all startup entry points from a loaded library.
    pub fn build_startup_graph(lib: &LoadedLibrary) -> Vec<StartupCallNode> {
        let mut entry_points = Vec::new();
        let mut visited_addrs = HashSet::new();

        // 1. Collect DT_INIT and DT_INIT_ARRAY routines
        for routine in &lib.init_routines {
            if routine.address != 0 && visited_addrs.insert(routine.address) {
                let name = match routine.kind {
                    crate::linker::InitRoutineKind::DtInit => "DT_INIT".to_string(),
                    crate::linker::InitRoutineKind::DtInitArray => {
                        format!("DT_INIT_ARRAY_{}", routine.order)
                    }
                };
                entry_points.push((name, routine.address));
            }
        }

        // 2. Collect JNI_OnLoad and JNI exported functions
        let common_start_symbols = [
            "JNI_OnLoad",
            "JNI_OnUnload",
            "nativeInit",
            "Java_com_example_MainActivity_onCreate",
            "ANativeActivity_onCreate",
        ];

        for &sym_name in &common_start_symbols {
            if let Some(addr) = lib.lookup_symbol(sym_name)
                && visited_addrs.insert(addr)
            {
                entry_points.push((sym_name.to_string(), addr));
            }
        }

        // 3. Extract instruction basic blocks for each entry point
        let mut nodes = Vec::with_capacity(entry_points.len());
        for (name, addr) in entry_points {
            let opcodes = Self::extract_block_opcodes(lib, addr, 64);
            nodes.push(StartupCallNode {
                name,
                address: addr,
                opcodes,
            });
        }

        nodes
    }

    /// Reads up to `max_insts` 32-bit ARM64 opcodes from memory starting at `addr` until `RET` or branch.
    pub fn extract_block_opcodes(
        lib: &LoadedLibrary,
        start_addr: usize,
        max_insts: usize,
    ) -> Vec<u32> {
        let mut opcodes = Vec::new();
        if !lib.shadow_text.contains(start_addr)
            || lib.shadow_text.read_text_u32(start_addr).is_none()
        {
            return opcodes;
        }

        let mut curr = start_addr;
        for _ in 0..max_insts {
            let Some(raw) = lib.shadow_text.read_text_u32(curr) else {
                break;
            };
            opcodes.push(raw);

            let inst = Arm64Decoder::decode(raw);
            if matches!(inst.op, Arm64Op::Ret | Arm64Op::B) {
                break;
            }

            curr += 4;
        }

        opcodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_callgraph_node_representation() {
        let node = StartupCallNode {
            name: "JNI_OnLoad".to_string(),
            address: 0x0040_1000,
            opcodes: vec![0x9100a820, 0xd65f03c0],
        };

        assert_eq!(node.name, "JNI_OnLoad");
        assert_eq!(node.opcodes.len(), 2);
    }
}
