//! Android Bionic Subsystem: libc, libm, libdl, liblog replacements and shims.

pub mod libc;
pub mod libdl;
pub mod liblog;
pub mod libm;

use crate::linker::symbols::SymbolRegistry;

/// Registers all Android Bionic runtime replacement symbols into the symbol registry.
pub fn register_bionic_shims(registry: &mut SymbolRegistry) {
    // libc memory functions
    registry.register("malloc", libc::bionic_malloc as *const () as usize);
    registry.register("free", libc::bionic_free as *const () as usize);
    registry.register("calloc", libc::bionic_calloc as *const () as usize);
    registry.register("realloc", libc::bionic_realloc as *const () as usize);
    registry.register("memcpy", libc::bionic_memcpy as *const () as usize);
    registry.register("memset", libc::bionic_memset as *const () as usize);
    registry.register("memmove", libc::bionic_memmove as *const () as usize);
    registry.register("memcmp", libc::bionic_memcmp as *const () as usize);

    // libc string functions
    registry.register("strlen", libc::bionic_strlen as *const () as usize);
    registry.register("strcmp", libc::bionic_strcmp as *const () as usize);
    registry.register("strncmp", libc::bionic_strncmp as *const () as usize);
    registry.register("strcpy", libc::bionic_strcpy as *const () as usize);
    registry.register("strncpy", libc::bionic_strncpy as *const () as usize);
    registry.register("strdup", libc::bionic_strdup as *const () as usize);
    registry.register("puts", libc::bionic_puts as *const () as usize);
    registry.register("abort", libc::bionic_abort as *const () as usize);
    registry.register("exit", libc::bionic_exit as *const () as usize);

    // libc pthread functions
    registry.register("pthread_self", libc::bionic_pthread_self as *const () as usize);
    registry.register("pthread_mutex_init", libc::bionic_pthread_mutex_init as *const () as usize);
    registry.register("pthread_mutex_lock", libc::bionic_pthread_mutex_lock as *const () as usize);
    registry.register("pthread_mutex_unlock", libc::bionic_pthread_mutex_unlock as *const () as usize);
    registry.register("pthread_mutex_destroy", libc::bionic_pthread_mutex_destroy as *const () as usize);

    // libm math functions
    registry.register("sin", libm::bionic_sin as *const () as usize);
    registry.register("cos", libm::bionic_cos as *const () as usize);
    registry.register("tan", libm::bionic_tan as *const () as usize);
    registry.register("asin", libm::bionic_asin as *const () as usize);
    registry.register("acos", libm::bionic_acos as *const () as usize);
    registry.register("atan", libm::bionic_atan as *const () as usize);
    registry.register("atan2", libm::bionic_atan2 as *const () as usize);
    registry.register("sqrt", libm::bionic_sqrt as *const () as usize);
    registry.register("cbrt", libm::bionic_cbrt as *const () as usize);
    registry.register("pow", libm::bionic_pow as *const () as usize);
    registry.register("exp", libm::bionic_exp as *const () as usize);
    registry.register("log", libm::bionic_log as *const () as usize);
    registry.register("log2", libm::bionic_log2 as *const () as usize);
    registry.register("log10", libm::bionic_log10 as *const () as usize);
    registry.register("fabs", libm::bionic_fabs as *const () as usize);
    registry.register("floor", libm::bionic_floor as *const () as usize);
    registry.register("ceil", libm::bionic_ceil as *const () as usize);
    registry.register("round", libm::bionic_round as *const () as usize);
    registry.register("fmod", libm::bionic_fmod as *const () as usize);

    // libdl functions
    registry.register("dlopen", libdl::bionic_dlopen as *const () as usize);
    registry.register("dlsym", libdl::bionic_dlsym as *const () as usize);
    registry.register("dlclose", libdl::bionic_dlclose as *const () as usize);
    registry.register("dlerror", libdl::bionic_dlerror as *const () as usize);
    registry.register("dladdr", libdl::bionic_dladdr as *const () as usize);

    // liblog Android logger functions
    registry.register("__android_log_write", liblog::bionic_android_log_write as *const () as usize);
    registry.register("__android_log_print", liblog::bionic_android_log_print as *const () as usize);
    registry.register("__android_log_buf_write", liblog::bionic_android_log_buf_write as *const () as usize);
    registry.register("__android_log_buf_print", liblog::bionic_android_log_buf_print as *const () as usize);
    registry.register("__android_log_assert", liblog::bionic_android_log_assert as *const () as usize);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_bionic_shims() {
        let mut registry = SymbolRegistry::new();
        register_bionic_shims(&mut registry);

        assert!(registry.resolve("malloc").is_some());
        assert!(registry.resolve("free").is_some());
        assert!(registry.resolve("sin").is_some());
        assert!(registry.resolve("dlopen").is_some());
        assert!(registry.resolve("__android_log_print").is_some());
        assert!(registry.count() >= 25);
    }
}
