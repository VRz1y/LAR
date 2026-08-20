//! Signal Dispatcher and Guest Context Translator for LAR.
//!
//! Intercepts host signals (`SIGSEGV`, `SIGILL`, `SIGBUS`, `SIGFPE`), translates
//! host machine context into guest `Arm64CpuContext`, and routes to guest signal handlers.

use crate::arch::context::Arm64CpuContext;
use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Host-side signals supported by the opt-in signal bridge.
pub const HOST_FAULT_SIGNALS: [i32; 4] = [libc::SIGSEGV, libc::SIGILL, libc::SIGBUS, libc::SIGFPE];

/// Async-signal-safe router invoked by the host signal trampoline.
pub type HostSignalRouter = extern "C" fn(i32, *mut libc::siginfo_t, *mut libc::c_void);

/// Failure to enable the process-global host signal bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSignalBridgeError {
    UnsupportedPlatform,
    AlreadyInstalled,
    SigactionFailed { signal: i32, errno: i32 },
}

impl fmt::Display for HostSignalBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                write!(f, "host signal bridge is unsupported on this platform")
            }
            Self::AlreadyInstalled => write!(f, "host signal bridge is already installed"),
            Self::SigactionFailed { signal, errno } => {
                write!(f, "sigaction failed for signal {signal} with errno {errno}")
            }
        }
    }
}

impl std::error::Error for HostSignalBridgeError {}

/// Restores the signal actions which preceded bridge installation when dropped.
pub struct HostSignalBridge {
    #[cfg(target_os = "linux")]
    previous: [(i32, libc::sigaction); 4],
}

#[cfg(target_os = "linux")]
static HOST_BRIDGE_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "linux")]
static HOST_SIGNAL_ROUTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(target_os = "linux")]
extern "C" fn host_signal_trampoline(
    signal: i32,
    info: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    let router = HOST_SIGNAL_ROUTER.load(Ordering::Relaxed);
    if router != 0 {
        let router: HostSignalRouter = unsafe { std::mem::transmute(router) };
        router(signal, info, context);
    }
}

#[cfg(target_os = "linux")]
impl Drop for HostSignalBridge {
    fn drop(&mut self) {
        HOST_SIGNAL_ROUTER.store(0, Ordering::Release);
        for (signal, action) in self.previous.iter().rev() {
            unsafe {
                libc::sigaction(*signal, action, std::ptr::null_mut());
            }
        }
        HOST_BRIDGE_INSTALLED.store(false, Ordering::Release);
    }
}

#[cfg(not(target_os = "linux"))]
impl Drop for HostSignalBridge {
    fn drop(&mut self) {}
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GuestSigAction {
    pub handler: usize,
    pub flags: u64,
    pub restorer: usize,
    pub mask: u64,
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

    /// Installs the process-global host fault bridge.
    ///
    /// The router must only perform async-signal-safe operations. The caller must
    /// ensure that no host fault signal is concurrently executing while the
    /// returned bridge is dropped.
    ///
    /// # Safety
    /// `router` must remain valid while the bridge is installed and must only
    /// perform operations that are safe from a signal handler.
    #[cfg(target_os = "linux")]
    pub unsafe fn install_host_signal_bridge(
        &self,
        router: HostSignalRouter,
    ) -> Result<HostSignalBridge, HostSignalBridgeError> {
        if HOST_BRIDGE_INSTALLED
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(HostSignalBridgeError::AlreadyInstalled);
        }

        HOST_SIGNAL_ROUTER.store(router as usize, Ordering::Release);
        let mut previous: [(i32, libc::sigaction); 4] = unsafe { std::mem::zeroed() };
        for (index, signal) in HOST_FAULT_SIGNALS.iter().copied().enumerate() {
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            action.sa_sigaction = host_signal_trampoline as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO;
            unsafe { libc::sigemptyset(&mut action.sa_mask) };
            if unsafe { libc::sigaction(signal, &action, &mut previous[index].1) } != 0 {
                let errno = std::io::Error::last_os_error()
                    .raw_os_error()
                    .unwrap_or(libc::EINVAL);
                for (restored_signal, restored_action) in previous[..index].iter().rev() {
                    unsafe {
                        libc::sigaction(*restored_signal, restored_action, std::ptr::null_mut());
                    }
                }
                HOST_SIGNAL_ROUTER.store(0, Ordering::Release);
                HOST_BRIDGE_INSTALLED.store(false, Ordering::Release);
                return Err(HostSignalBridgeError::SigactionFailed { signal, errno });
            }
            previous[index].0 = signal;
        }
        Ok(HostSignalBridge { previous })
    }

    #[cfg(not(target_os = "linux"))]
    pub unsafe fn install_host_signal_bridge(
        &self,
        _router: HostSignalRouter,
    ) -> Result<HostSignalBridge, HostSignalBridgeError> {
        Err(HostSignalBridgeError::UnsupportedPlatform)
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
        if let Some(action) = self.get_handler(signum)
            && action.handler != 0
            && action.handler != usize::MAX
        {
            // Prepare guest context for signal handler invocation:
            // x0: signum
            // x1: siginfo_t ptr (or fault addr)
            // x2: ucontext_t ptr
            ctx.set_arg(0, signum as u64);
            ctx.set_arg(1, fault_addr);
            ctx.pc = action.handler as u64;
            return true;
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

    #[cfg(target_os = "linux")]
    #[test]
    fn test_host_signal_bridge_is_opt_in_and_restores_actions() {
        extern "C" fn router(_: i32, _: *mut libc::siginfo_t, _: *mut libc::c_void) {}

        let dispatcher = std::sync::Arc::new(SignalDispatcher::new());
        let bridge = unsafe { dispatcher.install_host_signal_bridge(router) }.unwrap();
        let second = unsafe { dispatcher.install_host_signal_bridge(router) };
        assert!(matches!(
            second,
            Err(HostSignalBridgeError::AlreadyInstalled)
        ));
        drop(bridge);
        let bridge = unsafe { dispatcher.install_host_signal_bridge(router) }.unwrap();
        drop(bridge);
    }

    #[test]
    fn test_host_fault_signal_capability_is_explicit() {
        assert_eq!(HOST_FAULT_SIGNALS.len(), 4);
        assert!(HOST_FAULT_SIGNALS.contains(&libc::SIGSEGV));
        assert!(HOST_FAULT_SIGNALS.contains(&libc::SIGILL));
        assert!(HOST_FAULT_SIGNALS.contains(&libc::SIGBUS));
        assert!(HOST_FAULT_SIGNALS.contains(&libc::SIGFPE));
    }
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
