use lar::aidl::{ParameterDirection, generate_rust, parse};
use lar::art::{ArtConfig, ArtError, ArtRuntime, ProcessArtBackend};
use lar::ipc::{Binder, MockBinder, Parcel, SharedRingBuffer};
use lar::managers::{
    ActivityManager, ActivityState, PackageManager, WindowGeometry, WindowManager,
};
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn aidl_parser_and_generator_cover_interface_methods() {
    let source = "package demo;\ninterface IThing {\n int add(in int left, out String result);\n}";
    let file = parse(source).unwrap();
    assert_eq!(file.package.as_deref(), Some("demo"));
    assert_eq!(
        file.interfaces[0].methods[0].parameters[0].direction,
        ParameterDirection::In
    );
    let generated = generate_rust(&file);
    assert!(generated.contains("pub trait IThing"));
    assert!(generated.contains("pub struct IThingStub"));
}

#[test]
fn parcel_and_mock_binder_round_trip_values() {
    let binder = MockBinder::new();
    binder.register(7, |mut data| {
        let mut reply = Parcel::new();
        reply.write_i32(data.read_i32().unwrap() + 1);
        reply
    });
    let mut request = Parcel::new();
    request.write_i32(41);
    let mut reply = binder.transact(7, request);
    assert_eq!(reply.read_i32(), Some(42));
    assert_eq!(binder.transact(99, Parcel::new()).len(), 0);
}

#[test]
fn core_managers_track_manifest_activity_and_geometry() {
    let mut packages = PackageManager::new();
    let package = packages.parse_manifest(r#"<manifest package="demo.app" versionCode="3"><uses-permission android:name="android.permission.INTERNET"/></manifest>"#).unwrap();
    assert_eq!(package.version_code, 3);
    assert!(packages.has_permission("demo.app", "android.permission.INTERNET"));

    let mut activities = ActivityManager::new();
    let first = activities.start("demo.app", "MainActivity");
    let second = activities.start("demo.app", "DetailsActivity");
    assert_eq!(activities.stack()[0].state, ActivityState::Paused);
    assert_eq!(activities.top().unwrap().id, second);
    assert!(activities.finish(second));
    assert_eq!(activities.top().unwrap().id, first);
    assert_eq!(activities.top().unwrap().state, ActivityState::Resumed);

    let mut windows = WindowManager::new();
    windows.create(
        1,
        WindowGeometry {
            x: 0,
            y: 0,
            width: 100,
            height: 200,
            dpi: 160,
        },
    );
    windows.resize(1, 300, 400).unwrap();
    assert_eq!(windows.geometry(1).unwrap().width, 300);
}

#[test]
fn art_runtime_reports_optional_host_capabilities() {
    let mut art = ArtRuntime::with_config(ArtConfig {
        libart: Some(PathBuf::from("/tmp/libart.so")),
        dex2oat: None,
        classpath: Vec::new(),
    });
    assert!(art.is_available());
    assert!(!art.is_initialized());
    assert!(matches!(art.initialize(), Err(ArtError::Unavailable)));
    assert!(!art.is_initialized());
    let unavailable = ArtRuntime::with_config(ArtConfig {
        libart: None,
        dex2oat: None,
        classpath: Vec::new(),
    });
    assert!(matches!(
        unavailable.clone().initialize(),
        Err(ArtError::Unavailable)
    ));
}

#[test]
fn process_art_backend_executes_dex2oat_contract() {
    let dex = std::env::temp_dir().join(format!("lar-art-dex-{}.dex", std::process::id()));
    let output = std::env::temp_dir().join(format!("lar-art-out-{}", std::process::id()));
    std::fs::write(&dex, [0u8; 8]).unwrap();
    let mut art = ArtRuntime::with_config(ArtConfig {
        libart: None,
        dex2oat: Some(PathBuf::from("/bin/true")),
        classpath: vec![PathBuf::from("/system/framework/core.jar")],
    })
    .with_backend(Arc::new(ProcessArtBackend::new(&output)));
    art.initialize().unwrap();
    art.start_application("demo.app", Some(&dex)).unwrap();
    std::fs::remove_file(dex).unwrap();
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn runtime_exposes_phase3_components() {
    let runtime = lar::LarRuntime::new();
    assert_eq!(runtime.package_manager.len(), 0);
    assert_eq!(runtime.window_manager.len(), 0);
    assert!(!runtime.is_phase3_ready());
    let _ = Arc::new(runtime.binder.clone());
}

#[test]
fn shared_ring_buffer_preserves_fifo_and_backpressure() {
    let ring = SharedRingBuffer::new(1).unwrap();
    assert_eq!(ring.capacity(), 1);
    assert!(ring.push(10).is_ok());
    assert_eq!(ring.push(20), Err(20));
    assert_eq!(ring.pop(), Some(10));
    assert_eq!(ring.pop(), None);
}
