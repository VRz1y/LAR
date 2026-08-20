use super::{BufferDescription, GraphicBuffer, PixelFormat};
use crate::graphics::DmaBufPlane;
use std::ffi::{CString, c_void};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::Path;

struct Library(*mut c_void);

impl Library {
    fn open(names: &[&str]) -> Result<Self, HostGraphicsError> {
        for name in names {
            let name = CString::new(*name).map_err(|_| HostGraphicsError::LibraryUnavailable)?;
            let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if !handle.is_null() {
                return Ok(Self(handle));
            }
        }
        Err(HostGraphicsError::LibraryUnavailable)
    }

    fn symbol<T: Copy>(&self, name: &str) -> Result<T, HostGraphicsError> {
        let name = CString::new(name).map_err(|_| HostGraphicsError::MissingSymbol)?;
        let symbol = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        if symbol.is_null() {
            return Err(HostGraphicsError::MissingSymbol);
        }
        Ok(unsafe { std::mem::transmute_copy(&symbol) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.0) };
    }
}

pub struct WaylandConnection {
    display: *mut c_void,
    disconnect: unsafe extern "C" fn(*mut c_void),
    _library: Library,
}

impl WaylandConnection {
    pub fn connect() -> Result<Self, HostGraphicsError> {
        let library = Library::open(&["libwayland-client.so.0", "libwayland-client.so"])?;
        let connect: unsafe extern "C" fn(*const libc::c_char) -> *mut c_void =
            library.symbol("wl_display_connect")?;
        let disconnect = library.symbol("wl_display_disconnect")?;
        let display = unsafe { connect(std::ptr::null()) };
        if display.is_null() {
            return Err(HostGraphicsError::WaylandConnectionFailed);
        }
        Ok(Self {
            display,
            disconnect,
            _library: library,
        })
    }

    pub fn is_connected(&self) -> bool {
        !self.display.is_null()
    }
}

impl Drop for WaylandConnection {
    fn drop(&mut self) {
        unsafe { (self.disconnect)(self.display) };
    }
}

pub struct GbmAllocator {
    device: *mut c_void,
    destroy_device: unsafe extern "C" fn(*mut c_void),
    create_bo: unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void,
    destroy_bo: unsafe extern "C" fn(*mut c_void),
    get_fd: unsafe extern "C" fn(*mut c_void) -> libc::c_int,
    get_stride: unsafe extern "C" fn(*mut c_void) -> u32,
    get_modifier: unsafe extern "C" fn(*mut c_void) -> u64,
    _drm: File,
    _library: Library,
}

impl GbmAllocator {
    pub fn open_default() -> Result<Self, HostGraphicsError> {
        for index in 128..144 {
            let path = format!("/dev/dri/renderD{index}");
            if Path::new(&path).exists() {
                return Self::open(path);
            }
        }
        Err(HostGraphicsError::DrmNodeUnavailable)
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, HostGraphicsError> {
        let drm = OpenOptions::new().read(true).write(true).open(path)?;
        let library = Library::open(&["libgbm.so.1", "libgbm.so"])?;
        let create_device: unsafe extern "C" fn(libc::c_int) -> *mut c_void =
            library.symbol("gbm_create_device")?;
        let device = unsafe { create_device(std::os::fd::AsRawFd::as_raw_fd(&drm)) };
        if device.is_null() {
            return Err(HostGraphicsError::GbmDeviceFailed);
        }
        Ok(Self {
            device,
            destroy_device: library.symbol("gbm_device_destroy")?,
            create_bo: library.symbol("gbm_bo_create")?,
            destroy_bo: library.symbol("gbm_bo_destroy")?,
            get_fd: library.symbol("gbm_bo_get_fd")?,
            get_stride: library.symbol("gbm_bo_get_stride")?,
            get_modifier: library.symbol("gbm_bo_get_modifier")?,
            _drm: drm,
            _library: library,
        })
    }

    pub fn allocate(
        &self,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> Result<GraphicBuffer, HostGraphicsError> {
        let fourcc = match format {
            PixelFormat::Rgba8888 => 0x3432_4241,
            PixelFormat::Bgra8888 => 0x3432_5241,
            PixelFormat::Rgb565 => 0x3631_4752,
            _ => return Err(HostGraphicsError::UnsupportedFormat),
        };
        let bo = unsafe { (self.create_bo)(self.device, width, height, fourcc, 1 << 2) };
        if bo.is_null() {
            return Err(HostGraphicsError::BufferAllocationFailed);
        }
        let fd = unsafe { (self.get_fd)(bo) };
        let stride = unsafe { (self.get_stride)(bo) };
        let modifier = unsafe { (self.get_modifier)(bo) };
        unsafe { (self.destroy_bo)(bo) };
        if fd < 0 {
            return Err(HostGraphicsError::BufferExportFailed);
        }
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let size = u64::from(stride)
            .checked_mul(u64::from(height))
            .ok_or(HostGraphicsError::BufferAllocationFailed)?;
        let plane = DmaBufPlane::from_owned_fd(owned, 0, stride, size, modifier)
            .map_err(|_| HostGraphicsError::BufferExportFailed)?;
        GraphicBuffer::new(
            BufferDescription {
                width,
                height,
                stride: stride / format.bytes_per_pixel().unwrap_or(1) as u32,
                format,
            },
            vec![plane],
        )
        .map_err(|_| HostGraphicsError::BufferAllocationFailed)
    }
}

impl Drop for GbmAllocator {
    fn drop(&mut self) {
        unsafe { (self.destroy_device)(self.device) };
    }
}

#[derive(Debug)]
pub enum HostGraphicsError {
    Io(std::io::Error),
    LibraryUnavailable,
    MissingSymbol,
    WaylandConnectionFailed,
    DrmNodeUnavailable,
    GbmDeviceFailed,
    BufferAllocationFailed,
    BufferExportFailed,
    UnsupportedFormat,
}

impl fmt::Display for HostGraphicsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "host graphics error: {self:?}")
    }
}

impl std::error::Error for HostGraphicsError {}

impl From<std::io::Error> for HostGraphicsError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
