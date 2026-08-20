//! Integration tests for APK Extraction and LAR Native Execution.

use lar::api_policy::AndroidApi;
use lar_tester::*;
use std::fs;

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
    let libs =
        ApkReader::extract_arm64_libs_from_memory(&apk_bytes).expect("Failed to extract APK");
    assert_eq!(libs.len(), 2);
    assert_eq!(libs[0].name, "libcrypto_arm64.so");
    assert_eq!(libs[1].name, "libphysics_arm64.so");

    // 4. Load in LAR Runtime
    let mut runtime = lar::LarRuntime::new();
    let loaded1 = runtime.load_library(&libs[0].name, &libs[0].data).unwrap();
    assert!(lar::memory::is_16k_aligned(loaded1.load_base));
    assert!(
        loaded1
            .lookup_symbol("Java_com_example_crypto_sign")
            .is_some()
    );

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

#[test]
fn test_manifest_metadata_is_extracted() {
    let manifest = br#"<manifest package="com.example.app"><uses-sdk android:minSdkVersion="21" android:targetSdkVersion="35"/><application><activity android:name=".MainActivity"><intent-filter><action android:name="android.intent.action.MAIN"/></intent-filter></activity></application></manifest>"#;
    let apk = generate_synthetic_apk(&[("AndroidManifest.xml", manifest)]);
    let metadata = ApkReader::read_metadata_from_memory(&apk).unwrap();
    assert_eq!(metadata.package.as_deref(), Some("com.example.app"));
    assert_eq!(metadata.launcher_activity.as_deref(), Some(".MainActivity"));
    assert_eq!(metadata.min_sdk, Some(21));
    assert_eq!(metadata.target_sdk, Some(35));
    ApkReader::check_compatibility(&metadata, AndroidApi::API_35).unwrap();
}

#[test]
fn apk_sdk_compatibility_rejects_newer_requirements() {
    let manifest = br#"<manifest package="com.example.app"><uses-sdk android:minSdkVersion="36" android:targetSdkVersion="36"/></manifest>"#;
    let apk = generate_synthetic_apk(&[("AndroidManifest.xml", manifest)]);
    let metadata = ApkReader::read_metadata_from_memory(&apk).unwrap();
    assert!(ApkReader::check_compatibility(&metadata, AndroidApi::API_35).is_err());
}

#[test]
fn binary_manifest_metadata_and_api_policy_are_decoded() {
    let manifest = binary_manifest(36, 36);
    let apk = generate_synthetic_apk(&[("AndroidManifest.xml", &manifest)]);
    let metadata = ApkReader::read_metadata_from_memory(&apk).unwrap();
    assert_eq!(metadata.package.as_deref(), Some("com.example.binary"));
    assert_eq!(metadata.launcher_activity.as_deref(), Some(".MainActivity"));
    assert_eq!(metadata.min_sdk, Some(36));
    assert_eq!(metadata.target_sdk, Some(36));
    assert_eq!(metadata.manifest_xml.as_deref(), Some(manifest.as_slice()));
    assert!(ApkReader::check_compatibility(&metadata, AndroidApi::API_35).is_err());
    ApkReader::check_compatibility(&metadata, AndroidApi::API_36).unwrap();
}

#[test]
fn binary_manifest_api_35_policy_is_accepted() {
    let manifest = binary_manifest(21, 35);
    let apk = generate_synthetic_apk(&[("AndroidManifest.xml", &manifest)]);
    let metadata = ApkReader::read_metadata_from_memory(&apk).unwrap();
    ApkReader::check_compatibility(&metadata, AndroidApi::API_35).unwrap();
    ApkReader::check_compatibility(&metadata, AndroidApi::API_36).unwrap();
}

#[test]
fn malformed_binary_manifests_are_rejected() {
    let manifest = binary_manifest(21, 35);
    let truncated = &manifest[..manifest.len() - 1];
    let apk = generate_synthetic_apk(&[("AndroidManifest.xml", truncated)]);
    assert!(matches!(
        ApkReader::read_metadata_from_memory(&apk),
        Err(ApkError::CorruptedEntry(_))
    ));

    let mut oversized_chunk = manifest;
    oversized_chunk[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    let apk = generate_synthetic_apk(&[("AndroidManifest.xml", &oversized_chunk)]);
    assert!(matches!(
        ApkReader::read_metadata_from_memory(&apk),
        Err(ApkError::CorruptedEntry(_))
    ));
}

#[test]
fn api_aware_install_enforces_sdk_compatibility() {
    let root = std::env::temp_dir().join(format!("lar-apk-sdk-{}", std::process::id()));
    let manifest = br#"<manifest package="com.example.sdk"><uses-sdk android:minSdkVersion="36" android:targetSdkVersion="36"/></manifest>"#;
    let apk = generate_synthetic_apk(&[
        ("AndroidManifest.xml", manifest),
        ("classes.dex", &minimal_dex()),
    ]);
    assert!(ApkReader::install_from_memory_for_api(&apk, &root, AndroidApi::API_35).is_err());
    assert!(ApkReader::install_from_memory_for_api(&apk, &root, AndroidApi::API_36).is_ok());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn persistent_install_writes_multidex_and_arm64_paths() {
    let root = std::env::temp_dir().join(format!("lar-apk-install-{}", std::process::id()));
    let manifest = br#"<manifest package="com.example.installer"><application><activity android:name=".MainActivity"><intent-filter><action android:name="android.intent.action.MAIN"/></intent-filter></activity></application></manifest>"#;
    let dex = minimal_dex();
    let so = generate_synthetic_arm64_so("libnative.so", "native_entry");
    let apk = generate_synthetic_apk(&[
        ("AndroidManifest.xml", manifest),
        ("classes2.dex", &dex),
        ("classes.dex", &dex),
        ("lib/arm64-v8a/libnative.so", &so),
    ]);

    let installed = ApkReader::install_from_memory(&apk, &root).unwrap();
    assert_eq!(installed.multidex.files.len(), 2);
    assert_eq!(installed.multidex.files[0].name, "classes.dex");
    assert_eq!(installed.multidex.files[1].name, "classes2.dex");
    assert_eq!(
        installed.application.dex_path.as_ref(),
        Some(&installed.multidex.files[0].path)
    );
    assert_eq!(installed.native_library_paths.len(), 1);
    assert!(
        installed
            .multidex
            .files
            .iter()
            .all(|file| file.path.is_file())
    );
    assert!(installed.native_library_paths[0].is_file());

    let installed_again = ApkReader::install_from_memory(&apk, &root).unwrap();
    assert_eq!(installed_again.multidex, installed.multidex);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn persistent_install_rejects_path_traversal_package() {
    let root = std::env::temp_dir().join(format!("lar-apk-install-unsafe-{}", std::process::id()));
    let manifest = br#"<manifest package="../outside"><application/></manifest>"#;
    let apk = generate_synthetic_apk(&[
        ("AndroidManifest.xml", manifest),
        ("classes.dex", &minimal_dex()),
    ]);
    assert!(matches!(
        ApkReader::install_from_memory(&apk, &root),
        Err(ApkError::UnsafePath(_))
    ));
    let _ = fs::remove_dir_all(root);
}

fn minimal_dex() -> Vec<u8> {
    let mut dex = vec![0u8; 112];
    dex[..8].copy_from_slice(b"dex\n035\0");
    dex[32..36].copy_from_slice(&(112u32).to_le_bytes());
    dex[36..40].copy_from_slice(&(112u32).to_le_bytes());
    dex[40..44].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    dex
}

fn binary_manifest(min_sdk: u32, target_sdk: u32) -> Vec<u8> {
    let strings = [
        "manifest",
        "package",
        "com.example.binary",
        "uses-sdk",
        "minSdkVersion",
        "targetSdkVersion",
        "application",
        "activity",
        "name",
        ".MainActivity",
        "intent-filter",
        "action",
        "android.intent.action.MAIN",
        "category",
        "android.intent.category.LAUNCHER",
        "android",
        "http://schemas.android.com/apk/res/android",
    ];
    let mut body = string_pool(&strings);
    body.extend(namespace_chunk(0x0100, 15, 16));
    body.extend(start_element(0, &[(u32::MAX, 1, 2, 0x03, 2)]));
    body.extend(start_element(
        3,
        &[
            (16, 4, u32::MAX, 0x10, min_sdk),
            (16, 5, u32::MAX, 0x10, target_sdk),
        ],
    ));
    body.extend(end_element(3));
    body.extend(start_element(6, &[]));
    body.extend(start_element(7, &[(16, 8, u32::MAX, 0x03, 9)]));
    body.extend(start_element(10, &[]));
    body.extend(start_element(11, &[(16, 8, 12, 0x03, 12)]));
    body.extend(end_element(11));
    body.extend(start_element(13, &[(16, 8, u32::MAX, 0x03, 14)]));
    body.extend(end_element(13));
    body.extend(end_element(10));
    body.extend(end_element(7));
    body.extend(end_element(6));
    body.extend(end_element(0));
    body.extend(namespace_chunk(0x0101, 15, 16));
    let mut xml = Vec::with_capacity(body.len() + 8);
    push_u16(&mut xml, 0x0003);
    push_u16(&mut xml, 8);
    push_u32(&mut xml, (body.len() + 8) as u32);
    xml.extend(body);
    xml
}

fn string_pool(strings: &[&str]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for value in strings {
        offsets.push(data.len() as u32);
        push_length8(&mut data, value.chars().count());
        push_length8(&mut data, value.len());
        data.extend_from_slice(value.as_bytes());
        data.push(0);
    }
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let strings_start = 28 + offsets.len() * 4;
    let mut chunk = Vec::new();
    push_u16(&mut chunk, 0x0001);
    push_u16(&mut chunk, 28);
    push_u32(&mut chunk, (strings_start + data.len()) as u32);
    push_u32(&mut chunk, strings.len() as u32);
    push_u32(&mut chunk, 0);
    push_u32(&mut chunk, 1 << 8);
    push_u32(&mut chunk, strings_start as u32);
    push_u32(&mut chunk, 0);
    for offset in offsets {
        push_u32(&mut chunk, offset);
    }
    chunk.extend(data);
    chunk
}

fn namespace_chunk(chunk_type: u16, prefix: u32, uri: u32) -> Vec<u8> {
    let mut chunk = node_header(chunk_type, 24);
    push_u32(&mut chunk, prefix);
    push_u32(&mut chunk, uri);
    chunk
}

fn start_element(name: u32, attributes: &[(u32, u32, u32, u8, u32)]) -> Vec<u8> {
    let size = 36 + attributes.len() * 20;
    let mut chunk = node_header(0x0102, size);
    push_u32(&mut chunk, u32::MAX);
    push_u32(&mut chunk, name);
    push_u16(&mut chunk, 20);
    push_u16(&mut chunk, 20);
    push_u16(&mut chunk, attributes.len() as u16);
    push_u16(&mut chunk, 0);
    push_u16(&mut chunk, 0);
    push_u16(&mut chunk, 0);
    for &(namespace, attribute_name, raw, value_type, value) in attributes {
        push_u32(&mut chunk, namespace);
        push_u32(&mut chunk, attribute_name);
        push_u32(&mut chunk, raw);
        push_u16(&mut chunk, 8);
        chunk.push(0);
        chunk.push(value_type);
        push_u32(&mut chunk, value);
    }
    chunk
}

fn end_element(name: u32) -> Vec<u8> {
    let mut chunk = node_header(0x0103, 24);
    push_u32(&mut chunk, u32::MAX);
    push_u32(&mut chunk, name);
    chunk
}

fn node_header(chunk_type: u16, size: usize) -> Vec<u8> {
    let mut chunk = Vec::new();
    push_u16(&mut chunk, chunk_type);
    push_u16(&mut chunk, 16);
    push_u32(&mut chunk, size as u32);
    push_u32(&mut chunk, 1);
    push_u32(&mut chunk, u32::MAX);
    chunk
}

fn push_length8(output: &mut Vec<u8>, length: usize) {
    if length > 0x7f {
        output.push(((length >> 8) as u8) | 0x80);
    }
    output.push(length as u8);
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}
