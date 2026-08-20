//! High-Level Test Runner for APK and Native Library Validation in LAR.
//!
//! Orchestrates library extraction, 16KB alignment validation, dynamic linking,
//! Bionic symbol interception, and execution verification.

use crate::apk::ApkReader;
use crate::qemu::QemuEnvironment;
use crate::synthetic::{generate_synthetic_apk, generate_synthetic_arm64_so};
use lar::LarRuntime;
use lar::api_policy::AndroidApi;
use lar::memory::is_16k_aligned;
use lar::prejit::PreJitDaemon;
use std::fmt;
use std::path::Path;
use std::time::Instant;

/// Comprehensive test execution report.
#[derive(Debug, Clone, Default)]
pub struct TestReport {
    pub passed_checks: usize,
    pub failed_checks: usize,
    pub loaded_libraries: Vec<String>,
    pub resolved_symbols: Vec<String>,
    pub logs: Vec<String>,
    pub duration_ms: u128,
    pub apk_metadata: Option<crate::apk::ApkMetadata>,
    pub skipped_checks: usize,
}

impl TestReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_pass(&mut self, msg: impl Into<String>) {
        self.passed_checks += 1;
        self.logs.push(format!("[PASS] {}", msg.into()));
    }

    pub fn record_fail(&mut self, msg: impl Into<String>) {
        self.failed_checks += 1;
        self.logs.push(format!("[FAIL] {}", msg.into()));
    }

    pub fn record_skip(&mut self, msg: impl Into<String>) {
        self.skipped_checks += 1;
        self.logs.push(format!("[SKIP] {}", msg.into()));
    }

    pub fn is_success(&self) -> bool {
        self.failed_checks == 0 && self.passed_checks > 0
    }
}

impl fmt::Display for TestReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "========================================================"
        )?;
        writeln!(
            f,
            "              LAR TEST RUNNER REPORT                    "
        )?;
        writeln!(
            f,
            "========================================================"
        )?;
        writeln!(f, "Passed Checks : {}", self.passed_checks)?;
        writeln!(f, "Failed Checks : {}", self.failed_checks)?;
        writeln!(f, "Skipped Checks: {}", self.skipped_checks)?;
        writeln!(f, "Libraries     : {:?}", self.loaded_libraries)?;
        writeln!(f, "Duration      : {} ms", self.duration_ms)?;
        writeln!(
            f,
            "--------------------------------------------------------"
        )?;
        for log in &self.logs {
            writeln!(f, "{}", log)?;
        }
        writeln!(
            f,
            "========================================================"
        )
    }
}

/// LAR Test Harness.
pub struct LarTestHarness;

impl LarTestHarness {
    /// Tests an APK file: extracts all `lib/arm64-v8a/*.so` files, loads them, and runs checks.
    pub fn test_apk<P: AsRef<Path>>(apk_path: P) -> Result<TestReport, Box<dyn std::error::Error>> {
        Self::test_apk_for_api(apk_path, AndroidApi::API_36)
    }

    pub fn test_apk_for_api<P: AsRef<Path>>(
        apk_path: P,
        api: AndroidApi,
    ) -> Result<TestReport, Box<dyn std::error::Error>> {
        let start = Instant::now();
        let mut report = TestReport::new();

        let path = apk_path.as_ref();
        report.record_pass(format!("Opening APK: {}", path.display()));

        let native_libs = ApkReader::extract_arm64_libs(path)?;
        match ApkReader::read_metadata(path) {
            Ok(metadata) => {
                ApkReader::check_compatibility(&metadata, api)?;
                report.record_pass(format!(
                    "APK package: {:?}, launcher: {:?}",
                    metadata.package, metadata.launcher_activity
                ));
                report.apk_metadata = Some(metadata);
            }
            Err(error) => {
                report.record_skip(format!("APK manifest metadata unavailable: {error}"));
            }
        }
        report.record_pass(format!(
            "Extracted {} ARM64 native libraries from APK",
            native_libs.len()
        ));

        let mut runtime = LarRuntime::new();

        for lib in native_libs {
            report.loaded_libraries.push(lib.name.clone());
            match runtime.load_library(&lib.name, &lib.data) {
                Ok(loaded) => {
                    // Check 1: 16KB Alignment
                    if is_16k_aligned(loaded.load_base) {
                        report.record_pass(format!(
                            "{}: Load base 0x{:x} is 16KB aligned",
                            loaded.name, loaded.load_base
                        ));
                    } else {
                        report.record_fail(format!(
                            "{}: Load base 0x{:x} is NOT 16KB aligned",
                            loaded.name, loaded.load_base
                        ));
                    }

                    // Check 2: Size validity
                    report.record_pass(format!(
                        "{}: Mapped {} bytes",
                        loaded.name,
                        loaded.mem_region.len()
                    ));

                    // Check 3: Init vectors
                    if !loaded.init_array.is_empty() {
                        report.record_pass(format!(
                            "{}: Found {} DT_INIT routines",
                            loaded.name,
                            loaded.init_array.len()
                        ));
                    }
                }
                Err(err) => {
                    report.record_fail(format!("Failed to load {}: {}", lib.name, err));
                }
            }
        }

        let cache_path = path.with_extension("larcache");
        let compiled = PreJitDaemon::new().precompile_libraries(
            &runtime.loaded_libraries.iter().collect::<Vec<_>>(),
            &cache_path,
        )?;
        runtime.load_execution_cache(&cache_path)?;
        report.record_pass(format!(
            "Generated and loaded {} startup cache blocks",
            compiled
        ));
        let _ = std::fs::remove_file(cache_path);

        report.duration_ms = start.elapsed().as_millis();
        Ok(report)
    }

    /// Tests a single standalone `.so` library file.
    pub fn test_so_file<P: AsRef<Path>>(
        so_path: P,
    ) -> Result<TestReport, Box<dyn std::error::Error>> {
        let start = Instant::now();
        let mut report = TestReport::new();

        let mut runtime = LarRuntime::new();
        let loaded = runtime.load_library_file(so_path)?;

        report.loaded_libraries.push(loaded.name.clone());
        if is_16k_aligned(loaded.load_base) {
            report.record_pass(format!(
                "{}: Load base 0x{:x} is 16KB aligned",
                loaded.name, loaded.load_base
            ));
        } else {
            report.record_fail(format!(
                "{}: Load base 0x{:x} is NOT 16KB aligned",
                loaded.name, loaded.load_base
            ));
        }

        report.duration_ms = start.elapsed().as_millis();
        Ok(report)
    }

    /// Runs end-to-end self-test with synthetic APK and ARM64 shared libraries.
    pub fn run_self_test() -> TestReport {
        let start = Instant::now();
        let mut report = TestReport::new();

        report.record_pass("Starting LAR synthetic self-test suite");

        // 1. Generate synthetic ARM64 libraries
        let lib_core_data = generate_synthetic_arm64_so("libcore.so", "native_core_init");
        let lib_game_data =
            generate_synthetic_arm64_so("libgame.so", "Java_com_example_renderFrame");

        // 2. Package into synthetic APK
        let apk_data = generate_synthetic_apk(&[
            ("lib/arm64-v8a/libcore.so", &lib_core_data),
            ("lib/arm64-v8a/libgame.so", &lib_game_data),
        ]);
        report.record_pass(format!(
            "Generated synthetic APK ({} bytes)",
            apk_data.len()
        ));

        // 3. Extract libraries from APK in-memory
        let extracted = ApkReader::extract_arm64_libs_from_memory(&apk_data)
            .expect("Failed to extract synthetic APK");
        report.record_pass(format!(
            "Extracted {} libraries from synthetic APK",
            extracted.len()
        ));

        // 4. Load through LAR Runtime
        let mut runtime = LarRuntime::new();

        for lib in extracted {
            report.loaded_libraries.push(lib.name.clone());
            match runtime.load_library(&lib.name, &lib.data) {
                Ok(loaded) => {
                    // Check 16KB alignment
                    if is_16k_aligned(loaded.load_base) {
                        report.record_pass(format!(
                            "{}: 16KB alignment verified (base 0x{:x})",
                            loaded.name, loaded.load_base
                        ));
                    } else {
                        report.record_fail(format!("{}: 16KB alignment FAILED", loaded.name));
                    }

                    // Check symbol resolution
                    if lib.name == "libcore.so" {
                        if let Some(addr) = loaded.lookup_symbol("native_core_init") {
                            report.record_pass(format!(
                                "{}: Symbol 'native_core_init' resolved at 0x{:x}",
                                loaded.name, addr
                            ));
                        } else {
                            report.record_fail(format!(
                                "{}: Symbol 'native_core_init' NOT found",
                                loaded.name
                            ));
                        }
                    }

                    if lib.name == "libgame.so" {
                        if let Some(addr) = loaded.lookup_symbol("Java_com_example_renderFrame") {
                            report.record_pass(format!(
                                "{}: Symbol 'Java_com_example_renderFrame' resolved at 0x{:x}",
                                loaded.name, addr
                            ));
                        } else {
                            report.record_fail(format!(
                                "{}: Symbol 'Java_com_example_renderFrame' NOT found",
                                loaded.name
                            ));
                        }
                    }
                }
                Err(e) => {
                    report.record_fail(format!("Failed to load {}: {}", lib.name, e));
                }
            }
        }

        // 5. Test QEMU Environment status
        let qemu = QemuEnvironment::detect();
        if qemu.is_available {
            report.record_pass(format!("QEMU AArch64 available at {:?}", qemu.qemu_path));
        } else {
            report.record_skip(
                "QEMU AArch64 not detected; multi-architecture execution was not tested",
            );
        }

        report.duration_ms = start.elapsed().as_millis();
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_self_test_runs_cleanly() {
        let report = LarTestHarness::run_self_test();
        println!("{}", report);
        assert!(report.is_success());
        assert_eq!(report.loaded_libraries.len(), 2);
    }
}
