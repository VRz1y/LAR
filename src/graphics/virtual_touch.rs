use crate::lifecycle::InputEvent;
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::mem::size_of;
use std::path::Path;

const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const UI_DEV_SETUP: libc::c_ulong = 0x405c5503;
const UI_ABS_SETUP: libc::c_ulong = 0x401c5504;
const UI_SET_EVBIT: libc::c_ulong = 0x40045564;
const UI_SET_KEYBIT: libc::c_ulong = 0x40045565;
const UI_SET_ABSBIT: libc::c_ulong = 0x40045567;
const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const BTN_TOUCH: u16 = 330;

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}
#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}
#[repr(C)]
struct InputAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}
#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    padding: u16,
    absinfo: InputAbsInfo,
}
#[repr(C)]
struct RawInputEvent {
    time: libc::timeval,
    event_type: u16,
    code: u16,
    value: i32,
}

#[derive(Debug)]
pub struct VirtualTouchBackend {
    device: Option<std::fs::File>,
    max_x: i32,
    max_y: i32,
}
impl VirtualTouchBackend {
    pub fn open(max_x: i32, max_y: i32) -> Result<Self, VirtualTouchError> {
        if max_x <= 0 || max_y <= 0 {
            return Err(VirtualTouchError::InvalidEvent);
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(Path::new("/dev/uinput"))
            .map_err(map_open_error)?;
        let fd = std::os::fd::AsRawFd::as_raw_fd(&file);
        for (request, value) in [
            (UI_SET_EVBIT, EV_KEY),
            (UI_SET_EVBIT, EV_ABS),
            (UI_SET_KEYBIT, BTN_TOUCH),
            (UI_SET_ABSBIT, ABS_X),
            (UI_SET_ABSBIT, ABS_Y),
        ] {
            if unsafe { libc::ioctl(fd, request, value as libc::c_int) } < 0 {
                return Err(map_ioctl_error());
            }
        }
        let mut setup = UinputSetup {
            id: InputId {
                bustype: 0x06,
                vendor: 0x1d6b,
                product: 1,
                version: 1,
            },
            name: [0; 80],
            ff_effects_max: 0,
        };
        let name = b"LAR Virtual Touch";
        setup.name[..name.len()].copy_from_slice(name);
        if unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup) } < 0 {
            return Err(map_ioctl_error());
        }
        for (code, maximum) in [(ABS_X, max_x), (ABS_Y, max_y)] {
            let abs = UinputAbsSetup {
                code,
                padding: 0,
                absinfo: InputAbsInfo {
                    value: 0,
                    minimum: 0,
                    maximum,
                    fuzz: 0,
                    flat: 0,
                    resolution: 0,
                },
            };
            if unsafe { libc::ioctl(fd, UI_ABS_SETUP, &abs) } < 0 {
                return Err(map_ioctl_error());
            }
        }
        if unsafe { libc::ioctl(fd, UI_DEV_CREATE) } < 0 {
            return Err(map_ioctl_error());
        }
        Ok(Self {
            device: Some(file),
            max_x,
            max_y,
        })
    }
    pub fn unsupported(max_x: i32, max_y: i32) -> Self {
        Self {
            device: None,
            max_x,
            max_y,
        }
    }
    pub fn is_supported(&self) -> bool {
        self.device.is_some()
    }
    pub fn emit(&mut self, event: InputEvent) -> Result<(), VirtualTouchError> {
        let InputEvent::Touch { x, y, pressed } = event else {
            return Err(VirtualTouchError::InvalidEvent);
        };
        if x < 0 || y < 0 || x > self.max_x || y > self.max_y {
            return Err(VirtualTouchError::InvalidEvent);
        }
        let Some(file) = self.device.as_mut() else {
            return Err(VirtualTouchError::Unsupported);
        };
        for event in [
            raw(EV_ABS, ABS_X, x),
            raw(EV_ABS, ABS_Y, y),
            raw(EV_KEY, BTN_TOUCH, i32::from(pressed)),
            raw(EV_SYN, SYN_REPORT, 0),
        ] {
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    (&event as *const RawInputEvent).cast::<u8>(),
                    size_of::<RawInputEvent>(),
                )
            };
            file.write_all(bytes).map_err(VirtualTouchError::Io)?;
        }
        Ok(())
    }
    pub fn stop(&mut self) {
        if let Some(file) = self.device.take() {
            unsafe {
                libc::ioctl(std::os::fd::AsRawFd::as_raw_fd(&file), UI_DEV_DESTROY);
            }
        }
    }
}
impl Drop for VirtualTouchBackend {
    fn drop(&mut self) {
        self.stop();
    }
}
fn raw(event_type: u16, code: u16, value: i32) -> RawInputEvent {
    RawInputEvent {
        time: libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        event_type,
        code,
        value,
    }
}
fn map_open_error(error: std::io::Error) -> VirtualTouchError {
    if matches!(
        error.raw_os_error(),
        Some(libc::ENOENT | libc::ENODEV | libc::EACCES | libc::EPERM)
    ) {
        VirtualTouchError::Unsupported
    } else {
        VirtualTouchError::Io(error)
    }
}
fn map_ioctl_error() -> VirtualTouchError {
    let error = std::io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(libc::ENOTTY | libc::EOPNOTSUPP | libc::ENODEV | libc::EPERM | libc::EACCES)
    ) {
        VirtualTouchError::Unsupported
    } else {
        VirtualTouchError::Io(error)
    }
}
#[derive(Debug)]
pub enum VirtualTouchError {
    Unsupported,
    InvalidEvent,
    Io(std::io::Error),
}
impl fmt::Display for VirtualTouchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "virtual touch error: {self:?}")
    }
}
impl std::error::Error for VirtualTouchError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_backend_is_graceful_and_stoppable() {
        let mut backend = VirtualTouchBackend::unsupported(100, 100);
        assert_eq!(
            backend
                .emit(InputEvent::Touch {
                    x: 1,
                    y: 2,
                    pressed: true
                })
                .unwrap_err()
                .to_string(),
            "virtual touch error: Unsupported"
        );
        backend.stop();
        assert!(!backend.is_supported());
    }
}
