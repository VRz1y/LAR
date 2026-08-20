//! Android Bionic libc Replacement and Shims.
//!
//! Provides native implementations of core C standard library functions for Bionic compatibility.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

pub unsafe extern "C" fn bionic_malloc(size: usize) -> *mut c_void {
    unsafe { libc::malloc(size) }
}

pub unsafe extern "C" fn bionic_free(ptr: *mut c_void) {
    if !ptr.is_null() {
        unsafe {
            libc::free(ptr);
        }
    }
}

pub unsafe extern "C" fn bionic_calloc(num: usize, size: usize) -> *mut c_void {
    unsafe { libc::calloc(num, size) }
}

pub unsafe extern "C" fn bionic_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { libc::realloc(ptr, size) }
}

pub unsafe extern "C" fn bionic_memcpy(
    dst: *mut c_void,
    src: *const c_void,
    n: usize,
) -> *mut c_void {
    unsafe { libc::memcpy(dst, src, n) }
}

pub unsafe extern "C" fn bionic_memset(s: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    unsafe { libc::memset(s, c, n) }
}

pub unsafe extern "C" fn bionic_memmove(
    dst: *mut c_void,
    src: *const c_void,
    n: usize,
) -> *mut c_void {
    unsafe { libc::memmove(dst, src, n) }
}

pub unsafe extern "C" fn bionic_memcmp(s1: *const c_void, s2: *const c_void, n: usize) -> c_int {
    unsafe { libc::memcmp(s1, s2, n) }
}

pub unsafe extern "C" fn bionic_strlen(s: *const c_char) -> usize {
    unsafe { libc::strlen(s) }
}

pub unsafe extern "C" fn bionic_strcmp(s1: *const c_char, s2: *const c_char) -> c_int {
    unsafe { libc::strcmp(s1, s2) }
}

pub unsafe extern "C" fn bionic_strncmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int {
    unsafe { libc::strncmp(s1, s2, n) }
}

pub unsafe extern "C" fn bionic_strcpy(dst: *mut c_char, src: *const c_char) -> *mut c_char {
    unsafe { libc::strcpy(dst, src) }
}

pub unsafe extern "C" fn bionic_strncpy(
    dst: *mut c_char,
    src: *const c_char,
    n: usize,
) -> *mut c_char {
    unsafe { libc::strncpy(dst, src, n) }
}

pub unsafe extern "C" fn bionic_strdup(s: *const c_char) -> *mut c_char {
    unsafe { libc::strdup(s) }
}

pub unsafe extern "C" fn bionic_puts(s: *const c_char) -> c_int {
    if !s.is_null() {
        let cstr = unsafe { CStr::from_ptr(s) };
        if let Ok(str_slice) = cstr.to_str() {
            println!("{}", str_slice);
            return 0;
        }
    }
    unsafe { libc::puts(s) }
}

pub unsafe extern "C" fn bionic_abort() -> ! {
    eprintln!("[LAR] Bionic abort() called");
    unsafe { libc::abort() }
}

pub unsafe extern "C" fn bionic_exit(status: c_int) -> ! {
    unsafe { libc::exit(status) }
}

pub unsafe extern "C" fn bionic_pthread_self() -> libc::pthread_t {
    unsafe { libc::pthread_self() }
}

pub unsafe extern "C" fn bionic_pthread_mutex_init(
    mutex: *mut libc::pthread_mutex_t,
    attr: *const libc::pthread_mutexattr_t,
) -> c_int {
    unsafe { libc::pthread_mutex_init(mutex, attr) }
}

pub unsafe extern "C" fn bionic_pthread_mutex_lock(mutex: *mut libc::pthread_mutex_t) -> c_int {
    unsafe { libc::pthread_mutex_lock(mutex) }
}

pub unsafe extern "C" fn bionic_pthread_mutex_unlock(mutex: *mut libc::pthread_mutex_t) -> c_int {
    unsafe { libc::pthread_mutex_unlock(mutex) }
}

pub unsafe extern "C" fn bionic_pthread_mutex_destroy(mutex: *mut libc::pthread_mutex_t) -> c_int {
    unsafe { libc::pthread_mutex_destroy(mutex) }
}
