//! Unit and Integration Tests for Android Bionic Replacement Shims.

use lar::bionic::*;
use lar::linker::SymbolRegistry;
use std::ffi::CString;

#[test]
fn test_bionic_libc_memory() {
    unsafe {
        let ptr = libc::bionic_malloc(64);
        assert!(!ptr.is_null());

        libc::bionic_memset(ptr, 0x5A, 64);
        let slice = std::slice::from_raw_parts(ptr as *const u8, 64);
        assert!(slice.iter().all(|&b| b == 0x5A));

        let ptr2 = libc::bionic_malloc(64);
        libc::bionic_memcpy(ptr2, ptr, 64);
        assert_eq!(libc::bionic_memcmp(ptr, ptr2, 64), 0);

        let ptr3 = libc::bionic_realloc(ptr2, 128);
        assert!(!ptr3.is_null());

        libc::bionic_free(ptr);
        libc::bionic_free(ptr3);
    }
}

#[test]
fn test_bionic_libc_strings() {
    unsafe {
        let c_str = CString::new("Hello Android NDK").unwrap();
        let len = libc::bionic_strlen(c_str.as_ptr());
        assert_eq!(len, 17);

        let dup = libc::bionic_strdup(c_str.as_ptr());
        assert!(!dup.is_null());
        assert_eq!(libc::bionic_strcmp(c_str.as_ptr(), dup), 0);
        assert_eq!(libc::bionic_strncmp(c_str.as_ptr(), dup, 5), 0);

        libc::bionic_free(dup as *mut _);
    }
}

#[test]
fn test_bionic_libm_math() {
    unsafe {
        assert!((libm::bionic_sin(0.0) - 0.0).abs() < 1e-9);
        assert!((libm::bionic_cos(0.0) - 1.0).abs() < 1e-9);
        assert!((libm::bionic_sqrt(16.0) - 4.0).abs() < 1e-9);
        assert!((libm::bionic_pow(2.0, 8.0) - 256.0).abs() < 1e-9);
        assert!((libm::bionic_floor(3.7) - 3.0).abs() < 1e-9);
        assert!((libm::bionic_ceil(3.2) - 4.0).abs() < 1e-9);
        assert!((libm::bionic_fabs(-42.5) - 42.5).abs() < 1e-9);
    }
}

#[test]
fn test_bionic_liblog_writing() {
    unsafe {
        let tag = CString::new("NativeEngine").unwrap();
        let msg = CString::new("Initializing 3D renderer...").unwrap();

        let ret =
            liblog::bionic_android_log_write(liblog::ANDROID_LOG_INFO, tag.as_ptr(), msg.as_ptr());
        assert!(ret > 0);
    }
}

#[test]
fn test_bionic_symbol_registry_coverage() {
    let mut registry = SymbolRegistry::new();
    register_bionic_shims(&mut registry);

    let symbols = [
        "malloc",
        "free",
        "calloc",
        "realloc",
        "memcpy",
        "memset",
        "strlen",
        "strcmp",
        "strdup",
        "sin",
        "cos",
        "sqrt",
        "pow",
        "dlopen",
        "dlsym",
        "dlclose",
        "dlerror",
        "__android_log_write",
        "__android_log_print",
        "pthread_mutex_init",
    ];

    for sym in symbols {
        assert!(
            registry.resolve(sym).is_some(),
            "Expected Bionic symbol '{}' to be registered",
            sym
        );
    }
}
