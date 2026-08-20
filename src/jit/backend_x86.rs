//! x86_64 Machine Code Generator and NEON -> AVX2/SSE Vector Converter.
//!
//! Emits 64-bit AMD64 machine code from IR, mapping ARM64 calling conventions and registers,
//! and translating 128-bit NEON vector operations into SSE2/AVX instructions.

use crate::arch::Arm64CpuContext;
use crate::jit::decoder::ConditionCode;
use crate::jit::ir::{IrBlock, IrInstruction, IrOpcode, IrOperand, IrReg};
use crate::memory::mmap::{MemoryError, MemoryRegion, ProtFlags};

/// x86_64 Register encodings (0..15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum X86Reg {
    Rax = 0,
    Rcx = 1,
    Rdx = 2,
    Rbx = 3,
    Rsp = 4,
    Rbp = 5,
    Rsi = 6,
    Rdi = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
}

impl X86Reg {
    #[inline]
    pub fn is_extended(&self) -> bool {
        (*self as u8) >= 8
    }

    #[inline]
    pub fn low_3bits(&self) -> u8 {
        (*self as u8) & 0x7
    }
}

/// x86_64 Machine Code Emitter.
pub struct X86Backend;

const REG_SP_OFFSET: i32 = 248;
const REG_PC_OFFSET: i32 = 256;

pub type X86ContextFn = unsafe extern "C" fn(*mut Arm64CpuContext);

impl X86Backend {
    /// Maps an ARM64 register to an x86_64 register.
    pub fn map_reg(reg: IrReg) -> X86Reg {
        match reg {
            IrReg::X(0) => X86Reg::Rdi,
            IrReg::X(1) => X86Reg::Rsi,
            IrReg::X(2) => X86Reg::Rdx,
            IrReg::X(3) => X86Reg::Rcx,
            IrReg::X(4) => X86Reg::R8,
            IrReg::X(5) => X86Reg::R9,
            IrReg::X(6) => X86Reg::R10,
            IrReg::X(7) => X86Reg::R11,
            IrReg::X(8) => X86Reg::Rax,
            IrReg::X(29) => X86Reg::Rbp,
            IrReg::X(30) => X86Reg::R12,
            IrReg::SP => X86Reg::Rsp,
            IrReg::X(n) => {
                let idx = (n % 6) + 10;
                match idx {
                    10 => X86Reg::R10,
                    11 => X86Reg::R11,
                    12 => X86Reg::R12,
                    13 => X86Reg::R13,
                    14 => X86Reg::R14,
                    _ => X86Reg::R15,
                }
            }
            _ => X86Reg::Rax,
        }
    }

    /// Compiles an IR block into x86_64 machine code bytes.
    pub fn compile_block(block: &IrBlock) -> Vec<u8> {
        let mut code = Vec::with_capacity(block.instructions.len() * 8);
        let mut instruction_offsets = Vec::with_capacity(block.instructions.len() + 1);
        let mut branches = Vec::new();

        for (index, inst) in block.instructions.iter().enumerate() {
            instruction_offsets.push(code.len());
            match (inst.opcode, inst.src1) {
                (IrOpcode::Branch, IrOperand::Imm(offset)) => {
                    code.push(0xe9);
                    let displacement_offset = code.len();
                    code.extend_from_slice(&0i32.to_le_bytes());
                    branches.push((index, offset, displacement_offset));
                }
                (IrOpcode::CondBranch(cond), IrOperand::Imm(offset)) => {
                    if cond == ConditionCode::AL {
                        code.push(0xe9);
                    } else if cond == ConditionCode::NV {
                        continue;
                    } else {
                        code.extend_from_slice(&[0x0f, Self::condition_opcode(cond)]);
                    }
                    let displacement_offset = code.len();
                    code.extend_from_slice(&0i32.to_le_bytes());
                    branches.push((index, offset, displacement_offset));
                }
                _ => Self::emit_instruction(&mut code, inst),
            }
        }
        instruction_offsets.push(code.len());

        for (index, guest_offset, displacement_offset) in branches {
            let target = index as i64 + guest_offset / 4;
            if guest_offset % 4 != 0 || !(0..instruction_offsets.len() as i64).contains(&target) {
                continue;
            }
            let displacement =
                instruction_offsets[target as usize] as i64 - (displacement_offset + 4) as i64;
            if let Ok(displacement) = i32::try_from(displacement) {
                code[displacement_offset..displacement_offset + 4]
                    .copy_from_slice(&displacement.to_le_bytes());
            }
        }

        code
    }

    fn condition_opcode(cond: ConditionCode) -> u8 {
        match cond {
            ConditionCode::EQ => 0x84,
            ConditionCode::NE => 0x85,
            ConditionCode::CS => 0x83,
            ConditionCode::CC => 0x82,
            ConditionCode::MI => 0x88,
            ConditionCode::PL => 0x89,
            ConditionCode::VS => 0x80,
            ConditionCode::VC => 0x81,
            ConditionCode::HI => 0x87,
            ConditionCode::LS => 0x86,
            ConditionCode::GE => 0x8d,
            ConditionCode::LT => 0x8c,
            ConditionCode::GT => 0x8f,
            ConditionCode::LE => 0x8e,
            ConditionCode::AL | ConditionCode::NV => unreachable!(),
        }
    }

    pub fn compile_context_block(block: &IrBlock) -> Result<Vec<u8>, String> {
        let mut code = Vec::with_capacity(block.instructions.len() * 32 + 32);
        code.extend_from_slice(&[0x55, 0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57]);
        code.extend_from_slice(&[0x49, 0x89, 0xff]);

        for inst in &block.instructions {
            match inst.opcode {
                IrOpcode::Nop => Self::emit_context_pc_step(&mut code),
                IrOpcode::Mov => Self::emit_context_mov(&mut code, inst)?,
                IrOpcode::Add => Self::emit_context_binary(&mut code, inst, 0x01)?,
                IrOpcode::Sub => Self::emit_context_binary(&mut code, inst, 0x29)?,
                IrOpcode::And => Self::emit_context_binary(&mut code, inst, 0x21)?,
                IrOpcode::Orr => Self::emit_context_binary(&mut code, inst, 0x09)?,
                IrOpcode::Eor => Self::emit_context_binary(&mut code, inst, 0x31)?,
                IrOpcode::Cmp => return Err("context CMP requires Tier-0 fallback".into()),
                IrOpcode::MemoryBarrier => code.extend_from_slice(&[0x0f, 0xae, 0xf0]),
                IrOpcode::Return => {
                    Self::emit_context_load(&mut code, 0, REG_PC_OFFSET);
                    Self::emit_context_load_reg(&mut code, inst.src1, 1)?;
                    Self::emit_context_store(&mut code, 1, REG_PC_OFFSET);
                    Self::emit_context_epilogue(&mut code);
                }
                _ => return Err(format!("unsupported context IR opcode: {:?}", inst.opcode)),
            }
        }

        if !matches!(
            block.instructions.last().map(|i| i.opcode),
            Some(IrOpcode::Return)
        ) {
            Self::emit_context_pc_step(&mut code);
            Self::emit_context_epilogue(&mut code);
        }
        Ok(code)
    }

    fn emit_context_mov(code: &mut Vec<u8>, inst: &IrInstruction) -> Result<(), String> {
        let dst = match inst.dst {
            Some(IrReg::X(reg)) if reg < 31 => reg,
            _ => return Err("invalid context MOV destination".into()),
        };
        match inst.src1 {
            IrOperand::Imm(value) => {
                Self::emit_mov_imm(code, value);
                Self::emit_context_store(code, 0, dst as i32 * 8);
            }
            IrOperand::Reg(reg) => {
                Self::emit_context_load_reg(code, IrOperand::Reg(reg), 0)?;
                Self::emit_context_store(code, 0, dst as i32 * 8);
            }
            _ => return Err("invalid context MOV source".into()),
        }
        Self::emit_context_pc_step(code);
        Ok(())
    }

    fn emit_context_binary(
        code: &mut Vec<u8>,
        inst: &IrInstruction,
        opcode: u8,
    ) -> Result<(), String> {
        let dst = match inst.dst {
            Some(IrReg::X(reg)) if reg < 31 => reg,
            _ => return Err("invalid context binary destination".into()),
        };
        Self::emit_context_load_reg(code, inst.src1, 0)?;
        match inst.src2 {
            Some(IrOperand::Reg(reg)) => Self::emit_context_load_reg(code, IrOperand::Reg(reg), 1)?,
            Some(IrOperand::Imm(value)) => Self::emit_mov_imm_to(code, value, 1),
            _ => return Err("invalid context binary operand".into()),
        }
        Self::emit_reg_binary(code, opcode, 0, 1);
        Self::emit_context_store(code, 0, dst as i32 * 8);
        Self::emit_context_pc_step(code);
        Ok(())
    }

    fn emit_context_load_reg(
        code: &mut Vec<u8>,
        operand: IrOperand,
        reg: u8,
    ) -> Result<(), String> {
        match operand {
            IrOperand::Reg(IrReg::X(index)) if index < 31 => {
                Self::emit_context_load(code, reg, index as i32 * 8);
                Ok(())
            }
            IrOperand::Reg(IrReg::SP) => {
                Self::emit_context_load(code, reg, REG_SP_OFFSET);
                Ok(())
            }
            _ => Err("unsupported context register operand".into()),
        }
    }

    fn emit_mov_imm_to(code: &mut Vec<u8>, value: i64, reg: u8) {
        let opcode = 0xb8 + reg;
        code.extend_from_slice(&[0x48, opcode]);
        code.extend_from_slice(&(value as u64).to_le_bytes());
    }

    fn emit_mov_imm(code: &mut Vec<u8>, value: i64) {
        Self::emit_mov_imm_to(code, value, 0);
    }

    fn emit_context_load(code: &mut Vec<u8>, reg: u8, offset: i32) {
        let rex = 0x49 | if reg >= 8 { 0x04 } else { 0 };
        code.extend_from_slice(&[rex, 0x8b, 0x87 | ((reg & 7) << 3)]);
        code.extend_from_slice(&offset.to_le_bytes());
    }

    fn emit_context_store(code: &mut Vec<u8>, reg: u8, offset: i32) {
        let rex = 0x49 | if reg >= 8 { 0x04 } else { 0 };
        code.extend_from_slice(&[rex, 0x89, 0x87 | ((reg & 7) << 3)]);
        code.extend_from_slice(&offset.to_le_bytes());
    }

    fn emit_reg_binary(code: &mut Vec<u8>, opcode: u8, dst: u8, src: u8) {
        let rex = 0x48 | if src >= 8 { 0x04 } else { 0 } | if dst >= 8 { 0x01 } else { 0 };
        code.extend_from_slice(&[rex, opcode, 0xc0 | ((src & 7) << 3) | (dst & 7)]);
    }

    fn emit_context_pc_step(code: &mut Vec<u8>) {
        Self::emit_context_load(code, 0, REG_PC_OFFSET);
        Self::emit_mov_imm_to(code, 4, 1);
        Self::emit_reg_binary(code, 0x01, 0, 1);
        Self::emit_context_store(code, 0, REG_PC_OFFSET);
    }

    fn emit_context_epilogue(code: &mut Vec<u8>) {
        code.extend_from_slice(&[
            0x41, 0x5f, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0x5d, 0xc3,
        ]);
    }

    fn emit_instruction(code: &mut Vec<u8>, inst: &IrInstruction) {
        match inst.opcode {
            IrOpcode::Nop => {
                code.push(0x90); // NOP
            }
            IrOpcode::Return => {
                // If returning x0, move mapped rdi into rax if needed
                code.push(0x48);
                code.push(0x89);
                code.push(0xf8); // mov rax, rdi
                code.push(0xc3); // ret
            }
            IrOpcode::Mov => {
                if let (Some(dst), IrOperand::Imm(imm)) = (inst.dst, inst.src1) {
                    let r = Self::map_reg(dst);
                    // REX.W prefix: 0x48 + (1 if reg >= 8)
                    let rex = if r.is_extended() { 0x49 } else { 0x48 };
                    code.push(rex);
                    code.push(0xb8 + r.low_3bits()); // movabs reg, imm64
                    code.extend_from_slice(&imm.to_le_bytes());
                } else if let (Some(dst), IrOperand::Reg(src)) = (inst.dst, inst.src1) {
                    let rd = Self::map_reg(dst);
                    let rs = Self::map_reg(src);
                    Self::emit_mov_reg_reg(code, rd, rs);
                }
            }
            IrOpcode::Add => {
                if let Some(dst) = inst.dst {
                    let rd = Self::map_reg(dst);
                    // First move src1 into dst if not identical
                    if let IrOperand::Reg(src1) = inst.src1 {
                        let rs1 = Self::map_reg(src1);
                        if rd != rs1 {
                            Self::emit_mov_reg_reg(code, rd, rs1);
                        }
                    }
                    // Then add src2
                    if let Some(IrOperand::Imm(imm)) = inst.src2 {
                        Self::emit_add_imm(code, rd, imm);
                    } else if let Some(IrOperand::Reg(src2)) = inst.src2 {
                        let rs2 = Self::map_reg(src2);
                        Self::emit_add_reg_reg(code, rd, rs2);
                    }
                }
            }
            IrOpcode::Sub => {
                if let Some(dst) = inst.dst {
                    let rd = Self::map_reg(dst);
                    if let IrOperand::Reg(src1) = inst.src1 {
                        let rs1 = Self::map_reg(src1);
                        if rd != rs1 {
                            Self::emit_mov_reg_reg(code, rd, rs1);
                        }
                    }
                    if let Some(IrOperand::Imm(imm)) = inst.src2 {
                        Self::emit_sub_imm(code, rd, imm);
                    } else if let Some(IrOperand::Reg(src2)) = inst.src2 {
                        let rs2 = Self::map_reg(src2);
                        Self::emit_sub_reg_reg(code, rd, rs2);
                    }
                }
            }
            IrOpcode::Cmp => {
                if let IrOperand::Reg(src1) = inst.src1 {
                    let r1 = Self::map_reg(src1);
                    if let Some(IrOperand::Imm(imm)) = inst.src2 {
                        let rex = if r1.is_extended() { 0x49 } else { 0x48 };
                        code.push(rex);
                        code.push(0x81);
                        code.push(0xf8 + r1.low_3bits()); // cmp reg, imm32
                        code.extend_from_slice(&(imm as i32).to_le_bytes());
                    }
                }
            }
            IrOpcode::Branch => {
                if let IrOperand::Imm(offset) = inst.src1 {
                    code.push(0xe9); // jmp rel32
                    code.extend_from_slice(&(offset as i32).to_le_bytes());
                }
            }
            IrOpcode::CondBranch(cond) => {
                if let IrOperand::Imm(offset) = inst.src1 {
                    let jcc_byte = match cond {
                        ConditionCode::EQ => 0x84,                     // je
                        ConditionCode::NE => 0x85,                     // jne
                        ConditionCode::CS | ConditionCode::HI => 0x87, // ja
                        ConditionCode::CC | ConditionCode::LS => 0x86, // jbe
                        ConditionCode::GE => 0x8d,                     // jge
                        ConditionCode::LT => 0x8c,                     // jl
                        ConditionCode::GT => 0x8f,                     // jg
                        ConditionCode::LE => 0x8e,                     // jle
                        _ => 0x84,
                    };
                    code.push(0x0f);
                    code.push(jcc_byte);
                    code.extend_from_slice(&(offset as i32).to_le_bytes());
                }
            }
            IrOpcode::Syscall => {
                code.push(0x0f);
                code.push(0x05); // syscall
            }
            IrOpcode::MemoryBarrier => {
                // x86 TSO provides Total Store Order by default, mfence for full barrier
                code.push(0x0f);
                code.push(0xae);
                code.push(0xf0); // mfence
            }
            IrOpcode::VecAdd => {
                // SSE2 paddd xmm0, xmm1 (66 0f fe c1)
                code.push(0x66);
                code.push(0x0f);
                code.push(0xfe);
                code.push(0xc1);
            }
            IrOpcode::VecSub => {
                // SSE2 psubd xmm0, xmm1 (66 0f fa c1)
                code.push(0x66);
                code.push(0x0f);
                code.push(0xfa);
                code.push(0xc1);
            }
            _ => {
                code.push(0x90); // NOP fallback
            }
        }
    }

    fn emit_mov_reg_reg(code: &mut Vec<u8>, dst: X86Reg, src: X86Reg) {
        let mut rex = 0x48;
        if src.is_extended() {
            rex |= 0x04;
        }
        if dst.is_extended() {
            rex |= 0x01;
        }
        code.push(rex);
        code.push(0x89);
        let modrm = 0xc0 | (src.low_3bits() << 3) | dst.low_3bits();
        code.push(modrm);
    }

    fn emit_add_reg_reg(code: &mut Vec<u8>, dst: X86Reg, src: X86Reg) {
        let mut rex = 0x48;
        if src.is_extended() {
            rex |= 0x04;
        }
        if dst.is_extended() {
            rex |= 0x01;
        }
        code.push(rex);
        code.push(0x01);
        let modrm = 0xc0 | (src.low_3bits() << 3) | dst.low_3bits();
        code.push(modrm);
    }

    fn emit_add_imm(code: &mut Vec<u8>, dst: X86Reg, imm: i64) {
        let rex = if dst.is_extended() { 0x49 } else { 0x48 };
        code.push(rex);
        code.push(0x81);
        code.push(0xc0 + dst.low_3bits());
        code.extend_from_slice(&(imm as i32).to_le_bytes());
    }

    fn emit_sub_reg_reg(code: &mut Vec<u8>, dst: X86Reg, src: X86Reg) {
        let mut rex = 0x48;
        if src.is_extended() {
            rex |= 0x04;
        }
        if dst.is_extended() {
            rex |= 0x01;
        }
        code.push(rex);
        code.push(0x29);
        let modrm = 0xc0 | (src.low_3bits() << 3) | dst.low_3bits();
        code.push(modrm);
    }

    fn emit_sub_imm(code: &mut Vec<u8>, dst: X86Reg, imm: i64) {
        let rex = if dst.is_extended() { 0x49 } else { 0x48 };
        code.push(rex);
        code.push(0x81);
        code.push(0xe8 + dst.low_3bits());
        code.extend_from_slice(&(imm as i32).to_le_bytes());
    }

    /// Emits code directly into executable virtual memory region.
    pub fn emit_executable(block: &IrBlock) -> Result<MemoryRegion, MemoryError> {
        let code_bytes = Self::compile_block(block);
        let mut region = MemoryRegion::allocate_16k(code_bytes.len(), ProtFlags::READ_WRITE)?;
        region.write_at(0, &code_bytes)?;
        region.protect(ProtFlags::READ_EXEC)?;
        Ok(region)
    }

    pub fn emit_context_executable(block: &IrBlock) -> Result<MemoryRegion, MemoryError> {
        let code_bytes =
            Self::compile_context_block(block).map_err(|_| MemoryError::AllocationFailed {
                size: 0,
                errno: libc::EINVAL,
            })?;
        let mut region = MemoryRegion::allocate_16k(code_bytes.len(), ProtFlags::READ_WRITE)?;
        region.write_at(0, &code_bytes)?;
        region.protect(ProtFlags::READ_EXEC)?;
        Ok(region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x86_compile_and_execute() {
        let mut block = IrBlock::new(0x1000);
        // mov rdi, 42
        block.push(IrInstruction::new(
            IrOpcode::Mov,
            Some(IrReg::X(0)),
            IrOperand::Imm(42),
            None,
        ));
        // add rdi, 58
        block.push(IrInstruction::new(
            IrOpcode::Add,
            Some(IrReg::X(0)),
            IrOperand::Reg(IrReg::X(0)),
            Some(IrOperand::Imm(58)),
        ));
        // return
        block.push(IrInstruction::new(
            IrOpcode::Return,
            None,
            IrOperand::Reg(IrReg::X(0)),
            None,
        ));

        let exec_region =
            X86Backend::emit_executable(&block).expect("Failed to emit executable block");

        // Call the compiled function: () -> u64
        let func: extern "C" fn() -> u64 = unsafe { std::mem::transmute(exec_region.as_ptr()) };
        let res = func();
        assert_eq!(res, 100);
    }
}
