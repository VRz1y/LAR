use super::{
    ANativeWindow, CpuOverlayManager, GraphicBuffer, GraphicsCapabilities, OverlayError,
    VirtualTouchBackend, VirtualTouchError,
};
use crate::lifecycle::InputEvent;

#[derive(Debug)]
pub struct GraphicsRuntime {
    capabilities: GraphicsCapabilities,
    pub overlay: CpuOverlayManager,
    pub virtual_touch: VirtualTouchBackend,
}

impl GraphicsRuntime {
    pub fn new() -> Self {
        Self {
            capabilities: GraphicsCapabilities::probe(),
            overlay: CpuOverlayManager::new(),
            virtual_touch: VirtualTouchBackend::unsupported(1, 1),
        }
    }
    pub fn apply_overlay(&self, buffer: &GraphicBuffer) -> Result<usize, OverlayError> {
        self.overlay.apply(buffer)
    }
    pub fn configure_virtual_touch(
        &mut self,
        max_x: i32,
        max_y: i32,
    ) -> Result<(), VirtualTouchError> {
        self.virtual_touch.stop();
        self.virtual_touch = VirtualTouchBackend::open(max_x, max_y)?;
        Ok(())
    }
    pub fn emit_virtual_touch(&mut self, event: InputEvent) -> Result<(), VirtualTouchError> {
        self.virtual_touch.emit(event)
    }
    pub fn virtual_touch_supported(&self) -> bool {
        self.virtual_touch.is_supported()
    }
    pub fn capabilities(&self) -> GraphicsCapabilities {
        self.capabilities
    }
    pub fn zero_copy_ready(&self) -> bool {
        self.capabilities.zero_copy_ready()
    }
    pub fn create_native_window(&self) -> ANativeWindow {
        ANativeWindow::new()
    }
    pub fn stop(&mut self) {
        self.virtual_touch.stop();
        self.overlay.clear();
    }
}
impl Default for GraphicsRuntime {
    fn default() -> Self {
        Self::new()
    }
}
