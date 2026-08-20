//! ARM64 CPU Register Context and Execution State.
//!
//! Models 64-bit ARMv8-A general purpose registers (x0-x30), stack pointer (sp),
//! program counter (pc), processor state flags (pstate), and 128-bit NEON/FP registers (v0-v31).

use std::fmt;

/// 64-bit ARMv8-A CPU Register Context.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Arm64CpuContext {
    /// General purpose registers x0 through x30.
    /// x29 is Frame Pointer (FP), x30 is Link Register (LR).
    pub regs: [u64; 31],
    /// Stack Pointer.
    pub sp: u64,
    /// Program Counter.
    pub pc: u64,
    /// Processor State (NZCV flags + mode bits).
    pub pstate: u64,
    /// Vector / Floating-Point registers v0 through v31 (each 128-bit = 16 bytes).
    pub vregs: [[u8; 16]; 32],
}

impl Default for Arm64CpuContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Arm64CpuContext {
    /// Creates a zero-initialized ARM64 CPU context.
    pub const fn new() -> Self {
        Self {
            regs: [0; 31],
            sp: 0,
            pc: 0,
            pstate: 0,
            vregs: [[0; 16]; 32],
        }
    }

    /// Gets an integer argument register (x0..x7).
    #[inline]
    pub fn get_arg(&self, index: usize) -> u64 {
        if index < 8 { self.regs[index] } else { 0 }
    }

    /// Sets an integer argument register (x0..x7).
    #[inline]
    pub fn set_arg(&mut self, index: usize, val: u64) {
        if index < 8 {
            self.regs[index] = val;
        }
    }

    /// Gets return value (x0).
    #[inline]
    pub fn get_return(&self) -> u64 {
        self.regs[0]
    }

    /// Sets return value (x0).
    #[inline]
    pub fn set_return(&mut self, val: u64) {
        self.regs[0] = val;
    }

    /// Gets secondary return value (x1) for 128-bit scalar returns.
    #[inline]
    pub fn get_return_secondary(&self) -> u64 {
        self.regs[1]
    }

    /// Sets secondary return value (x1).
    #[inline]
    pub fn set_return_secondary(&mut self, val: u64) {
        self.regs[1] = val;
    }

    /// Gets Link Register (x30 / LR).
    #[inline]
    pub fn lr(&self) -> u64 {
        self.regs[30]
    }

    /// Sets Link Register (x30 / LR).
    #[inline]
    pub fn set_lr(&mut self, val: u64) {
        self.regs[30] = val;
    }

    /// Gets Frame Pointer (x29 / FP).
    #[inline]
    pub fn fp(&self) -> u64 {
        self.regs[29]
    }

    /// Sets Frame Pointer (x29 / FP).
    #[inline]
    pub fn set_fp(&mut self, val: u64) {
        self.regs[29] = val;
    }

    /// Reads a 64-bit float from vector register (d0..d31).
    #[inline]
    pub fn get_dreg(&self, index: usize) -> f64 {
        if index < 32 {
            let bytes: [u8; 8] = self.vregs[index][0..8].try_into().unwrap();
            f64::from_le_bytes(bytes)
        } else {
            0.0
        }
    }

    /// Sets a 64-bit float into vector register (d0..d31).
    #[inline]
    pub fn set_dreg(&mut self, index: usize, val: f64) {
        if index < 32 {
            self.vregs[index][0..8].copy_from_slice(&val.to_le_bytes());
            self.vregs[index][8..16].fill(0);
        }
    }

    /// Reads a 32-bit float from vector register (s0..s31).
    #[inline]
    pub fn get_sreg(&self, index: usize) -> f32 {
        if index < 32 {
            let bytes: [u8; 4] = self.vregs[index][0..4].try_into().unwrap();
            f32::from_le_bytes(bytes)
        } else {
            0.0
        }
    }

    /// Sets a 32-bit float into vector register (s0..s31).
    #[inline]
    pub fn set_sreg(&mut self, index: usize, val: f32) {
        if index < 32 {
            self.vregs[index][0..4].copy_from_slice(&val.to_le_bytes());
            self.vregs[index][4..16].fill(0);
        }
    }

    /// Checks if Negative (N) flag is set.
    #[inline]
    pub const fn flag_n(&self) -> bool {
        (self.pstate & (1 << 31)) != 0
    }

    /// Checks if Zero (Z) flag is set.
    #[inline]
    pub const fn flag_z(&self) -> bool {
        (self.pstate & (1 << 30)) != 0
    }

    /// Checks if Carry (C) flag is set.
    #[inline]
    pub const fn flag_c(&self) -> bool {
        (self.pstate & (1 << 29)) != 0
    }

    /// Checks if Overflow (V) flag is set.
    #[inline]
    pub const fn flag_v(&self) -> bool {
        (self.pstate & (1 << 28)) != 0
    }

    /// Sets NZCV condition flags.
    pub fn set_flags(&mut self, n: bool, z: bool, c: bool, v: bool) {
        let mut mask = 0u64;
        if n {
            mask |= 1 << 31;
        }
        if z {
            mask |= 1 << 30;
        }
        if c {
            mask |= 1 << 29;
        }
        if v {
            mask |= 1 << 28;
        }
        self.pstate = (self.pstate & !(0xF << 28)) | mask;
    }
}

impl fmt::Debug for Arm64CpuContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Arm64CpuContext:")?;
        for i in 0..8 {
            writeln!(
                f,
                "  x{:02}: 0x{:016x}   x{:02}: 0x{:016x}   x{:02}: 0x{:016x}   x{:02}: 0x{:016x}",
                i,
                self.regs[i],
                i + 8,
                self.regs[i + 8],
                i + 16,
                self.regs[i + 16],
                i + 24,
                if i + 24 < 31 { self.regs[i + 24] } else { 0 }
            )?;
        }
        writeln!(
            f,
            "  sp:  0x{:016x}   pc: 0x{:016x}   pstate: 0x{:08x} [N:{} Z:{} C:{} V:{}]",
            self.sp,
            self.pc,
            self.pstate,
            self.flag_n() as u8,
            self.flag_z() as u8,
            self.flag_c() as u8,
            self.flag_v() as u8
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arm64_context_args_and_returns() {
        let mut ctx = Arm64CpuContext::new();
        ctx.set_arg(0, 0x1111);
        ctx.set_arg(1, 0x2222);
        ctx.set_arg(7, 0x8888);

        assert_eq!(ctx.get_arg(0), 0x1111);
        assert_eq!(ctx.get_arg(1), 0x2222);
        assert_eq!(ctx.get_arg(7), 0x8888);

        ctx.set_return(42);
        assert_eq!(ctx.get_return(), 42);
        assert_eq!(ctx.regs[0], 42);
    }

    #[test]
    fn test_arm64_context_float_and_flags() {
        let mut ctx = Arm64CpuContext::new();
        ctx.set_dreg(0, std::f64::consts::PI);
        assert!((ctx.get_dreg(0) - std::f64::consts::PI).abs() < 1e-9);

        ctx.set_sreg(1, std::f32::consts::E);
        assert!((ctx.get_sreg(1) - std::f32::consts::E).abs() < 1e-5);

        ctx.set_flags(true, false, true, false);
        assert!(ctx.flag_n());
        assert!(!ctx.flag_z());
        assert!(ctx.flag_c());
        assert!(!ctx.flag_v());
    }
}
