use std::ffi::{CString, c_void};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    pub available: bool,
    pub library: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphicsCapabilities {
    pub wayland: Capability,
    pub gbm: Capability,
    pub egl: Capability,
    pub vulkan: Capability,
}

impl GraphicsCapabilities {
    pub fn probe() -> Self {
        Self {
            wayland: probe_wayland(),
            gbm: probe_library(
                &["libgbm.so.1", "libgbm.so"],
                &[
                    "gbm_create_device",
                    "gbm_device_destroy",
                    "gbm_bo_create",
                    "gbm_bo_destroy",
                    "gbm_bo_get_fd",
                    "gbm_bo_get_stride",
                    "gbm_bo_get_modifier",
                ],
            ),
            egl: probe_library(
                &["libEGL.so.1", "libEGL.so"],
                &["eglGetDisplay", "eglInitialize", "eglCreateImage"],
            ),
            vulkan: probe_library(
                &["libvulkan.so.1", "libvulkan.so"],
                &["vkGetInstanceProcAddr", "vkCreateInstance"],
            ),
        }
    }

    pub fn any_native_backend(&self) -> bool {
        self.egl.available || self.vulkan.available
    }

    pub fn zero_copy_ready(&self) -> bool {
        self.wayland.available && self.gbm.available && self.any_native_backend()
    }
}

impl Default for GraphicsCapabilities {
    fn default() -> Self {
        Self::probe()
    }
}

fn probe_wayland() -> Capability {
    for library in ["libwayland-client.so.0", "libwayland-client.so"] {
        let Ok(name) = CString::new(library) else {
            continue;
        };
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            continue;
        }
        let connect = load_symbol(handle, "wl_display_connect");
        let disconnect = load_symbol(handle, "wl_display_disconnect");
        let available = if let (Some(connect), Some(disconnect)) = (connect, disconnect) {
            let connect: unsafe extern "C" fn(*const libc::c_char) -> *mut c_void =
                unsafe { std::mem::transmute(connect) };
            let disconnect: unsafe extern "C" fn(*mut c_void) =
                unsafe { std::mem::transmute(disconnect) };
            let display = unsafe { connect(std::ptr::null()) };
            if display.is_null() {
                false
            } else {
                unsafe { disconnect(display) };
                true
            }
        } else {
            false
        };
        unsafe { libc::dlclose(handle) };
        if available {
            return Capability {
                available: true,
                library: Some(library),
            };
        }
    }
    Capability {
        available: false,
        library: None,
    }
}

fn probe_library(libraries: &[&'static str], symbols: &[&str]) -> Capability {
    for &library in libraries {
        if library_has_symbols(library, symbols) {
            return Capability {
                available: true,
                library: Some(library),
            };
        }
    }
    Capability {
        available: false,
        library: None,
    }
}

fn load_symbol(handle: *mut c_void, symbol: &str) -> Option<*mut c_void> {
    let symbol = CString::new(symbol).ok()?;
    let pointer = unsafe { libc::dlsym(handle, symbol.as_ptr()) };
    (!pointer.is_null()).then_some(pointer)
}

fn library_has_symbols(name: &str, symbols: &[&str]) -> bool {
    let Ok(name) = CString::new(name) else {
        return false;
    };
    let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        return false;
    }
    let available = symbols.iter().all(|symbol| {
        let Ok(symbol) = CString::new(*symbol) else {
            return false;
        };
        unsafe { !libc::dlsym(handle, symbol.as_ptr()).is_null() }
    });
    unsafe {
        libc::dlclose(handle);
    }
    available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_copy_requires_complete_stack() {
        let available = Capability {
            available: true,
            library: Some("test"),
        };
        let unavailable = Capability {
            available: false,
            library: None,
        };
        let capabilities = GraphicsCapabilities {
            wayland: available,
            gbm: available,
            egl: unavailable,
            vulkan: available,
        };
        assert!(capabilities.zero_copy_ready());

        let capabilities = GraphicsCapabilities {
            wayland: unavailable,
            ..capabilities
        };
        assert!(!capabilities.zero_copy_ready());
    }
}
