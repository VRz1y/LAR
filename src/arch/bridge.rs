//! Call Bridges and ABI Shims between Guest (ARM64) and Host (x86_64 / RISC-V / ARM64).
//!
//! Provides ABI translation and invocation dispatch for foreign targets,
//! marshaling register parameters and return values.

use crate::arch::context::Arm64CpuContext;

/// Host signature for 0-argument C function.
pub type HostFn0 = unsafe extern "C" fn() -> usize;
/// Host signature for 1-argument C function.
pub type HostFn1 = unsafe extern "C" fn(usize) -> usize;
/// Host signature for 2-argument C function.
pub type HostFn2 = unsafe extern "C" fn(usize, usize) -> usize;
/// Host signature for 3-argument C function.
pub type HostFn3 = unsafe extern "C" fn(usize, usize, usize) -> usize;
/// Host signature for 4-argument C function.
pub type HostFn4 = unsafe extern "C" fn(usize, usize, usize, usize) -> usize;
/// Host signature for 5-argument C function.
pub type HostFn5 = unsafe extern "C" fn(usize, usize, usize, usize, usize) -> usize;
/// Host signature for 6-argument C function.
pub type HostFn6 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize) -> usize;
/// Host signature for 8-argument C function.
pub type HostFn8 =
    unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> usize;

/// Dynamic bridge handler taking guest context directly.
pub type GuestBridgeHandler = fn(&mut Arm64CpuContext);

/// Represents an ABI Bridge entry that shims guest ARM64 calls to host implementations.
#[derive(Clone, Copy)]
pub enum CallBridge {
    /// Handler function taking full mutable CPU context.
    ContextHandler(GuestBridgeHandler),
    /// Host C function with specified argument count (up to 8).
    HostCFunction {
        func_ptr: *const (),
        arg_count: usize,
    },
}

unsafe impl Send for CallBridge {}
unsafe impl Sync for CallBridge {}

impl CallBridge {
    /// Creates a bridge from a context handler.
    pub const fn from_handler(handler: GuestBridgeHandler) -> Self {
        Self::ContextHandler(handler)
    }

    /// Creates a bridge from a host function pointer with given argument count.
    pub fn from_c_fn(func_ptr: *const (), arg_count: usize) -> Self {
        Self::HostCFunction {
            func_ptr,
            arg_count,
        }
    }

    /// Dispatches a call from guest ARM64 context through the bridge to the host.
    pub fn invoke(&self, ctx: &mut Arm64CpuContext) {
        match self {
            Self::ContextHandler(handler) => {
                handler(ctx);
            }
            Self::HostCFunction {
                func_ptr,
                arg_count,
            } => {
                let a0 = ctx.get_arg(0) as usize;
                let a1 = ctx.get_arg(1) as usize;
                let a2 = ctx.get_arg(2) as usize;
                let a3 = ctx.get_arg(3) as usize;
                let a4 = ctx.get_arg(4) as usize;
                let a5 = ctx.get_arg(5) as usize;
                let a6 = ctx.get_arg(6) as usize;
                let a7 = ctx.get_arg(7) as usize;

                let ret = unsafe {
                    match *arg_count {
                        0 => {
                            let f: HostFn0 = std::mem::transmute(*func_ptr);
                            f()
                        }
                        1 => {
                            let f: HostFn1 = std::mem::transmute(*func_ptr);
                            f(a0)
                        }
                        2 => {
                            let f: HostFn2 = std::mem::transmute(*func_ptr);
                            f(a0, a1)
                        }
                        3 => {
                            let f: HostFn3 = std::mem::transmute(*func_ptr);
                            f(a0, a1, a2)
                        }
                        4 => {
                            let f: HostFn4 = std::mem::transmute(*func_ptr);
                            f(a0, a1, a2, a3)
                        }
                        5 => {
                            let f: HostFn5 = std::mem::transmute(*func_ptr);
                            f(a0, a1, a2, a3, a4)
                        }
                        6 => {
                            let f: HostFn6 = std::mem::transmute(*func_ptr);
                            f(a0, a1, a2, a3, a4, a5)
                        }
                        _ => {
                            let f: HostFn8 = std::mem::transmute(*func_ptr);
                            f(a0, a1, a2, a3, a4, a5, a6, a7)
                        }
                    }
                };

                ctx.set_return(ret as u64);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn sample_add(a: usize, b: usize) -> usize {
        a + b
    }

    extern "C" fn sample_mul3(a: usize, b: usize, c: usize) -> usize {
        a * b * c
    }

    #[test]
    fn test_call_bridge_host_c_fn() {
        let bridge_add = CallBridge::from_c_fn(sample_add as *const (), 2);
        let mut ctx = Arm64CpuContext::new();
        ctx.set_arg(0, 15);
        ctx.set_arg(1, 27);

        bridge_add.invoke(&mut ctx);
        assert_eq!(ctx.get_return(), 42);

        let bridge_mul = CallBridge::from_c_fn(sample_mul3 as *const (), 3);
        ctx.set_arg(0, 2);
        ctx.set_arg(1, 3);
        ctx.set_arg(2, 4);

        bridge_mul.invoke(&mut ctx);
        assert_eq!(ctx.get_return(), 24);
    }

    #[test]
    fn test_call_bridge_context_handler() {
        fn custom_handler(ctx: &mut Arm64CpuContext) {
            let x = ctx.get_arg(0);
            ctx.set_return(x * 10);
        }

        let bridge = CallBridge::from_handler(custom_handler);
        let mut ctx = Arm64CpuContext::new();
        ctx.set_arg(0, 7);

        bridge.invoke(&mut ctx);
        assert_eq!(ctx.get_return(), 70);
    }
}
