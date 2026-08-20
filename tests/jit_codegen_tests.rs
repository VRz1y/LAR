//! Machine Code Generation and JIT Execution Tests for x86_64 and RISC-V.

use lar::jit::backend_riscv::RiscvBackend;
use lar::jit::backend_x86::X86Backend;
use lar::jit::ir::*;

#[cfg(target_arch = "x86_64")]
use lar::arch::Arm64CpuContext;
#[cfg(target_arch = "x86_64")]
use lar::jit::JitEngine;

#[test]
fn test_x86_codegen_multi_arithmetic() {
    let mut block = IrBlock::new(0x1000);
    // mov rdi, 100
    block.push(IrInstruction::new(
        IrOpcode::Mov,
        Some(IrReg::X(0)),
        IrOperand::Imm(100),
        None,
    ));
    // add rdi, rdi, 250
    block.push(IrInstruction::new(
        IrOpcode::Add,
        Some(IrReg::X(0)),
        IrOperand::Reg(IrReg::X(0)),
        Some(IrOperand::Imm(250)),
    ));
    // sub rdi, rdi, 50
    block.push(IrInstruction::new(
        IrOpcode::Sub,
        Some(IrReg::X(0)),
        IrOperand::Reg(IrReg::X(0)),
        Some(IrOperand::Imm(50)),
    ));
    // ret
    block.push(IrInstruction::new(
        IrOpcode::Return,
        None,
        IrOperand::Reg(IrReg::X(0)),
        None,
    ));

    let exec = X86Backend::emit_executable(&block).expect("Failed to emit executable x86_64 code");
    let func: extern "C" fn() -> u64 = unsafe { std::mem::transmute(exec.as_ptr()) };

    let result = func();
    assert_eq!(result, 300); // 100 + 250 - 50 = 300
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_x86_executable_branch_displacements_all_conditions() {
    use lar::jit::ConditionCode;

    let conditions = [
        ConditionCode::EQ,
        ConditionCode::NE,
        ConditionCode::CS,
        ConditionCode::CC,
        ConditionCode::MI,
        ConditionCode::PL,
        ConditionCode::VS,
        ConditionCode::VC,
        ConditionCode::HI,
        ConditionCode::LS,
        ConditionCode::GE,
        ConditionCode::LT,
        ConditionCode::GT,
        ConditionCode::LE,
        ConditionCode::AL,
        ConditionCode::NV,
    ];

    for cond in conditions {
        let mut block = IrBlock::new(0x1000);
        block.push(IrInstruction::new(
            IrOpcode::Mov,
            Some(IrReg::X(0)),
            IrOperand::Imm(42),
            None,
        ));
        block.push(IrInstruction::new(
            IrOpcode::Cmp,
            None,
            IrOperand::Reg(IrReg::X(0)),
            Some(IrOperand::Imm(0)),
        ));
        block.push(IrInstruction::new(
            IrOpcode::CondBranch(cond),
            None,
            IrOperand::Imm(8),
            None,
        ));
        block.push(IrInstruction::new(
            IrOpcode::Mov,
            Some(IrReg::X(0)),
            IrOperand::Imm(7),
            None,
        ));
        block.push(IrInstruction::new(
            IrOpcode::Return,
            None,
            IrOperand::Reg(IrReg::X(0)),
            None,
        ));

        let expected = match cond {
            ConditionCode::NE
            | ConditionCode::CS
            | ConditionCode::PL
            | ConditionCode::VC
            | ConditionCode::HI
            | ConditionCode::GE
            | ConditionCode::GT
            | ConditionCode::AL => 42,
            _ => 7,
        };
        let exec = X86Backend::emit_executable(&block).expect("failed to emit branch block");
        let func: extern "C" fn() -> u64 = unsafe { std::mem::transmute(exec.as_ptr()) };
        assert_eq!(func(), expected, "condition {:?}", cond);
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_x86_executable_unconditional_branch_displacement() {
    let mut block = IrBlock::new(0x1000);
    block.push(IrInstruction::new(
        IrOpcode::Branch,
        None,
        IrOperand::Imm(8),
        None,
    ));
    block.push(IrInstruction::new(
        IrOpcode::Mov,
        Some(IrReg::X(0)),
        IrOperand::Imm(1),
        None,
    ));
    block.push(IrInstruction::new(
        IrOpcode::Mov,
        Some(IrReg::X(0)),
        IrOperand::Imm(42),
        None,
    ));
    block.push(IrInstruction::new(
        IrOpcode::Return,
        None,
        IrOperand::Reg(IrReg::X(0)),
        None,
    ));

    let code = X86Backend::compile_block(&block);
    assert_eq!(code[0], 0xe9);
    assert_eq!(i32::from_le_bytes(code[1..5].try_into().unwrap()), 10);
    let exec = X86Backend::emit_executable(&block).expect("failed to emit branch block");
    let func: extern "C" fn() -> u64 = unsafe { std::mem::transmute(exec.as_ptr()) };
    assert_eq!(func(), 42);
}

#[test]
fn test_riscv_mapping_x0_through_x28_is_unique() {
    let mapped: Vec<u8> = (0..=28)
        .map(|index| RiscvBackend::map_reg(IrReg::X(index)) as u8)
        .collect();
    let unique: std::collections::HashSet<u8> = mapped.iter().copied().collect();

    assert_eq!(mapped.len(), 29);
    assert_eq!(unique.len(), mapped.len());
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_x86_executable_conditional_branch_displacement() {
    let mut block = IrBlock::new(0x1000);
    block.push(IrInstruction::new(
        IrOpcode::Mov,
        Some(IrReg::X(0)),
        IrOperand::Imm(1),
        None,
    ));
    block.push(IrInstruction::new(
        IrOpcode::Cmp,
        None,
        IrOperand::Reg(IrReg::X(0)),
        Some(IrOperand::Imm(1)),
    ));
    block.push(IrInstruction::new(
        IrOpcode::CondBranch(lar::jit::ConditionCode::EQ),
        None,
        IrOperand::Imm(8),
        None,
    ));
    block.push(IrInstruction::new(
        IrOpcode::Mov,
        Some(IrReg::X(0)),
        IrOperand::Imm(42),
        None,
    ));
    block.push(IrInstruction::new(
        IrOpcode::Return,
        None,
        IrOperand::Reg(IrReg::X(0)),
        None,
    ));

    let exec = X86Backend::emit_executable(&block).expect("failed to emit branch block");
    let func: extern "C" fn() -> u64 = unsafe { std::mem::transmute(exec.as_ptr()) };
    assert_eq!(func(), 1);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_x86_context_jit_updates_guest_state() {
    let mut engine = JitEngine::new();
    let mut ctx = Arm64CpuContext::new();
    ctx.pc = 0x1000;
    ctx.set_lr(0xfeed_beef);

    let opcodes = [0xd2800140, 0x91003c00, 0xd65f03c0];
    engine.execute(&mut ctx, &opcodes);

    assert_eq!(ctx.get_return(), 25);
    assert_eq!(ctx.pc, 0xfeed_beef);
}

#[cfg(target_arch = "x86_64")]
#[test]
fn test_x86_context_jit_updates_nzcv() {
    let mut engine = JitEngine::new();
    let mut ctx = Arm64CpuContext::new();
    ctx.pc = 0x2000;
    let opcodes = [0xd2800320, 0xf100641f, 0xd65f03c0];

    engine.execute(&mut ctx, &opcodes);

    assert_eq!(ctx.get_return(), 25);
    assert!(ctx.flag_z());
    assert!(ctx.flag_c());
    assert!(!ctx.flag_n());
    assert!(!ctx.flag_v());
}

#[test]
fn test_x86_codegen_vector_ops() {
    let mut block = IrBlock::new(0x2000);
    // Vector Add & Sub
    block.push(IrInstruction::new(
        IrOpcode::VecAdd,
        Some(IrReg::V(0)),
        IrOperand::Reg(IrReg::V(0)),
        Some(IrOperand::Reg(IrReg::V(1))),
    ));
    block.push(IrInstruction::new(
        IrOpcode::VecSub,
        Some(IrReg::V(0)),
        IrOperand::Reg(IrReg::V(0)),
        Some(IrOperand::Reg(IrReg::V(1))),
    ));
    block.push(IrInstruction::new(
        IrOpcode::Return,
        None,
        IrOperand::Imm(0),
        None,
    ));

    let code = X86Backend::compile_block(&block);
    assert!(!code.is_empty());
    // Verify SSE2 opcode presence (0x66, 0x0f, 0xfe / 0xfa)
    assert!(code.windows(3).any(|w| w == [0x66, 0x0f, 0xfe]));
    assert!(code.windows(3).any(|w| w == [0x66, 0x0f, 0xfa]));
}

#[test]
fn test_riscv_codegen_barrier_and_vector() {
    let mut block = IrBlock::new(0x3000);
    // RISC-V Memory barrier mapping from ARM Weak Ordering
    block.push(IrInstruction::new(
        IrOpcode::MemoryBarrier,
        None,
        IrOperand::Imm(0),
        None,
    ));
    // RVV 1.0 Vector Add
    block.push(IrInstruction::new(
        IrOpcode::VecAdd,
        Some(IrReg::V(0)),
        IrOperand::Reg(IrReg::V(1)),
        Some(IrOperand::Reg(IrReg::V(2))),
    ));
    block.push(IrInstruction::new(
        IrOpcode::Return,
        None,
        IrOperand::Imm(0),
        None,
    ));

    let words = RiscvBackend::compile_block(&block);
    assert_eq!(words.len(), 3);
    assert_eq!(words[0], 0x0ff0000f); // fence rw, rw (Native Weak Ordering Barrier)
    assert_eq!(words[2], 0x00008067); // ret (jalr x0, 0(ra))
}
