use std::ffi::{CStr, CString};
use std::ptr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeWireCapabilities {
    pub available: bool,
    pub version: Option<String>,
    pub has_stream_api: bool,
}

pub fn probe_pipewire() -> PipeWireCapabilities {
    #[cfg(target_os = "linux")]
    {
        for name in ["libpipewire-0.3.so.0", "libpipewire-0.3.so"] {
            if let Some(handle) = Library::open(name) {
                let symbols_complete = [
                    "pw_init",
                    "pw_main_loop_new",
                    "pw_main_loop_get_loop",
                    "pw_main_loop_destroy",
                    "pw_stream_new_simple",
                    "pw_stream_connect",
                    "pw_stream_disconnect",
                    "pw_stream_destroy",
                    "pw_stream_set_active",
                ]
                .iter()
                .all(|symbol| handle.has_symbol(symbol));
                let version = handle.library_version();
                drop(handle);
                let has_stream_api = symbols_complete
                    && PipeWireStream::connect("LAR capability probe", true).is_ok();
                return PipeWireCapabilities {
                    available: symbols_complete,
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
        (!version.is_null()).then(|| {
            unsafe { CStr::from_ptr(version) }
                .to_string_lossy()
                .into_owned()
        })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.0) };
    }
}

#[repr(C)]
struct StreamEvents {
    version: u32,
    callbacks: [usize; 12],
}

pub struct PipeWireStream {
    main_loop: *mut libc::c_void,
    stream: *mut libc::c_void,
    set_active: unsafe extern "C" fn(*mut libc::c_void, bool) -> libc::c_int,
    disconnect: unsafe extern "C" fn(*mut libc::c_void) -> libc::c_int,
    destroy_stream: unsafe extern "C" fn(*mut libc::c_void),
    destroy_loop: unsafe extern "C" fn(*mut libc::c_void),
    _events: Box<StreamEvents>,
    _library: Library,
}

impl std::fmt::Debug for PipeWireStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipeWireStream").finish_non_exhaustive()
    }
}

impl PipeWireStream {
    pub fn connect(name: &str, output: bool) -> Result<Self, PipeWireError> {
        let library = Library::open("libpipewire-0.3.so.0")
            .or_else(|| Library::open("libpipewire-0.3.so"))
            .ok_or(PipeWireError::Unavailable)?;
        unsafe {
            symbol::<unsafe extern "C" fn(*mut i32, *mut *mut libc::c_char)>(&library, "pw_init")?(
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        let new_loop = symbol::<
            unsafe extern "C" fn(*mut libc::c_void, *mut libc::c_void) -> *mut libc::c_void,
        >(&library, "pw_main_loop_new")?;
        let main_loop = unsafe { new_loop(ptr::null_mut(), ptr::null_mut()) };
        if main_loop.is_null() {
            return Err(PipeWireError::CreateFailed);
        }
        let get_loop = symbol::<unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void>(
            &library,
            "pw_main_loop_get_loop",
        )?;
        let events = Box::new(StreamEvents {
            version: 0,
            callbacks: [0; 12],
        });
        let name = CString::new(name).map_err(|_| PipeWireError::InvalidName)?;
        let new_stream = symbol::<
            unsafe extern "C" fn(
                *mut libc::c_void,
                *const libc::c_char,
                *mut libc::c_void,
                *const StreamEvents,
                *mut libc::c_void,
            ) -> *mut libc::c_void,
        >(&library, "pw_stream_new_simple")?;
        let stream = unsafe {
            new_stream(
                get_loop(main_loop),
                name.as_ptr(),
                ptr::null_mut(),
                events.as_ref(),
                ptr::null_mut(),
            )
        };
        if stream.is_null() {
            unsafe {
                symbol::<unsafe extern "C" fn(*mut libc::c_void)>(&library, "pw_main_loop_destroy")?(
                    main_loop,
                )
            };
            return Err(PipeWireError::CreateFailed);
        }
        let connect = symbol::<
            unsafe extern "C" fn(
                *mut libc::c_void,
                u32,
                u32,
                u32,
                *const *const libc::c_void,
                u32,
            ) -> libc::c_int,
        >(&library, "pw_stream_connect")?;
        let direction = u32::from(!output);
        let result = unsafe { connect(stream, direction, u32::MAX, 1, ptr::null(), 0) };
        if result < 0 {
            unsafe {
                symbol::<unsafe extern "C" fn(*mut libc::c_void)>(&library, "pw_stream_destroy")?(
                    stream,
                );
                symbol::<unsafe extern "C" fn(*mut libc::c_void)>(
                    &library,
                    "pw_main_loop_destroy",
                )?(main_loop);
            }
            return Err(PipeWireError::ConnectFailed(-result));
        }
        Ok(Self {
            main_loop,
            stream,
            set_active: symbol(&library, "pw_stream_set_active")?,
            disconnect: symbol(&library, "pw_stream_disconnect")?,
            destroy_stream: symbol(&library, "pw_stream_destroy")?,
            destroy_loop: symbol(&library, "pw_main_loop_destroy")?,
            _events: events,
            _library: library,
        })
    }

    pub fn set_active(&self, active: bool) -> Result<(), PipeWireError> {
        let result = unsafe { (self.set_active)(self.stream, active) };
        if result < 0 {
            Err(PipeWireError::OperationFailed(-result))
        } else {
            Ok(())
        }
    }
}

impl Drop for PipeWireStream {
    fn drop(&mut self) {
        unsafe {
            (self.disconnect)(self.stream);
            (self.destroy_stream)(self.stream);
            (self.destroy_loop)(self.main_loop);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeWireError {
    Unavailable,
    MissingSymbol,
    InvalidName,
    CreateFailed,
    ConnectFailed(i32),
    OperationFailed(i32),
}

fn symbol<T: Copy>(library: &Library, name: &str) -> Result<T, PipeWireError> {
    let pointer = library.symbol(name).ok_or(PipeWireError::MissingSymbol)?;
    Ok(unsafe { std::mem::transmute_copy(&pointer) })
}
