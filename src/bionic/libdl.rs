#![allow(clippy::missing_safety_doc)]

use std::os::raw::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

static LAST_DLERROR: AtomicPtr<c_char> = AtomicPtr::new(std::ptr::null_mut());

pub unsafe extern "C" fn bionic_dlopen(filename: *const c_char, flags: c_int) -> *mut c_void {
    if filename.is_null() {
        return unsafe { libc::dlopen(std::ptr::null(), flags) };
    }
    unsafe { libc::dlopen(filename, flags) }
}

pub unsafe extern "C" fn bionic_dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    if symbol.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { libc::dlsym(handle, symbol) }
}

pub unsafe extern "C" fn bionic_dlclose(handle: *mut c_void) -> c_int {
    if handle.is_null() {
        return 0;
    }
    unsafe { libc::dlclose(handle) }
}

pub unsafe extern "C" fn bionic_dlerror() -> *mut c_char {
    let err = unsafe { libc::dlerror() };
    if !err.is_null() {
        LAST_DLERROR.store(err, Ordering::Relaxed);
    }
    LAST_DLERROR.swap(std::ptr::null_mut(), Ordering::Relaxed)
}

pub unsafe extern "C" fn bionic_dladdr(addr: *const c_void, info: *mut libc::Dl_info) -> c_int {
    unsafe { libc::dladdr(addr, info) }
}
