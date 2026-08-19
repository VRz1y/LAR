//! Unit Tests for Signal Dispatcher, CPU Register Context, and Call Bridges.

use lar::arch::*;
use lar::signal::*;

#[test]
fn test_arm64_register_state() {
    let mut ctx = Arm64CpuContext::new();

    // Verify all 31 general purpose registers
    for i in 0..31 {
        ctx.regs[i] = 0x1000 + i as u64;
    }
    for i in 0..31 {
        assert_eq!(ctx.regs[i], 0x1000 + i as u64);
    }

    // Stack pointer & program counter
    ctx.sp = 0x0000_7fff_ffff_0000;
    ctx.pc = 0x0000_0000_0040_0000;
    assert_eq!(ctx.sp, 0x0000_7fff_ffff_0000);
    assert_eq!(ctx.pc, 0x0000_0000_0040_0000);

    // Condition flags
    ctx.set_flags(true, true, false, false);
    assert!(ctx.flag_n());
    assert!(ctx.flag_z());
    assert!(!ctx.flag_c());
    assert!(!ctx.flag_v());

    ctx.set_flags(false, false, true, true);
    assert!(!ctx.flag_n());
    assert!(!ctx.flag_z());
    assert!(ctx.flag_c());
    assert!(ctx.flag_v());
}

#[test]
fn test_signal_guest_delivery() {
    let dispatcher = SignalDispatcher::new();
    let segv_action = GuestSigAction {
        handler: 0x0040_5000,
        flags: 0,
        restorer: 0,
        mask: 0,
    };
    let ill_action = GuestSigAction {
        handler: 0x0040_6000,
        flags: 0,
        restorer: 0,
        mask: 0,
    };

    dispatcher.register_handler(GUEST_SIGSEGV, segv_action);
    dispatcher.register_handler(GUEST_SIGILL, ill_action);

    let mut ctx = Arm64CpuContext::new();
    let handled_segv = dispatcher.dispatch_to_guest(GUEST_SIGSEGV, &mut ctx, 0x0000_0000_bad0_0000);
    assert!(handled_segv);
    assert_eq!(ctx.pc, 0x0040_5000);
    assert_eq!(ctx.get_arg(0), GUEST_SIGSEGV as u64);
    assert_eq!(ctx.get_arg(1), 0x0000_0000_bad0_0000);

    let handled_ill = dispatcher.dispatch_to_guest(GUEST_SIGILL, &mut ctx, 0x0040_1234);
    assert!(handled_ill);
    assert_eq!(ctx.pc, 0x0040_6000);
    assert_eq!(ctx.get_arg(0), GUEST_SIGILL as u64);
    assert_eq!(ctx.get_arg(1), 0x0040_1234);

    let handled_unregistered = dispatcher.dispatch_to_guest(GUEST_SIGFPE, &mut ctx, 0);
    assert!(!handled_unregistered);
}

#[test]
fn test_call_bridge_marshaling() {
    extern "C" fn sum6(a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> usize {
        a + b + c + d + e + f
    }

    let bridge = CallBridge::from_c_fn(sum6 as *const (), 6);
    let mut ctx = Arm64CpuContext::new();
    ctx.set_arg(0, 10);
    ctx.set_arg(1, 20);
    ctx.set_arg(2, 30);
    ctx.set_arg(3, 40);
    ctx.set_arg(4, 50);
    ctx.set_arg(5, 60);

    bridge.invoke(&mut ctx);
    assert_eq!(ctx.get_return(), 210);
}
