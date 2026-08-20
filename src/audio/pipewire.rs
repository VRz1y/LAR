use std::ffi::CStr;
use std::ffi::CString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeWireCapabilities {
    pub available: bool,
    pub version: Option<String>,
    pub has_stream_api: bool,
}

pub fn probe_pipewire() -> PipeWireCapabilities {
    #[cfg(target_os = "linux")]
    {
        let names = ["libpipewire-0.3.so.0", "libpipewire-0.3.so"];
        for name in names {
            if let Some(handle) = Library::open(name) {
                let has_stream_api = handle.has_symbol("pw_stream_new_simple")
                    && handle.has_symbol("pw_stream_connect");
                let version = handle.library_version();
                return PipeWireCapabilities {
                    available: true,
                    version,
                    has_stream_api,
                };
            }
        }
    }
    PipeWireCapabilities {
        available: false,
        version: None,
        has_stream_api: false,
    }
}

struct Library(*mut libc::c_void);

impl Library {
    fn open(name: &str) -> Option<Self> {
        let name = CString::new(name).ok()?;
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        (!handle.is_null()).then_some(Self(handle))
    }

    fn symbol(&self, name: &str) -> Option<*mut libc::c_void> {
        let name = CString::new(name).ok()?;
        let symbol = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        (!symbol.is_null()).then_some(symbol)
    }

    fn has_symbol(&self, name: &str) -> bool {
        self.symbol(name).is_some()
    }

    fn library_version(&self) -> Option<String> {
        let symbol = self.symbol("pw_get_library_version")?;
        let function: unsafe extern "C" fn() -> *const libc::c_char =
            unsafe { std::mem::transmute(symbol) };
        let version = unsafe { function() };
        if version.is_null() {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(version) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.0) };
    }
}
