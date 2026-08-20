//! 64-bit ARMv8-A Instruction Decoder.
//!
//! Decodes 32-bit AArch64 machine instructions into structured representations
//! for JIT intermediate representation translation and execution.

use std::fmt;

/// ARM64 Condition codes (B.cond).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionCode {
    EQ = 0,  // Equal (Z == 1)
    NE = 1,  // Not Equal (Z == 0)
    CS = 2,  // Carry Set / Unsigned Higher or Same (C == 1)
    CC = 3,  // Carry Clear / Unsigned Lower (C == 0)
    MI = 4,  // Minus / Negative (N == 1)
    PL = 5,  // Plus / Positive or Zero (N == 0)
    VS = 6,  // Overflow Set (V == 1)
    VC = 7,  // Overflow Clear (V == 0)
    HI = 8,  // Unsigned Higher (C == 1 && Z == 0)
    LS = 9,  // Unsigned Lower or Same (C == 0 || Z == 1)
    GE = 10, // Signed Greater Than or Equal (N == V)
    LT = 11, // Signed Less Than (N != V)
    GT = 12, // Signed Greater Than (Z == 0 && N == V)
    LE = 13, // Signed Less Than or Equal (Z == 1 || N != V)
    AL = 14, // Always
    NV = 15, // Never
}

impl ConditionCode {
    pub fn from_u8(val: u8) -> Self {
        match val & 0xF {
            0 => Self::EQ,
            1 => Self::NE,
            2 => Self::CS,
            3 => Self::CC,
            4 => Self::MI,
            5 => Self::PL,
            6 => Self::VS,
            7 => Self::VC,
            8 => Self::HI,
            9 => Self::LS,
            10 => Self::GE,
            11 => Self::LT,
            12 => Self::GT,
            13 => Self::LE,
            14 => Self::AL,
            _ => Self::NV,
        }
    }
}

/// Decoded ARM64 Operation Types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm64Op {
    // Arithmetic & Logic
    Add,
    Adds,
    Sub,
    Subs,
    Cmp,
    Cmn,
    Mul,
    Smull,
    Umull,
    Sdiv,
    Udiv,
    And,
    Ands,
    Orr,
    Eor,
    Lsl,
    Lsr,
    Asr,
    Ror,
    Clz,
    Rev,
    Mov,
    Movz,
    Movk,
    Movn,

    // Branching & Control Flow
    B,
    Bl,
    Bcc(ConditionCode),
    Cbz,
    Cbnz,
    Tbz,
    Tbnz,
    Br,
    Blr,
    Ret,

    // Memory (Load / Store)
    Ldr,
    Str,
    Ldp,
    Stp,
    Ldrb,
    Strb,
    Ldrh,
    Strh,
    Ldur,
    Stur,

    // Floating-Point & Vector (NEON)
    Fadd,
    Fsub,
    Fmul,
    Fdiv,
    Fsqrt,
    Fabs,
    Fneg,
    Fcmp,
    Fmov,
    Vadd,
    Vsub,
    Vmul,
    Vdup,
    Vmov,

    // System & Barriers
    Dmb,
    Dsb,
    Isb,
    Svc,
    Brk,
    Nop,
    Mrs,
    Msr,

    // Fallback for un-decoded / complex instructions
    Unknown(u32),
}

/// Structured Decoded ARM64 Instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arm64Inst {
    pub op: Arm64Op,
    pub rd: u8,
    pub rn: u8,
    pub rm: u8,
    pub imm: i64,
    pub shift: u8,
    pub is_64bit: bool,
    pub raw: u32,
}

impl Arm64Inst {
    pub fn new(op: Arm64Op, raw: u32) -> Self {
        Self {
            op,
            rd: 0,
            rn: 0,
            rm: 0,
            imm: 0,
            shift: 0,
            is_64bit: true,
            raw,
        }
    }
}

/// AArch64 Instruction Decoder.
pub struct Arm64Decoder;

impl Arm64Decoder {
    /// Decodes a 32-bit little-endian AArch64 opcode.
    pub fn decode(raw: u32) -> Arm64Inst {
        // NOP: 0xd503201f
        if raw == 0xd503201f {
            return Arm64Inst::new(Arm64Op::Nop, raw);
        }

        // RET: 0xd65f03c0 (or RET xn: 1101 0110 0101 1111 0000 00nn nnn0 0000)
        if (raw & 0xfffffc1f) == 0xd65f0000 {
            let rn = ((raw >> 5) & 0x1f) as u8;
            let mut inst = Arm64Inst::new(Arm64Op::Ret, raw);
            inst.rn = rn;
            return inst;
        }

        // SVC #imm: 1101 0100 000i iiii iiii iiii iii0 0001
        if (raw & 0xffe0001f) == 0xd4000001 {
            let imm = ((raw >> 5) & 0xffff) as i64;
            let mut inst = Arm64Inst::new(Arm64Op::Svc, raw);
            inst.imm = imm;
            return inst;
        }

        // BRK #imm: 1101 0100 001i iiii iiii iiii iii0 0000
        if (raw & 0xffe0001f) == 0xd4200000 {
            let imm = ((raw >> 5) & 0xffff) as i64;
            let mut inst = Arm64Inst::new(Arm64Op::Brk, raw);
            inst.imm = imm;
            return inst;
        }

        // Memory Barriers: DMB / DSB / ISB
        if raw == 0xd50339bf || (raw & 0xfffff0ff) == 0xd503309f {
            let mut inst = Arm64Inst::new(Arm64Op::Dsb, raw);
            inst.imm = ((raw >> 8) & 0xf) as i64;
            return inst;
        }
        if (raw & 0xfffff0ff) == 0xd50330bf {
            let barrier_type = ((raw >> 8) & 0xf) as i64;
            let mut inst = Arm64Inst::new(Arm64Op::Dmb, raw);
            inst.imm = barrier_type;
            return inst;
        }
        if raw == 0xd5033fdf {
            return Arm64Inst::new(Arm64Op::Isb, raw);
        }

        // B / BL (Unconditional Branch / Branch with Link): 0/1 001 01ii iiii ...
        // B: 0001 01ii ... (0x14000000 mask 0x7c000000)
        if (raw & 0x7c000000) == 0x14000000 {
            let is_bl = (raw & 0x80000000) != 0;
            let raw_imm = (raw & 0x03ffffff) as i32;
            // Sign-extend 26-bit imm to 32/64 bit, multiply by 4 (pc relative offset)
            let signed_imm = if (raw_imm & 0x02000000) != 0 {
                (raw_imm | !0x03ffffff) as i64 * 4
            } else {
                raw_imm as i64 * 4
            };

            let op = if is_bl { Arm64Op::Bl } else { Arm64Op::B };
            let mut inst = Arm64Inst::new(op, raw);
            inst.imm = signed_imm;
            return inst;
        }

        // B.cond (Conditional Branch): 0101 0100 iiii iiii iiii iiii iii0 cccc
        if (raw & 0xff000010) == 0x54000000 {
            let cond = ConditionCode::from_u8((raw & 0xf) as u8);
            let raw_imm = ((raw >> 5) & 0x7ffff) as i32;
            let signed_imm = if (raw_imm & 0x40000) != 0 {
                (raw_imm | !0x7ffff) as i64 * 4
            } else {
                raw_imm as i64 * 4
            };

            let mut inst = Arm64Inst::new(Arm64Op::Bcc(cond), raw);
            inst.imm = signed_imm;
            return inst;
        }

        // CBZ / CBNZ: sf 011 0100 ...
        if (raw & 0x7e000000) == 0x34000000 {
            let is_64 = (raw & 0x80000000) != 0;
            let is_cbnz = (raw & 0x01000000) != 0;
            let rt = (raw & 0x1f) as u8;
            let raw_imm = ((raw >> 5) & 0x7ffff) as i32;
            let signed_imm = if (raw_imm & 0x40000) != 0 {
                (raw_imm | !0x7ffff) as i64 * 4
            } else {
                raw_imm as i64 * 4
            };

            let op = if is_cbnz { Arm64Op::Cbnz } else { Arm64Op::Cbz };
            let mut inst = Arm64Inst::new(op, raw);
            inst.rd = rt;
            inst.imm = signed_imm;
            inst.is_64bit = is_64;
            return inst;
        }

        // MOVZ / MOVK / MOVN: sf 101 0010 1hw iiii iiii iiii iiii rrrr r
        if (raw & 0x7f800000) == 0x12800000
            || (raw & 0x7f800000) == 0x52800000
            || (raw & 0x7f800000) == 0x72800000
        {
            let is_64 = (raw & 0x80000000) != 0;
            let opc = (raw >> 29) & 0x3;
            let hw = ((raw >> 21) & 0x3) as u8;
            let imm16 = ((raw >> 5) & 0xffff) as i64;
            let rd = (raw & 0x1f) as u8;

            let op = match opc {
                0 => Arm64Op::Movn,
                2 => Arm64Op::Movz,
                3 => Arm64Op::Movk,
                _ => Arm64Op::Mov,
            };

            let mut inst = Arm64Inst::new(op, raw);
            inst.rd = rd;
            inst.imm = imm16;
            inst.shift = hw * 16;
            inst.is_64bit = is_64;
            return inst;
        }

        // ADD / SUB (Immediate): sf op S 1000 1000 sh iiii iiii iiii rrrr rdddd d
        if (raw & 0x1f000000) == 0x11000000 {
            let is_64 = (raw & 0x80000000) != 0;
            let is_sub = (raw & 0x40000000) != 0;
            let set_flags = (raw & 0x20000000) != 0;
            let shift_flag = ((raw >> 22) & 1) as u8;
            let mut imm12 = ((raw >> 10) & 0xfff) as i64;
            if shift_flag == 1 {
                imm12 <<= 12;
            }
            let rn = ((raw >> 5) & 0x1f) as u8;
            let rd = (raw & 0x1f) as u8;

            let op = match (is_sub, set_flags, rd == 31) {
                (false, false, _) => Arm64Op::Add,
                (false, true, true) => Arm64Op::Cmn,
                (false, true, false) => Arm64Op::Adds,
                (true, false, _) => Arm64Op::Sub,
                (true, true, true) => Arm64Op::Cmp,
                (true, true, false) => Arm64Op::Subs,
            };

            let mut inst = Arm64Inst::new(op, raw);
            inst.rd = rd;
            inst.rn = rn;
            inst.imm = imm12;
            inst.is_64bit = is_64;
            return inst;
        }

        // ADD / SUB (Shifted Register): sf op S 0101 1000 ...
        if (raw & 0x1f000000) == 0x0b000000 {
            let is_64 = (raw & 0x80000000) != 0;
            let is_sub = (raw & 0x40000000) != 0;
            let set_flags = (raw & 0x20000000) != 0;
            let rm = ((raw >> 16) & 0x1f) as u8;
            let imm6 = ((raw >> 10) & 0x3f) as u8;
            let rn = ((raw >> 5) & 0x1f) as u8;
            let rd = (raw & 0x1f) as u8;

            let op = match (is_sub, set_flags, rd == 31) {
                (false, false, _) => Arm64Op::Add,
                (false, true, true) => Arm64Op::Cmn,
                (false, true, false) => Arm64Op::Adds,
                (true, false, _) => Arm64Op::Sub,
                (true, true, true) => Arm64Op::Cmp,
                (true, true, false) => Arm64Op::Subs,
            };

            let mut inst = Arm64Inst::new(op, raw);
            inst.rd = rd;
            inst.rn = rn;
            inst.rm = rm;
            inst.shift = imm6;
            inst.is_64bit = is_64;
            return inst;
        }

        // Logical (Shifted Register: AND, ORR, EOR, ANDS): sf 00/01 0101 0...
        if (raw & 0x1f000000) == 0x0a000000 {
            let is_64 = (raw & 0x80000000) != 0;
            let opc = (raw >> 29) & 0x3;
            let rm = ((raw >> 16) & 0x1f) as u8;
            let rn = ((raw >> 5) & 0x1f) as u8;
            let rd = (raw & 0x1f) as u8;

            let op = match opc {
                0 => Arm64Op::And,
                1 => Arm64Op::Orr,
                2 => Arm64Op::Eor,
                _ => Arm64Op::Ands,
            };

            let mut inst = Arm64Inst::new(op, raw);
            inst.rd = rd;
            inst.rn = rn;
            inst.rm = rm;
            inst.is_64bit = is_64;
            return inst;
        }

        // LDR / STR (Unscaled Immediate / Signed Offset): size 111 000 ...
        if (raw & 0x3b200c00) == 0x38000000 || (raw & 0x3b200c00) == 0x38200000 {
            let size = (raw >> 30) & 0x3;
            let is_load = ((raw >> 22) & 1) != 0;
            let rn = ((raw >> 5) & 0x1f) as u8;
            let rt = (raw & 0x1f) as u8;
            let raw_imm = ((raw >> 12) & 0x1ff) as i32;
            let signed_imm = if (raw_imm & 0x100) != 0 {
                (raw_imm | !0x1ff) as i64
            } else {
                raw_imm as i64
            };

            let op = match (is_load, size) {
                (true, 0) => Arm64Op::Ldrb,
                (false, 0) => Arm64Op::Strb,
                (true, 1) => Arm64Op::Ldrh,
                (false, 1) => Arm64Op::Strh,
                (true, _) => Arm64Op::Ldr,
                (false, _) => Arm64Op::Str,
            };

            let mut inst = Arm64Inst::new(op, raw);
            inst.rd = rt;
            inst.rn = rn;
            inst.imm = signed_imm;
            inst.is_64bit = size == 3;
            return inst;
        }

        // LDP / STP (Load/Store Pair): opc 101 0 ...
        if (raw & 0x3f000000) == 0x28000000 || (raw & 0x3f000000) == 0x29000000 {
            let is_load = (raw & 0x00400000) != 0;
            let is_64 = (raw & 0x80000000) != 0;
            let rt2 = ((raw >> 10) & 0x1f) as u8;
            let rn = ((raw >> 5) & 0x1f) as u8;
            let rt = (raw & 0x1f) as u8;
            let raw_imm = ((raw >> 15) & 0x7f) as i32;
            let scale = if is_64 { 8 } else { 4 };
            let signed_imm = if (raw_imm & 0x40) != 0 {
                (raw_imm | !0x7f) as i64 * scale
            } else {
                raw_imm as i64 * scale
            };

            let op = if is_load { Arm64Op::Ldp } else { Arm64Op::Stp };
            let mut inst = Arm64Inst::new(op, raw);
            inst.rd = rt;
            inst.rm = rt2;
            inst.rn = rn;
            inst.imm = signed_imm;
            inst.is_64bit = is_64;
            return inst;
        }

        // NEON Vector Add / Sub / Mul (SIMD Three Same): 0 Q U 01110 size 1 Rm ...
        if (raw & 0xbf200400) == 0x0e200400 {
            let is_sub = (raw & 0x00800000) != 0;
            let rd = (raw & 0x1f) as u8;
            let rn = ((raw >> 5) & 0x1f) as u8;
            let rm = ((raw >> 16) & 0x1f) as u8;

            let op = if is_sub { Arm64Op::Vsub } else { Arm64Op::Vadd };
            let mut inst = Arm64Inst::new(op, raw);
            inst.rd = rd;
            inst.rn = rn;
            inst.rm = rm;
            return inst;
        }

        // Unknown / Fallback
        Arm64Inst::new(Arm64Op::Unknown(raw), raw)
    }
}

impl fmt::Display for Arm64Inst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.op {
            Arm64Op::Nop => write!(f, "nop"),
            Arm64Op::Ret => write!(f, "ret x{}", self.rn),
            Arm64Op::Svc => write!(f, "svc #0x{:x}", self.imm),
            Arm64Op::Brk => write!(f, "brk #0x{:x}", self.imm),
            Arm64Op::Dmb => write!(f, "dmb #0x{:x}", self.imm),
            Arm64Op::Dsb => write!(f, "dsb #0x{:x}", self.imm),
            Arm64Op::Isb => write!(f, "isb"),
            Arm64Op::B => write!(f, "b 0x{:x}", self.imm),
            Arm64Op::Bl => write!(f, "bl 0x{:x}", self.imm),
            Arm64Op::Bcc(cond) => write!(f, "b.{:?} 0x{:x}", cond, self.imm),
            Arm64Op::Cbz => write!(f, "cbz x{}, 0x{:x}", self.rd, self.imm),
            Arm64Op::Cbnz => write!(f, "cbnz x{}, 0x{:x}", self.rd, self.imm),
            Arm64Op::Add => write!(f, "add x{}, x{}, #{}", self.rd, self.rn, self.imm),
            Arm64Op::Sub => write!(f, "sub x{}, x{}, #{}", self.rd, self.rn, self.imm),
            Arm64Op::Cmp => write!(f, "cmp x{}, #{}", self.rn, self.imm),
            Arm64Op::Movz | Arm64Op::Movk | Arm64Op::Mov => {
                write!(f, "mov x{}, #0x{:x}", self.rd, self.imm)
            }
            Arm64Op::Ldr => write!(f, "ldr x{}, [x{}, #{}]", self.rd, self.rn, self.imm),
            Arm64Op::Str => write!(f, "str x{}, [x{}, #{}]", self.rd, self.rn, self.imm),
            Arm64Op::Vadd => write!(f, "vadd.16b v{}, v{}, v{}", self.rd, self.rn, self.rm),
            Arm64Op::Vsub => write!(f, "vsub.16b v{}, v{}, v{}", self.rd, self.rn, self.rm),
            Arm64Op::Unknown(raw) => write!(f, ".word 0x{:08x}", raw),
            _ => write!(f, "{:?} (raw: 0x{:08x})", self.op, self.raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_basic_instructions() {
        // NOP
        let nop = Arm64Decoder::decode(0xd503201f);
        assert_eq!(nop.op, Arm64Op::Nop);

        // RET (x30)
        let ret = Arm64Decoder::decode(0xd65f03c0);
        assert_eq!(ret.op, Arm64Op::Ret);
        assert_eq!(ret.rn, 30);

        // SVC #0
        let svc = Arm64Decoder::decode(0xd4000001);
        assert_eq!(svc.op, Arm64Op::Svc);
        assert_eq!(svc.imm, 0);

        // DMB ISH (0xd5033bbf)
        let dmb = Arm64Decoder::decode(0xd5033bbf);
        assert_eq!(dmb.op, Arm64Op::Dmb);
    }

    #[test]
    fn test_decode_arithmetic_and_branch() {
        // ADD x0, x1, #42 -> 0x9100a820
        let add = Arm64Decoder::decode(0x9100a820);
        assert_eq!(add.op, Arm64Op::Add);
        assert_eq!(add.rd, 0);
        assert_eq!(add.rn, 1);
        assert_eq!(add.imm, 42);

        // B #+16 -> 0x14000004
        let b = Arm64Decoder::decode(0x14000004);
        assert_eq!(b.op, Arm64Op::B);
        assert_eq!(b.imm, 16);
    }
}
