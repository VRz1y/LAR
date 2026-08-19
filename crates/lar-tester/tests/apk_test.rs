//! Integration tests for APK Extraction and LAR Native Execution.

use lar_tester::*;

#[test]
fn test_apk_extraction_and_lar_loading_lifecycle() {
    // 1. Generate two ARM64 .so libraries
    let so1 = generate_synthetic_arm64_so("libcrypto_arm64.so", "Java_com_example_crypto_sign");
    let so2 = generate_synthetic_arm64_so("libphysics_arm64.so", "physics_simulate_step");

    // 2. Package into APK
    let apk_bytes = generate_synthetic_apk(&[
        ("lib/arm64-v8a/libcrypto_arm64.so", &so1),
        ("lib/arm64-v8a/libphysics_arm64.so", &so2),
        ("assets/config.json", b"{\"version\": 1}"),
    ]);

    // 3. Extract libraries from APK
    let libs = ApkReader::extract_arm64_libs_from_memory(&apk_bytes).expect("Failed to extract APK");
    assert_eq!(libs.len(), 2);
    assert_eq!(libs[0].name, "libcrypto_arm64.so");
    assert_eq!(libs[1].name, "libphysics_arm64.so");

    // 4. Load in LAR Runtime
    let mut runtime = lar::LarRuntime::new();
    let loaded1 = runtime.load_library(&libs[0].name, &libs[0].data).unwrap();
    assert!(lar::memory::is_16k_aligned(loaded1.load_base));
    assert!(loaded1.lookup_symbol("Java_com_example_crypto_sign").is_some());

    let loaded2 = runtime.load_library(&libs[1].name, &libs[1].data).unwrap();
    assert!(lar::memory::is_16k_aligned(loaded2.load_base));
    assert!(loaded2.lookup_symbol("physics_simulate_step").is_some());

    // 5. Verify Bionic shims are linked
    assert!(runtime.resolve_symbol("malloc").is_some());
    assert!(runtime.resolve_symbol("free").is_some());
    assert!(runtime.resolve_symbol("__android_log_print").is_some());
}

#[test]
fn test_self_test_harness_execution() {
    let report = LarTestHarness::run_self_test();
    assert!(report.is_success());
    assert!(report.passed_checks >= 5);
    assert_eq!(report.failed_checks, 0);
}
