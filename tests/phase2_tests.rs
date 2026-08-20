use lar::LarRuntime;
use lar::audio::{AudioStreamConfig, StreamBackend, StreamDirection};
use lar::graphics::DmaBufPlane;
use lar::graphics::{
    ANativeWindow, GbmAllocator, GraphicsCapabilities, PixelFormat, WaylandConnection, WindowState,
};
use std::os::fd::{AsFd, AsRawFd};

#[test]
fn native_window_lifecycle_is_enforced() {
    let window = ANativeWindow::new();
    assert_eq!(window.state(), WindowState::New);
    assert!(window.dequeue().is_err());
    window.connect().unwrap();
    assert_eq!(window.state(), WindowState::Connected);
    window.disconnect().unwrap();
    assert_eq!(window.state(), WindowState::Disconnected);
    assert!(window.connect().is_err());
}

#[test]
fn graphics_capabilities_are_consistent() {
    let capabilities = GraphicsCapabilities::probe();
    if capabilities.zero_copy_ready() {
        assert!(capabilities.wayland.available);
        assert!(capabilities.gbm.available);
        assert!(capabilities.egl.available || capabilities.vulkan.available);
    }
}

#[test]
fn runtime_exposes_phase2_subsystems() {
    let runtime = LarRuntime::new();
    assert_eq!(
        runtime.graphics.zero_copy_ready(),
        runtime.graphics.capabilities().zero_copy_ready()
    );

    let config = AudioStreamConfig {
        sample_rate: 48_000,
        channels: 2,
        frames_per_buffer: 256,
        direction: StreamDirection::Output,
    };
    let stream = runtime.audio.open_stream(StreamBackend::AAudio, config);
    assert_eq!(
        stream.is_ok(),
        runtime.audio.capabilities().available && runtime.audio.capabilities().has_stream_api
    );
}

#[test]
fn dmabuf_plane_duplicates_descriptor_ownership() {
    let file = std::fs::File::open("/dev/null").unwrap();
    let plane = DmaBufPlane::duplicate(file.as_fd(), 0, 256, 4096, 0).unwrap();
    assert_ne!(plane.fd(), file.as_fd().as_raw_fd());
    assert_eq!(plane.stride, 256);
}

#[test]
fn active_wayland_session_accepts_connection() {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        assert!(WaylandConnection::connect().unwrap().is_connected());
    }
}

#[test]
fn gbm_allocates_exportable_buffer_when_render_node_exists() {
    let Ok(allocator) = GbmAllocator::open_default() else {
        return;
    };
    let buffer = allocator.allocate(64, 64, PixelFormat::Bgra8888).unwrap();
    assert_eq!(buffer.description().width, 64);
    assert!(buffer.planes()[0].fd() >= 0);
}
