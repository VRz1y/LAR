//! Unit Tests for 64-bit ARMv8-A Instruction Decoder.

use lar::jit::decoder::*;

#[test]
fn test_decode_alu_immediate_and_register() {
    // 1. ADD x0, x1, #42 (0x9100a820)
    let add_imm = Arm64Decoder::decode(0x9100a820);
    assert_eq!(add_imm.op, Arm64Op::Add);
    assert_eq!(add_imm.rd, 0);
    assert_eq!(add_imm.rn, 1);
    assert_eq!(add_imm.imm, 42);
    assert!(add_imm.is_64bit);

    // 2. SUB w2, w3, #15 (0x51003c62)
    let sub_imm = Arm64Decoder::decode(0x51003c62);
    assert_eq!(sub_imm.op, Arm64Op::Sub);
    assert_eq!(sub_imm.rd, 2);
    assert_eq!(sub_imm.rn, 3);
    assert_eq!(sub_imm.imm, 15);
    assert!(!sub_imm.is_64bit);

    // 3. CMP x4, #100 (0xf101909f) -> SUBS xzr, x4, #100
    let cmp = Arm64Decoder::decode(0xf101909f);
    assert_eq!(cmp.op, Arm64Op::Cmp);
    assert_eq!(cmp.rn, 4);
    assert_eq!(cmp.imm, 100);

    // 4. ADD x0, x1, x2 (0x8b020020)
    let add_reg = Arm64Decoder::decode(0x8b020020);
    assert_eq!(add_reg.op, Arm64Op::Add);
    assert_eq!(add_reg.rd, 0);
    assert_eq!(add_reg.rn, 1);
    assert_eq!(add_reg.rm, 2);
}

#[test]
fn test_decode_moves_and_logicals() {
    // 1. MOVZ x0, #0x1234 (0xd2824680)
    let movz = Arm64Decoder::decode(0xd2824680);
    assert_eq!(movz.op, Arm64Op::Movz);
    assert_eq!(movz.rd, 0);
    assert_eq!(movz.imm, 0x1234);

    // 2. MOVK x0, #0x5678, LSL #16 (0xf2aacf00)
    let movk = Arm64Decoder::decode(0xf2aacf00);
    assert_eq!(movk.op, Arm64Op::Movk);
    assert_eq!(movk.rd, 0);
    assert_eq!(movk.imm, 0x5678);
    assert_eq!(movk.shift, 16);

    // 3. AND x0, x1, x2 (0x8a020020)
    let and_reg = Arm64Decoder::decode(0x8a020020);
    assert_eq!(and_reg.op, Arm64Op::And);

    // 4. ORR x0, x1, x2 (0xaa020020)
    let orr_reg = Arm64Decoder::decode(0xaa020020);
    assert_eq!(orr_reg.op, Arm64Op::Orr);

    // 5. EOR x0, x1, x2 (0xca020020)
    let eor_reg = Arm64Decoder::decode(0xca020020);
    assert_eq!(eor_reg.op, Arm64Op::Eor);
}

#[test]
fn test_decode_branching_instructions() {
    // 1. B #+1024 -> 0x14000100
    let b = Arm64Decoder::decode(0x14000100);
    assert_eq!(b.op, Arm64Op::B);
    assert_eq!(b.imm, 1024);

    // 2. BL #+2048 -> 0x94000200
    let bl = Arm64Decoder::decode(0x94000200);
    assert_eq!(bl.op, Arm64Op::Bl);
    assert_eq!(bl.imm, 2048);

    // 3. B.EQ #+16 -> 0x54000080
    let b_eq = Arm64Decoder::decode(0x54000080);
    assert_eq!(b_eq.op, Arm64Op::Bcc(ConditionCode::EQ));
    assert_eq!(b_eq.imm, 16);

    // 4. B.NE #+32 -> 0x54000101
    let b_ne = Arm64Decoder::decode(0x54000101);
    assert_eq!(b_ne.op, Arm64Op::Bcc(ConditionCode::NE));
    assert_eq!(b_ne.imm, 32);

    // 5. CBZ x0, #+64 -> 0xb4000200
    let cbz = Arm64Decoder::decode(0xb4000200);
    assert_eq!(cbz.op, Arm64Op::Cbz);
    assert_eq!(cbz.rd, 0);
    assert_eq!(cbz.imm, 64);
}

#[test]
fn test_decode_barriers_and_system() {
    // DMB ISH -> 0xd5033bbf
    let dmb = Arm64Decoder::decode(0xd5033bbf);
    assert_eq!(dmb.op, Arm64Op::Dmb);

    // DSB ISH -> 0xd50339bf
    let dsb = Arm64Decoder::decode(0xd50339bf);
    assert_eq!(dsb.op, Arm64Op::Dsb);

    // ISB -> 0xd5033fdf
    let isb = Arm64Decoder::decode(0xd5033fdf);
    assert_eq!(isb.op, Arm64Op::Isb);

    // NOP -> 0xd503201f
    let nop = Arm64Decoder::decode(0xd503201f);
    assert_eq!(nop.op, Arm64Op::Nop);

    // RET x30 -> 0xd65f03c0
    let ret = Arm64Decoder::decode(0xd65f03c0);
    assert_eq!(ret.op, Arm64Op::Ret);
    assert_eq!(ret.rn, 30);
}
