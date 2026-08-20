//! Android Bionic libm Replacement and Math Shims.
#![allow(clippy::missing_safety_doc)]

use std::os::raw::c_double;

pub unsafe extern "C" fn bionic_sin(x: c_double) -> c_double {
    x.sin()
}

pub unsafe extern "C" fn bionic_cos(x: c_double) -> c_double {
    x.cos()
}

pub unsafe extern "C" fn bionic_tan(x: c_double) -> c_double {
    x.tan()
}

pub unsafe extern "C" fn bionic_asin(x: c_double) -> c_double {
    x.asin()
}

pub unsafe extern "C" fn bionic_acos(x: c_double) -> c_double {
    x.acos()
}

pub unsafe extern "C" fn bionic_atan(x: c_double) -> c_double {
    x.atan()
}

pub unsafe extern "C" fn bionic_atan2(y: c_double, x: c_double) -> c_double {
    y.atan2(x)
}

pub unsafe extern "C" fn bionic_sqrt(x: c_double) -> c_double {
    x.sqrt()
}

pub unsafe extern "C" fn bionic_cbrt(x: c_double) -> c_double {
    x.cbrt()
}

pub unsafe extern "C" fn bionic_pow(x: c_double, y: c_double) -> c_double {
    x.powf(y)
}

pub unsafe extern "C" fn bionic_exp(x: c_double) -> c_double {
    x.exp()
}

pub unsafe extern "C" fn bionic_log(x: c_double) -> c_double {
    x.ln()
}

pub unsafe extern "C" fn bionic_log2(x: c_double) -> c_double {
    x.log2()
}

pub unsafe extern "C" fn bionic_log10(x: c_double) -> c_double {
    x.log10()
}

pub unsafe extern "C" fn bionic_fabs(x: c_double) -> c_double {
    x.abs()
}

pub unsafe extern "C" fn bionic_floor(x: c_double) -> c_double {
    x.floor()
}

pub unsafe extern "C" fn bionic_ceil(x: c_double) -> c_double {
    x.ceil()
}

pub unsafe extern "C" fn bionic_round(x: c_double) -> c_double {
    x.round()
}

pub unsafe extern "C" fn bionic_fmod(x: c_double, y: c_double) -> c_double {
    x % y
}
