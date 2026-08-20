//! Signal handling and dispatching subsystem for LAR.

pub mod dispatcher;

pub use dispatcher::{
    GUEST_SIGABRT, GUEST_SIGALRM, GUEST_SIGBUS, GUEST_SIGFPE, GUEST_SIGHUP, GUEST_SIGILL,
    GUEST_SIGINT, GUEST_SIGKILL, GUEST_SIGPIPE, GUEST_SIGQUIT, GUEST_SIGSEGV, GUEST_SIGTERM,
    GUEST_SIGTRAP, GUEST_SIGUSR1, GUEST_SIGUSR2, GuestSigAction, SignalDispatcher,
};
