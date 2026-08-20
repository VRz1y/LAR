//! Signal Dispatcher and Guest Context Translator for LAR.
//!
//! Intercepts host signals (`SIGSEGV`, `SIGILL`, `SIGBUS`, `SIGFPE`), translates
//! host machine context into guest `Arm64CpuContext`, and routes to guest signal handlers.

use crate::arch::context::Arm64CpuContext;
use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;

/// Guest signal numbers (matching Linux ARM64).
pub const GUEST_SIGHUP: i32 = 1;
pub const GUEST_SIGINT: i32 = 2;
pub const GUEST_SIGQUIT: i32 = 3;
pub const GUEST_SIGILL: i32 = 4;
pub const GUEST_SIGTRAP: i32 = 5;
pub const GUEST_SIGABRT: i32 = 6;
pub const GUEST_SIGBUS: i32 = 7;
pub const GUEST_SIGFPE: i32 = 8;
pub const GUEST_SIGKILL: i32 = 9;
pub const GUEST_SIGUSR1: i32 = 10;
pub const GUEST_SIGSEGV: i32 = 11;
pub const GUEST_SIGUSR2: i32 = 12;
pub const GUEST_SIGPIPE: i32 = 13;
pub const GUEST_SIGALRM: i32 = 14;
pub const GUEST_SIGTERM: i32 = 15;

/// Guest signal action definition (matches Linux `struct sigaction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestSigAction {
    pub handler: usize,
    pub flags: u64,
    pub restorer: usize,
    pub mask: u64,
}

impl Default for GuestSigAction {
    fn default() -> Self {
        Self {
            handler: 0,
            flags: 0,
            restorer: 0,
            mask: 0,
        }
    }
}

/// Signal Dispatcher managing guest signal handlers and fault recovery.
pub struct SignalDispatcher {
    handlers: RwLock<HashMap<i32, GuestSigAction>>,
}

impl Default for SignalDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalDispatcher {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a guest signal handler for a signal.
    pub fn register_handler(&self, signum: i32, action: GuestSigAction) {
        let mut handlers = self.handlers.write().unwrap();
        handlers.insert(signum, action);
    }

    /// Retrieves registered handler for a signal.
    pub fn get_handler(&self, signum: i32) -> Option<GuestSigAction> {
        let handlers = self.handlers.read().unwrap();
        handlers.get(&signum).copied()
    }

    /// Dispatches a signal to a guest handler if registered, setting up the guest context.
    pub fn dispatch_to_guest(
        &self,
        signum: i32,
        ctx: &mut Arm64CpuContext,
        fault_addr: u64,
    ) -> bool {
        if let Some(action) = self.get_handler(signum) {
            if action.handler != 0 && action.handler != usize::MAX {
                // Prepare guest context for signal handler invocation:
                // x0: signum
                // x1: siginfo_t ptr (or fault addr)
                // x2: ucontext_t ptr
                ctx.set_arg(0, signum as u64);
                ctx.set_arg(1, fault_addr);
                ctx.pc = action.handler as u64;
                return true;
            }
        }
        false
    }
}

impl fmt::Debug for SignalDispatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignalDispatcher")
            .field("registered_signals", &self.handlers.read().unwrap().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_registration_and_dispatch() {
        let dispatcher = SignalDispatcher::new();
        let action = GuestSigAction {
            handler: 0x0040_1000,
            flags: 0,
            restorer: 0,
            mask: 0,
        };

        dispatcher.register_handler(GUEST_SIGSEGV, action);
        assert_eq!(dispatcher.get_handler(GUEST_SIGSEGV), Some(action));

        let mut ctx = Arm64CpuContext::new();
        let handled = dispatcher.dispatch_to_guest(GUEST_SIGSEGV, &mut ctx, 0xdead_beef);
        assert!(handled);
        assert_eq!(ctx.get_arg(0), GUEST_SIGSEGV as u64);
        assert_eq!(ctx.get_arg(1), 0xdead_beef);
        assert_eq!(ctx.pc, 0x0040_1000);
    }
}
