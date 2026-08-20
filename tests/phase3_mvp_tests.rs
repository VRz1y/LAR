use lar::LarRuntime;
use lar::api_policy::{AndroidApi, ApiPolicy, ApiPolicyError, ApiTier, BundleTierMetadata};
use lar::art::{AndroidRuntimeBundle, ArtConfig, ArtRuntime, FakeArtBackend, RuntimeBundleCatalog};
use lar::dex::{DexError, DexReader};
use lar::ipc::{BinderRegistry, CoreServiceState, Parcel, transaction};
use lar::lifecycle::{InputEvent, RuntimeLifecycle};
use lar::managers::ApplicationInfo;
use std::fs;
use std::sync::{Arc, Mutex};

fn minimal_dex() -> Vec<u8> {
    let mut dex = vec![0; 120];
    dex[..8].copy_from_slice(b"dex\n035\0");
    dex[32..36].copy_from_slice(&(120u32).to_le_bytes());
    dex[36..40].copy_from_slice(&(112u32).to_le_bytes());
    dex[40..44].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    dex[56..60].copy_from_slice(&1u32.to_le_bytes());
    dex[60..64].copy_from_slice(&112u32.to_le_bytes());
    dex[96..100].copy_from_slice(&0u32.to_le_bytes());
    dex[100..104].copy_from_slice(&112u32.to_le_bytes());
    dex[112..116].copy_from_slice(&(116u32).to_le_bytes());
    dex[116] = 3;
    dex[117..120].copy_from_slice(b"app");
    dex
}

#[test]
fn dex_reader_rejects_truncated_and_reads_bounded_metadata() {
    assert_eq!(DexReader::read(&[0; 4]), Err(DexError::TooSmall));
    let metadata = DexReader::read(&minimal_dex()).unwrap();
    assert_eq!(metadata.version, "035");
    assert_eq!(metadata.strings, vec!["app"]);
}

#[test]
fn fake_art_backend_drives_application_lifecycle() {
    let backend = Arc::new(FakeArtBackend::default());
    let mut runtime = LarRuntime::new();
    runtime.art = ArtRuntime::with_config(ArtConfig {
        libart: None,
        dex2oat: None,
        classpath: Vec::new(),
    })
    .with_backend(backend.clone());
    runtime.art.initialize().unwrap();
    runtime.install_application(ApplicationInfo {
        package: "demo.app".into(),
        launcher_activity: Some(".MainActivity".into()),
        dex_path: None,
        dex: None,
        native_libraries: Vec::new(),
    });
    let id = runtime.start_application("demo.app").unwrap();
    assert_eq!(runtime.activity_manager.top().unwrap().id, id);
    assert_eq!(runtime.lifecycle, RuntimeLifecycle::Started);
    assert_eq!(backend.started_packages(), vec!["demo.app"]);
    runtime.input_dispatcher.push(InputEvent::Key {
        code: 4,
        pressed: true,
    });
    assert_eq!(runtime.input_dispatcher.drain().len(), 1);
}

#[test]
fn application_start_passes_the_installed_dex_path_to_art() {
    let path = std::env::temp_dir().join(format!("lar-phase3-{}.dex", std::process::id()));
    fs::write(&path, minimal_dex()).unwrap();
    let backend = Arc::new(FakeArtBackend::default());
    let mut runtime = LarRuntime::new();
    runtime.art = ArtRuntime::with_config(ArtConfig {
        libart: None,
        dex2oat: None,
        classpath: Vec::new(),
    })
    .with_backend(backend.clone());
    runtime.art.initialize().unwrap();
    runtime.install_application(ApplicationInfo {
        package: "demo.dex".into(),
        launcher_activity: Some("MainActivity".into()),
        dex_path: Some(path.clone()),
        dex: Some(DexReader::read(&minimal_dex()).unwrap()),
        native_libraries: Vec::new(),
    });
    runtime.start_application("demo.dex").unwrap();
    assert_eq!(backend.started_packages(), vec!["demo.dex"]);
    fs::remove_file(path).unwrap();
}

#[test]
fn binder_core_services_are_in_process() {
    let registry = lar::ipc::BinderRegistry::new();
    registry.register_core_services();
    assert!(registry.get("activity").is_some());
    assert!(registry.get("input").is_some());
}

#[test]
fn android_runtime_bundle_resolves_apex_layout() {
    let root = std::env::temp_dir().join(format!("lar-art-bundle-{}", std::process::id()));
    let paths = [
        "system/apex/com.android.art/lib64/libart.so",
        "system/apex/com.android.art/bin/dex2oat64",
        "system/apex/com.android.art/javalib/core-oj.jar",
        "system/apex/com.android.art/javalib/core-libart.jar",
        "system/framework/framework.jar",
    ];
    for path in paths {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, []).unwrap();
    }
    let bundle = AndroidRuntimeBundle::discover(&root).unwrap();
    assert_eq!(bundle.art_config().classpath.len(), 3);
    assert!(ArtRuntime::from_bundle(&bundle).is_available());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn android_api_policy_resolves_primary_before_secondary() {
    let policy = ApiPolicy::default();
    assert_eq!(policy.classify(36), ApiTier::Primary);
    assert_eq!(policy.classify(35), ApiTier::Secondary);
    assert_eq!(policy.classify(34), ApiTier::Unsupported);
    let secondary = BundleTierMetadata::new(35, "android-15", Some("sha256:35".into()), "ready");
    let primary = BundleTierMetadata::new(36, "android-16", Some("sha256:36".into()), "ready");
    assert_eq!(policy.resolve(&[secondary, primary]).unwrap().api.0, 36);
}

#[test]
fn android_api_policy_rejects_unsupported_and_incomplete_bundles() {
    let policy = ApiPolicy::default();
    let unsupported = BundleTierMetadata::new(34, "android-14", Some("sha256:34".into()), "ready");
    assert!(matches!(
        policy.validate(&unsupported),
        Err(ApiPolicyError::UnsupportedApi(_))
    ));
    let incomplete =
        BundleTierMetadata::new(36, "android-16", Some("sha256:36".into()), "incomplete");
    assert!(matches!(
        policy.validate(&incomplete),
        Err(ApiPolicyError::BundleNotReady(_))
    ));
}

#[test]
fn android_api_policy_parses_manifest_and_skips_unready_primary() {
    let policy = ApiPolicy::default();
    let manifest = "api\tandroid\ttag\tdigest\ttier\tstatus\n35\t15\tandroid-15\tsha256:35\tsecondary\tready\n36\t16\tandroid-16\t-\tprimary\tincomplete\n";
    let bundles = policy.resolve_manifest(manifest).unwrap();
    assert_eq!(policy.resolve(&bundles).unwrap().api, AndroidApi::API_35);
}

#[test]
fn android_api_policy_checks_apk_sdk_bounds() {
    let policy = ApiPolicy::default();
    assert!(
        policy
            .check_apk_compatibility(AndroidApi::API_35, Some(35), Some(35))
            .is_ok()
    );
    assert!(matches!(
        policy.check_apk_compatibility(AndroidApi::API_35, Some(36), Some(35)),
        Err(ApiPolicyError::ApkMinSdkTooHigh { .. })
    ));
    assert!(matches!(
        policy.check_apk_compatibility(AndroidApi::API_35, Some(35), Some(36)),
        Err(ApiPolicyError::ApkTargetSdkTooHigh { .. })
    ));
}

#[test]
fn runtime_bundle_catalog_resolves_api_and_initializes_runtime() {
    let root = std::env::temp_dir().join(format!("lar-catalog-{}", std::process::id()));
    let manifest = root.join("manifest.tsv");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        &manifest,
        "api\tandroid\ttag\tdigest\ttier\tstatus\n35\t15\tandroid-15\tsha256:35\tsecondary\tready\n36\t16\tandroid-16\tsha256:36\tprimary\tready\n",
    )
    .unwrap();
    for path in [
        "android16/system/apex/com.android.art/lib64/libart.so",
        "android16/system/apex/com.android.art/bin/dex2oat64",
        "android16/system/apex/com.android.art/javalib/core-oj.jar",
        "android16/system/apex/com.android.art/javalib/core-libart.jar",
        "android16/system/framework/framework.jar",
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, []).unwrap();
    }
    let catalog = RuntimeBundleCatalog::load(&root, &manifest).unwrap();
    let bundle = catalog.resolve_for_apk(Some(35), Some(36)).unwrap();
    assert_eq!(bundle.metadata.api, AndroidApi::API_36);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn typed_core_services_round_trip_against_manager_state() {
    let state = Arc::new(Mutex::new(CoreServiceState::new()));
    state.lock().unwrap().package_manager.parse_manifest(
        r#"<manifest package="demo.app" versionCode="7"><uses-permission android:name="android.permission.INTERNET"/></manifest>"#,
    ).unwrap();
    let registry = BinderRegistry::new();
    registry.register_core_services_with_state(state.clone());

    let activity = registry.get("activity").unwrap();
    let mut start = Parcel::new();
    start.write_string("demo.app");
    start.write_string("MainActivity");
    let mut started = activity.transact(transaction::activity::START, start);
    let id = started.read_i64().unwrap() as u64;

    let mut top = activity.transact(transaction::activity::GET_TOP, Parcel::new());
    assert_eq!(top.read_bool(), Some(true));
    assert_eq!(top.read_i64(), Some(id as i64));
    assert_eq!(top.read_string().as_deref(), Some("demo.app"));
    assert_eq!(top.read_string().as_deref(), Some("MainActivity"));
    assert_eq!(top.read_i32(), Some(1));

    let package = registry.get("package").unwrap();
    let mut lookup = Parcel::new();
    lookup.write_string("demo.app");
    let mut package_reply = package.transact(transaction::package::GET_PACKAGE, lookup);
    assert_eq!(package_reply.read_bool(), Some(true));
    assert_eq!(package_reply.read_string().as_deref(), Some("demo.app"));
    assert_eq!(package_reply.read_i64(), Some(7));
    assert_eq!(package_reply.read_i32(), Some(1));
    assert_eq!(
        package_reply.read_string().as_deref(),
        Some("android.permission.INTERNET")
    );

    let window = registry.get("window").unwrap();
    let mut create = Parcel::new();
    create.write_i64(id as i64);
    create.write_i32(10);
    create.write_i32(20);
    create.write_i32(100);
    create.write_i32(200);
    create.write_i32(160);
    let mut created = window.transact(transaction::window::CREATE, create);
    assert_eq!(created.read_bool(), Some(true));

    let mut resize = Parcel::new();
    resize.write_i64(id as i64);
    resize.write_i32(300);
    resize.write_i32(400);
    let mut resized = window.transact(transaction::window::RESIZE, resize);
    assert_eq!(resized.read_bool(), Some(true));

    let mut geometry_request = Parcel::new();
    geometry_request.write_i64(id as i64);
    let mut geometry = window.transact(transaction::window::GET_GEOMETRY, geometry_request);
    assert_eq!(geometry.read_bool(), Some(true));
    assert_eq!(geometry.read_i32(), Some(10));
    assert_eq!(geometry.read_i32(), Some(20));
    assert_eq!(geometry.read_i32(), Some(300));
    assert_eq!(geometry.read_i32(), Some(400));
    assert_eq!(geometry.read_i32(), Some(160));

    let input = registry.get("input").unwrap();
    let mut inject = Parcel::new();
    inject.write_i32(0);
    inject.write_i32(4);
    inject.write_bool(true);
    let mut injected = input.transact(transaction::input::INJECT, inject);
    assert_eq!(injected.read_bool(), Some(true));
    let mut drained = input.transact(transaction::input::DRAIN, Parcel::new());
    assert_eq!(drained.read_i32(), Some(1));
    assert_eq!(drained.read_i32(), Some(0));
    assert_eq!(drained.read_i32(), Some(4));
    assert_eq!(drained.read_bool(), Some(true));
    assert_eq!(
        input
            .transact(transaction::input::DRAIN, Parcel::new())
            .read_i32(),
        Some(0)
    );
    assert_eq!(state.lock().unwrap().activity_manager.top().unwrap().id, id);
}
