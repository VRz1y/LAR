//! Android Bionic liblog / Logcat Shims.
//!
//! Provides shims for `__android_log_write`, `__android_log_print`, and log routing.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

pub const ANDROID_LOG_UNKNOWN: c_int = 0;
pub const ANDROID_LOG_DEFAULT: c_int = 1;
pub const ANDROID_LOG_VERBOSE: c_int = 2;
pub const ANDROID_LOG_DEBUG: c_int = 3;
pub const ANDROID_LOG_INFO: c_int = 4;
pub const ANDROID_LOG_WARN: c_int = 5;
pub const ANDROID_LOG_ERROR: c_int = 6;
pub const ANDROID_LOG_FATAL: c_int = 7;
pub const ANDROID_LOG_SILENT: c_int = 8;

fn log_prio_char(prio: c_int) -> char {
    match prio {
        ANDROID_LOG_VERBOSE => 'V',
        ANDROID_LOG_DEBUG => 'D',
        ANDROID_LOG_INFO => 'I',
        ANDROID_LOG_WARN => 'W',
        ANDROID_LOG_ERROR => 'E',
        ANDROID_LOG_FATAL => 'F',
        _ => 'I',
    }
}

pub unsafe extern "C" fn bionic_android_log_write(
    prio: c_int,
    tag: *const c_char,
    text: *const c_char,
) -> c_int {
    let tag_str = if !tag.is_null() {
        unsafe { CStr::from_ptr(tag).to_string_lossy() }
    } else {
        std::borrow::Cow::Borrowed("UNKNOWN")
    };

    let text_str = if !text.is_null() {
        unsafe { CStr::from_ptr(text).to_string_lossy() }
    } else {
        std::borrow::Cow::Borrowed("")
    };

    let p = log_prio_char(prio);
    println!("[Logcat/{}/{}] {}", p, tag_str, text_str);
    text_str.len() as c_int
}

pub unsafe extern "C" fn bionic_android_log_print(
    prio: c_int,
    tag: *const c_char,
    fmt_ptr: *const c_char,
    // Varargs follow in C calling convention
) -> c_int {
    let tag_str = if !tag.is_null() {
        unsafe { CStr::from_ptr(tag).to_string_lossy() }
    } else {
        std::borrow::Cow::Borrowed("UNKNOWN")
    };

    let msg_str = if !fmt_ptr.is_null() {
        unsafe { CStr::from_ptr(fmt_ptr).to_string_lossy() }
    } else {
        std::borrow::Cow::Borrowed("")
    };

    let p = log_prio_char(prio);
    println!("[Logcat/{}/{}] {}", p, tag_str, msg_str);
    msg_str.len() as c_int
}

pub unsafe extern "C" fn bionic_android_log_buf_write(
    _buf_id: c_int,
    prio: c_int,
    tag: *const c_char,
    text: *const c_char,
) -> c_int {
    unsafe { bionic_android_log_write(prio, tag, text) }
}

pub unsafe extern "C" fn bionic_android_log_buf_print(
    _buf_id: c_int,
    prio: c_int,
    tag: *const c_char,
    fmt_ptr: *const c_char,
) -> c_int {
    unsafe { bionic_android_log_print(prio, tag, fmt_ptr) }
}

pub unsafe extern "C" fn bionic_android_log_assert(
    cond: *const c_char,
    tag: *const c_char,
    fmt_ptr: *const c_char,
) -> ! {
    let cond_str = if !cond.is_null() {
        unsafe { CStr::from_ptr(cond).to_string_lossy() }
    } else {
        std::borrow::Cow::Borrowed("assertion failed")
    };

    let tag_str = if !tag.is_null() {
        unsafe { CStr::from_ptr(tag).to_string_lossy() }
    } else {
        std::borrow::Cow::Borrowed("ASSERT")
    };

    let msg = if !fmt_ptr.is_null() {
        unsafe { CStr::from_ptr(fmt_ptr).to_string_lossy() }
    } else {
        std::borrow::Cow::Borrowed("")
    };

    eprintln!(
        "[Logcat/Fatal/{}] Assertion '{}' failed: {}",
        tag_str, cond_str, msg
    );
    unsafe { libc::abort() }
}
