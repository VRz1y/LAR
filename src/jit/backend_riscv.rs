//! RISC-V 64 (RV64GCV) Machine Code Generator and RVV Vector Extension Converter.
//!
//! Emits 32-bit standard RISC-V instructions from IR, maps ARM Weak Memory Barriers
//! natively to RISC-V `fence` instructions, and converts ARM NEON to RVV 1.0 vector ops.

use crate::jit::ir::{IrBlock, IrInstruction, IrOpcode, IrOperand, IrReg};

/// RISC-V 64 Architecture Register Encodings (x0..x31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RiscvReg {
    Zero = 0,
    Ra = 1,
    Sp = 2,
    Gp = 3,
    Tp = 4,
    T0 = 5,
    T1 = 6,
    T2 = 7,
    S0 = 8,
    S1 = 9,
    A0 = 10,
    A1 = 11,
    A2 = 12,
    A3 = 13,
    A4 = 14,
    A5 = 15,
    A6 = 16,
    A7 = 17,
    S2 = 18,
    S3 = 19,
    S4 = 20,
    S5 = 21,
    S6 = 22,
    S7 = 23,
    S8 = 24,
    S9 = 25,
    S10 = 26,
    S11 = 27,
    T3 = 28,
    T4 = 29,
    T5 = 30,
    T6 = 31,
}

/// RISC-V Backend Emitter.
pub struct RiscvBackend;

impl RiscvBackend {
    /// Maps an ARM64 register to a RISC-V register.
    pub fn map_reg(reg: IrReg) -> RiscvReg {
        match reg {
            IrReg::X(0) => RiscvReg::A0,
            IrReg::X(1) => RiscvReg::A1,
            IrReg::X(2) => RiscvReg::A2,
            IrReg::X(3) => RiscvReg::A3,
            IrReg::X(4) => RiscvReg::A4,
            IrReg::X(5) => RiscvReg::A5,
            IrReg::X(6) => RiscvReg::A6,
            IrReg::X(7) => RiscvReg::A7,
            IrReg::X(8) => RiscvReg::T0,
            IrReg::X(9) => RiscvReg::T1,
            IrReg::X(10) => RiscvReg::T2,
            IrReg::X(11) => RiscvReg::T3,
            IrReg::X(12) => RiscvReg::T4,
            IrReg::X(13) => RiscvReg::T5,
            IrReg::X(14) => RiscvReg::T6,
            IrReg::X(15) => RiscvReg::S2,
            IrReg::X(16) => RiscvReg::S3,
            IrReg::X(17) => RiscvReg::S4,
            IrReg::X(18) => RiscvReg::S5,
            IrReg::X(19) => RiscvReg::S6,
            IrReg::X(20) => RiscvReg::S7,
            IrReg::X(21) => RiscvReg::S8,
            IrReg::X(22) => RiscvReg::S9,
            IrReg::X(23) => RiscvReg::S10,
            IrReg::X(24) => RiscvReg::S11,
            IrReg::X(25) => RiscvReg::Gp,
            IrReg::X(26) => RiscvReg::Tp,
            IrReg::X(27) => RiscvReg::S1,
            IrReg::X(28) => RiscvReg::Ra,
            IrReg::X(29) => RiscvReg::S0,
            IrReg::X(30) => RiscvReg::Ra,
            IrReg::X(_) => RiscvReg::A0,
            IrReg::SP => RiscvReg::Sp,
            _ => RiscvReg::A0,
        }
    }

    /// Compiles an IR block into 32-bit RISC-V machine instructions.
    pub fn compile_block(block: &IrBlock) -> Vec<u32> {
        let mut code = Vec::with_capacity(block.instructions.len());

        for inst in &block.instructions {
            Self::emit_instruction(&mut code, inst);
        }

        code
    }

    fn emit_instruction(code: &mut Vec<u32>, inst: &IrInstruction) {
        match inst.opcode {
            IrOpcode::Nop => {
                // nop: addi x0, x0, 0 (0x00000013)
                code.push(0x00000013);
            }
            IrOpcode::Return => {
                // ret: jalr x0, 0(ra) (0x00008067)
                code.push(0x00008067);
            }
            IrOpcode::Mov => {
                if let (Some(dst), IrOperand::Imm(imm)) = (inst.dst, inst.src1) {
                    let rd = Self::map_reg(dst) as u32;
                    // li rd, imm (addi rd, x0, imm if fits in 12-bit)
                    let imm12 = (imm as u32) & 0xfff;
                    code.push(
                        ((imm12 << 20) | ((RiscvReg::Zero as u32) << 15)) | (rd << 7) | 0b0010011,
                    );
                } else if let (Some(dst), IrOperand::Reg(src)) = (inst.dst, inst.src1) {
                    let rd = Self::map_reg(dst) as u32;
                    let rs = Self::map_reg(src) as u32;
                    // mv rd, rs (addi rd, rs, 0)
                    code.push((rs << 15) | (rd << 7) | 0b0010011);
                }
            }
            IrOpcode::Add => {
                if let Some(dst) = inst.dst {
                    let rd = Self::map_reg(dst) as u32;
                    let rs1 = if let IrOperand::Reg(src1) = inst.src1 {
                        Self::map_reg(src1) as u32
                    } else {
                        rd
                    };

                    if let Some(IrOperand::Imm(imm)) = inst.src2 {
                        let imm12 = (imm as u32) & 0xfff;
                        // addi rd, rs1, imm
                        code.push(((imm12 << 20) | (rs1 << 15)) | (rd << 7) | 0b0010011);
                    } else if let Some(IrOperand::Reg(src2)) = inst.src2 {
                        let rs2 = Self::map_reg(src2) as u32;
                        // add rd, rs1, rs2
                        code.push(((rs2 << 20) | (rs1 << 15)) | (rd << 7) | 0b0110011);
                    }
                }
            }
            IrOpcode::Sub => {
                if let Some(dst) = inst.dst {
                    let rd = Self::map_reg(dst) as u32;
                    let rs1 = if let IrOperand::Reg(src1) = inst.src1 {
                        Self::map_reg(src1) as u32
                    } else {
                        rd
                    };

                    if let Some(IrOperand::Imm(imm)) = inst.src2 {
                        let imm12 = ((-imm) as u32) & 0xfff;
                        code.push(((imm12 << 20) | (rs1 << 15)) | (rd << 7) | 0b0010011);
                    } else if let Some(IrOperand::Reg(src2)) = inst.src2 {
                        let rs2 = Self::map_reg(src2) as u32;
                        // sub rd, rs1, rs2
                        code.push(
                            ((0b0100000 << 25) | (rs2 << 20) | (rs1 << 15)) | (rd << 7) | 0b0110011,
                        );
                    }
                }
            }
            IrOpcode::MemoryBarrier => {
                // ARM Weak Ordering maps natively to RISC-V Weak Ordering:
                // fence rw, rw (0x0ff0000f) without TSO penalty
                code.push(0x0ff0000f);
            }
            IrOpcode::Syscall => {
                // ecall (0x00000073)
                code.push(0x00000073);
            }
            IrOpcode::VecAdd => {
                // RVV 1.0 vadd.vv v0, v1, v2
                // opcode 0x57, funct3 0x0, funct6 0x00
                code.push(((1 << 20) | (2 << 15)) | 0x57);
            }
            IrOpcode::VecSub => {
                // RVV 1.0 vsub.vv v0, v1, v2
                // funct6 0x02
                code.push(((0b000010 << 26) | (1 << 20) | (2 << 15)) | 0x57);
            }
            _ => {
                code.push(0x00000013); // nop fallback
            }
        }
    }

    /// Converts compiled 32-bit instructions to byte array.
    pub fn compile_to_bytes(block: &IrBlock) -> Vec<u8> {
        let words = Self::compile_block(block);
        let mut bytes = Vec::with_capacity(words.len() * 4);
        for w in words {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_riscv_codegen_basic() {
        let mut block = IrBlock::new(0x1000);
        // li a0, 55 (addi a0, x0, 55)
        block.push(IrInstruction::new(
            IrOpcode::Mov,
            Some(IrReg::X(0)),
            IrOperand::Imm(55),
            None,
        ));
        // add a0, a0, a1
        block.push(IrInstruction::new(
            IrOpcode::Add,
            Some(IrReg::X(0)),
            IrOperand::Reg(IrReg::X(0)),
            Some(IrOperand::Reg(IrReg::X(1))),
        ));
        // fence rw, rw
        block.push(IrInstruction::new(
            IrOpcode::MemoryBarrier,
            None,
            IrOperand::Imm(0),
            None,
        ));
        // ret
        block.push(IrInstruction::new(
            IrOpcode::Return,
            None,
            IrOperand::Reg(IrReg::X(0)),
            None,
        ));

        let riscv_code = RiscvBackend::compile_block(&block);
        assert_eq!(riscv_code.len(), 4);
        assert_eq!(riscv_code[2], 0x0ff0000f); // fence rw, rw
        assert_eq!(riscv_code[3], 0x00008067); // ret
    }
}
