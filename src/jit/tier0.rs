//! In-Memory Tier-0 Fast JIT for Runtime Code Unpackers and Dynamic Blocks.
//!
//! Provides instant execution for dynamic code unpacked at runtime (Tencent, Bangcle, SecNeo)
//! without the overhead of heavy compiler pipelines.

use crate::arch::context::Arm64CpuContext;
use crate::jit::decoder::{Arm64Decoder, Arm64Inst, Arm64Op, ConditionCode};

/// Tier-0 Fast Execution Engine.
pub struct Tier0FastJit;

impl Tier0FastJit {
    /// Executes a single ARM64 instruction directly on the CPU context.
    /// Returns `true` to continue sequential execution, or `false` on branch/return.
    pub fn step(ctx: &mut Arm64CpuContext, inst: &Arm64Inst) -> bool {
        match inst.op {
            Arm64Op::Nop => true,
            Arm64Op::Add => {
                let op1 = ctx.regs[inst.rn as usize];
                let op2 = if inst.rm != 0 {
                    ctx.regs[inst.rm as usize] << inst.shift
                } else {
                    inst.imm as u64
                };
                let val = if inst.is_64bit {
                    op1.wrapping_add(op2)
                } else {
                    (op1 as u32).wrapping_add(op2 as u32) as u64
                };
                ctx.regs[inst.rd as usize] = val;
                true
            }
            Arm64Op::Adds => {
                let op1 = ctx.regs[inst.rn as usize];
                let op2 = if inst.rm != 0 {
                    ctx.regs[inst.rm as usize]
                } else {
                    inst.imm as u64
                };
                let res = op1.wrapping_add(op2);
                ctx.set_flags(
                    (res as i64) < 0,
                    res == 0,
                    res < op1,
                    ((op1 ^ res) & (op2 ^ res) & 0x8000_0000_0000_0000) != 0,
                );
                ctx.regs[inst.rd as usize] = res;
                true
            }
            Arm64Op::Sub => {
                let op1 = ctx.regs[inst.rn as usize];
                let op2 = if inst.rm != 0 {
                    ctx.regs[inst.rm as usize] << inst.shift
                } else {
                    inst.imm as u64
                };
                let val = if inst.is_64bit {
                    op1.wrapping_sub(op2)
                } else {
                    (op1 as u32).wrapping_sub(op2 as u32) as u64
                };
                ctx.regs[inst.rd as usize] = val;
                true
            }
            Arm64Op::Subs | Arm64Op::Cmp => {
                let op1 = ctx.regs[inst.rn as usize];
                let op2 = if inst.rm != 0 {
                    ctx.regs[inst.rm as usize]
                } else {
                    inst.imm as u64
                };
                let res = op1.wrapping_sub(op2);
                ctx.set_flags(
                    (res as i64) < 0,
                    res == 0,
                    op1 >= op2,
                    ((op1 ^ op2) & (op1 ^ res) & 0x8000_0000_0000_0000) != 0,
                );
                if inst.op != Arm64Op::Cmp {
                    ctx.regs[inst.rd as usize] = res;
                }
                true
            }
            Arm64Op::Mov | Arm64Op::Movz => {
                ctx.regs[inst.rd as usize] = (inst.imm as u64) << inst.shift;
                true
            }
            Arm64Op::Movk => {
                let mask = !(0xffffu64 << inst.shift);
                let val = (ctx.regs[inst.rd as usize] & mask) | ((inst.imm as u64) << inst.shift);
                ctx.regs[inst.rd as usize] = val;
                true
            }
            Arm64Op::And => {
                let op1 = ctx.regs[inst.rn as usize];
                let op2 = ctx.regs[inst.rm as usize];
                ctx.regs[inst.rd as usize] = op1 & op2;
                true
            }
            Arm64Op::Orr => {
                let op1 = ctx.regs[inst.rn as usize];
                let op2 = ctx.regs[inst.rm as usize];
                ctx.regs[inst.rd as usize] = op1 | op2;
                true
            }
            Arm64Op::Eor => {
                let op1 = ctx.regs[inst.rn as usize];
                let op2 = ctx.regs[inst.rm as usize];
                ctx.regs[inst.rd as usize] = op1 ^ op2;
                true
            }
            Arm64Op::Ldr => {
                let base = ctx.regs[inst.rn as usize];
                let addr = (base as i64 + inst.imm) as usize;
                if addr != 0 {
                    let val = unsafe { *(addr as *const u64) };
                    ctx.regs[inst.rd as usize] = val;
                }
                true
            }
            Arm64Op::Str => {
                let base = ctx.regs[inst.rn as usize];
                let addr = (base as i64 + inst.imm) as usize;
                if addr != 0 {
                    let val = ctx.regs[inst.rd as usize];
                    unsafe { *(addr as *mut u64) = val };
                }
                true
            }
            Arm64Op::B => {
                ctx.pc = (ctx.pc as i64 + inst.imm) as u64;
                false
            }
            Arm64Op::Bcc(cond) => {
                if Self::eval_condition(ctx, cond) {
                    ctx.pc = (ctx.pc as i64 + inst.imm) as u64;
                } else {
                    ctx.pc += 4;
                }
                false
            }
            Arm64Op::Cbz => {
                if ctx.regs[inst.rd as usize] == 0 {
                    ctx.pc = (ctx.pc as i64 + inst.imm) as u64;
                } else {
                    ctx.pc += 4;
                }
                false
            }
            Arm64Op::Cbnz => {
                if ctx.regs[inst.rd as usize] != 0 {
                    ctx.pc = (ctx.pc as i64 + inst.imm) as u64;
                } else {
                    ctx.pc += 4;
                }
                false
            }
            Arm64Op::Ret => {
                let target = ctx.regs[inst.rn as usize];
                ctx.pc = target;
                false
            }
            _ => {
                // Advance PC past unsupported or nop-like instruction
                ctx.pc += 4;
                true
            }
        }
    }

    /// Evaluates condition flags.
    pub fn eval_condition(ctx: &Arm64CpuContext, cond: ConditionCode) -> bool {
        match cond {
            ConditionCode::EQ => ctx.flag_z(),
            ConditionCode::NE => !ctx.flag_z(),
            ConditionCode::CS => ctx.flag_c(),
            ConditionCode::CC => !ctx.flag_c(),
            ConditionCode::MI => ctx.flag_n(),
            ConditionCode::PL => !ctx.flag_n(),
            ConditionCode::VS => ctx.flag_v(),
            ConditionCode::VC => !ctx.flag_v(),
            ConditionCode::HI => ctx.flag_c() && !ctx.flag_z(),
            ConditionCode::LS => !ctx.flag_c() || ctx.flag_z(),
            ConditionCode::GE => ctx.flag_n() == ctx.flag_v(),
            ConditionCode::LT => ctx.flag_n() != ctx.flag_v(),
            ConditionCode::GT => !ctx.flag_z() && (ctx.flag_n() == ctx.flag_v()),
            ConditionCode::LE => ctx.flag_z() || (ctx.flag_n() != ctx.flag_v()),
            ConditionCode::AL => true,
            ConditionCode::NV => false,
        }
    }

    /// Executes a continuous block of ARM64 opcodes from memory until branch or limit.
    pub fn execute_block(ctx: &mut Arm64CpuContext, opcodes: &[u32], max_steps: usize) -> usize {
        let mut steps = 0;
        for &raw in opcodes {
            if steps >= max_steps {
                break;
            }
            let inst = Arm64Decoder::decode(raw);
            steps += 1;
            if !Self::step(ctx, &inst) {
                break;
            }
            ctx.pc += 4;
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier0_execution() {
        let mut ctx = Arm64CpuContext::new();
        // 1. mov x0, #10 (0xd2800140)
        // 2. add x0, x0, #15 (0x91003c00)
        // 3. cmp x0, #25 (0xf100641f)
        // 4. ret (0xd65f03c0)
        let opcodes = [0xd2800140, 0x91003c00, 0xf100641f, 0xd65f03c0];

        let steps = Tier0FastJit::execute_block(&mut ctx, &opcodes, 10);
        assert_eq!(steps, 4);
        assert_eq!(ctx.get_arg(0), 25);
        assert!(ctx.flag_z()); // Equal to 25
    }
}
