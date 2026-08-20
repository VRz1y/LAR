use super::{ANativeWindow, GraphicsCapabilities};

#[derive(Debug)]
pub struct GraphicsRuntime {
    capabilities: GraphicsCapabilities,
}

impl GraphicsRuntime {
    pub fn new() -> Self {
        Self {
            capabilities: GraphicsCapabilities::probe(),
        }
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
}

impl Default for GraphicsRuntime {
    fn default() -> Self {
        Self::new()
    }
}
