use lar::arch::Arm64ContextTrampoline;
use lar::graphics::{GraphicsRuntime, OverlayRect, bezier_trajectory, trajectory_touch_events};
use lar::jit::{
    IrBlock, IrHook, IrHookError, IrInstruction, IrOpcode, IrOperand, IrReg, RiscvBackend,
};
use lar::lifecycle::InputEvent;

struct ReplaceImmediate;
impl IrHook for ReplaceImmediate {
    fn apply(&self, block: &mut IrBlock) -> Result<(), IrHookError> {
        block.instructions[0] =
            IrInstruction::new(IrOpcode::Mov, Some(IrReg::X(0)), IrOperand::Imm(42), None);
        Ok(())
    }
}

#[test]
fn phase4_arm64_trampoline_is_wx_and_contains_handler_literal() {
    let trampoline = Arm64ContextTrampoline::emit_address(0x1234_5678_9abc_def0).unwrap();
    assert_eq!(trampoline.protection().0, libc::PROT_READ | libc::PROT_EXEC);
    assert_eq!(trampoline.code_len(), 32);
    let code = Arm64ContextTrampoline::machine_code(0x1234_5678_9abc_def0);
    assert_eq!(&code[24..], 0x1234_5678_9abc_def0u64.to_le_bytes());
    assert_eq!(&code[0..4], &0xa9bf7bfdu32.to_le_bytes());
    assert_eq!(&code[12..16], &0xa8c17bfdu32.to_le_bytes());
}

#[test]
fn phase4_riscv_lowering_applies_ir_hook_before_emission() {
    let mut block = IrBlock::new(0x1000);
    block.push(IrInstruction::new(
        IrOpcode::Nop,
        None,
        IrOperand::Imm(0),
        None,
    ));
    let code = RiscvBackend::lower_with_hook(&mut block, &ReplaceImmediate).unwrap();
    assert_eq!(code.len(), 1);
    assert_ne!(code[0], 0x00000013);
}

#[test]
fn phase4_graphics_facade_and_deterministic_bezier_trajectory() {
    let mut runtime = GraphicsRuntime::new();
    runtime
        .overlay
        .add(OverlayRect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
            color: [255, 0, 0, 128],
        })
        .unwrap();
    assert!(!runtime.virtual_touch_supported());
    let touch = runtime.configure_virtual_touch(100, 100);
    if touch.is_ok() {
        assert!(runtime.virtual_touch_supported());
        runtime
            .emit_virtual_touch(InputEvent::Touch {
                x: 1,
                y: 2,
                pressed: true,
            })
            .unwrap();
    }

    let first = bezier_trajectory((0.0, 0.0), (1.0, 4.0), (8.0, 4.0), (10.0, 0.0), 8, 7, 0.25);
    let second = bezier_trajectory((0.0, 0.0), (1.0, 4.0), (8.0, 4.0), (10.0, 0.0), 8, 7, 0.25);
    assert_eq!(first, second);
    assert_eq!(first.first().unwrap().x, 0.0);
    assert_eq!(first.last().unwrap().x, 10.0);
    assert!(matches!(
        trajectory_touch_events(&first, true).last().unwrap(),
        InputEvent::Touch { pressed: false, .. }
    ));
}
