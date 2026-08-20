//! Architecture-Neutral Intermediate Representation (IR) for Multi-Target JIT.
//!
//! Provides an SSA/3-address code IR for optimizing and emitting machine code
//! targeting x86_64, RISC-V 64, or direct execution.

use crate::jit::decoder::{Arm64Inst, Arm64Op, ConditionCode};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedInstruction {
    pub raw: u32,
}

impl fmt::Display for UnsupportedInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported ARM64 instruction 0x{:08x}", self.raw)
    }
}

impl std::error::Error for UnsupportedInstruction {}

/// Register identifier in IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IrReg {
    /// Guest ARM64 general purpose register (x0..x30).
    X(u8),
    /// Guest ARM64 vector register (v0..v31).
    V(u8),
    /// Temporary virtual register.
    Temp(u32),
    /// Stack Pointer.
    SP,
    /// Program Counter.
    PC,
}

/// Operand in IR instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrOperand {
    Reg(IrReg),
    Imm(i64),
    Memory { base: IrReg, offset: i64 },
}

/// Operations supported in IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrOpcode {
    Nop,
    Mov,
    Add,
    Sub,
    Mul,
    Sdiv,
    Udiv,
    And,
    Orr,
    Eor,
    Shl,
    Shr,
    Sar,
    Cmp,
    Load64,
    Load32,
    Store64,
    Store32,
    Branch,
    CondBranch(ConditionCode),
    Call,
    Return,
    Syscall,
    MemoryBarrier,
    VecAdd,
    VecSub,
    VecMul,
}

/// Intermediate Representation Instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrInstruction {
    pub opcode: IrOpcode,
    pub dst: Option<IrReg>,
    pub src1: IrOperand,
    pub src2: Option<IrOperand>,
}

impl IrInstruction {
    pub fn new(
        opcode: IrOpcode,
        dst: Option<IrReg>,
        src1: IrOperand,
        src2: Option<IrOperand>,
    ) -> Self {
        Self {
            opcode,
            dst,
            src1,
            src2,
        }
    }
}

/// Basic Block consisting of a sequence of IR instructions ending in a terminator.
#[derive(Debug, Clone, Default)]
pub struct IrBlock {
    pub start_address: u64,
    pub instructions: Vec<IrInstruction>,
}

impl IrBlock {
    pub fn new(start_address: u64) -> Self {
        Self {
            start_address,
            instructions: Vec::new(),
        }
    }

    pub fn push(&mut self, inst: IrInstruction) {
        self.instructions.push(inst);
    }

    /// Converts a decoded ARM64 instruction and appends corresponding IR instructions.
    pub fn translate_arm64_inst(&mut self, inst: &Arm64Inst) {
        match inst.op {
            Arm64Op::Nop => {
                self.push(IrInstruction::new(
                    IrOpcode::Nop,
                    None,
                    IrOperand::Imm(0),
                    None,
                ));
            }
            Arm64Op::Add => {
                let src2 = if inst.rm != 0 {
                    IrOperand::Reg(IrReg::X(inst.rm))
                } else {
                    IrOperand::Imm(inst.imm)
                };
                self.push(IrInstruction::new(
                    IrOpcode::Add,
                    Some(IrReg::X(inst.rd)),
                    IrOperand::Reg(IrReg::X(inst.rn)),
                    Some(src2),
                ));
            }
            Arm64Op::Sub => {
                let src2 = if inst.rm != 0 {
                    IrOperand::Reg(IrReg::X(inst.rm))
                } else {
                    IrOperand::Imm(inst.imm)
                };
                self.push(IrInstruction::new(
                    IrOpcode::Sub,
                    Some(IrReg::X(inst.rd)),
                    IrOperand::Reg(IrReg::X(inst.rn)),
                    Some(src2),
                ));
            }
            Arm64Op::Cmp => {
                let src2 = if inst.rm != 0 {
                    IrOperand::Reg(IrReg::X(inst.rm))
                } else {
                    IrOperand::Imm(inst.imm)
                };
                self.push(IrInstruction::new(
                    IrOpcode::Cmp,
                    None,
                    IrOperand::Reg(IrReg::X(inst.rn)),
                    Some(src2),
                ));
            }
            Arm64Op::Mov | Arm64Op::Movz => {
                self.push(IrInstruction::new(
                    IrOpcode::Mov,
                    Some(IrReg::X(inst.rd)),
                    IrOperand::Imm(inst.imm << inst.shift),
                    None,
                ));
            }
            Arm64Op::Ldr => {
                self.push(IrInstruction::new(
                    if inst.is_64bit {
                        IrOpcode::Load64
                    } else {
                        IrOpcode::Load32
                    },
                    Some(IrReg::X(inst.rd)),
                    IrOperand::Memory {
                        base: IrReg::X(inst.rn),
                        offset: inst.imm,
                    },
                    None,
                ));
            }
            Arm64Op::Str => {
                self.push(IrInstruction::new(
                    if inst.is_64bit {
                        IrOpcode::Store64
                    } else {
                        IrOpcode::Store32
                    },
                    None,
                    IrOperand::Reg(IrReg::X(inst.rd)),
                    Some(IrOperand::Memory {
                        base: IrReg::X(inst.rn),
                        offset: inst.imm,
                    }),
                ));
            }
            Arm64Op::B => {
                self.push(IrInstruction::new(
                    IrOpcode::Branch,
                    None,
                    IrOperand::Imm(inst.imm),
                    None,
                ));
            }
            Arm64Op::Bcc(cond) => {
                self.push(IrInstruction::new(
                    IrOpcode::CondBranch(cond),
                    None,
                    IrOperand::Imm(inst.imm),
                    None,
                ));
            }
            Arm64Op::Ret => {
                self.push(IrInstruction::new(
                    IrOpcode::Return,
                    None,
                    IrOperand::Reg(IrReg::X(inst.rn)),
                    None,
                ));
            }
            Arm64Op::Svc => {
                self.push(IrInstruction::new(
                    IrOpcode::Syscall,
                    None,
                    IrOperand::Imm(inst.imm),
                    None,
                ));
            }
            Arm64Op::Dmb | Arm64Op::Dsb => {
                self.push(IrInstruction::new(
                    IrOpcode::MemoryBarrier,
                    None,
                    IrOperand::Imm(inst.imm),
                    None,
                ));
            }
            Arm64Op::Vadd => {
                self.push(IrInstruction::new(
                    IrOpcode::VecAdd,
                    Some(IrReg::V(inst.rd)),
                    IrOperand::Reg(IrReg::V(inst.rn)),
                    Some(IrOperand::Reg(IrReg::V(inst.rm))),
                ));
            }
            Arm64Op::Vsub => {
                self.push(IrInstruction::new(
                    IrOpcode::VecSub,
                    Some(IrReg::V(inst.rd)),
                    IrOperand::Reg(IrReg::V(inst.rn)),
                    Some(IrOperand::Reg(IrReg::V(inst.rm))),
                ));
            }
            _ => {
                // Generic move / raw representation
                self.push(IrInstruction::new(
                    IrOpcode::Nop,
                    None,
                    IrOperand::Imm(inst.raw as i64),
                    None,
                ));
            }
        }
    }

    pub fn translate_arm64_inst_checked(
        &mut self,
        inst: &Arm64Inst,
    ) -> Result<(), UnsupportedInstruction> {
        if matches!(inst.op, Arm64Op::Unknown(_)) {
            return Err(UnsupportedInstruction { raw: inst.raw });
        }
        self.translate_arm64_inst(inst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ir_translation() {
        let mut block = IrBlock::new(0x1000);
        let inst = Arm64Inst {
            op: Arm64Op::Add,
            rd: 0,
            rn: 1,
            rm: 0,
            imm: 100,
            shift: 0,
            is_64bit: true,
            raw: 0,
        };

        block.translate_arm64_inst(&inst);
        assert_eq!(block.instructions.len(), 1);
        assert_eq!(block.instructions[0].opcode, IrOpcode::Add);
        assert_eq!(block.instructions[0].dst, Some(IrReg::X(0)));
    }
}
