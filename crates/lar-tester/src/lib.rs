//! LAR Test Harness and QEMU/APK Validation Suite.

pub mod apk;
pub mod qemu;
pub mod runner;
pub mod synthetic;

pub use apk::{ApkError, ApkNativeLib, ApkReader};
pub use qemu::QemuEnvironment;
pub use runner::{LarTestHarness, TestReport};
pub use synthetic::{generate_synthetic_apk, generate_synthetic_arm64_so};
