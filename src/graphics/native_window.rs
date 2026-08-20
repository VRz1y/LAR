use super::buffers::GraphicBuffer;
use super::dmabuf::SyncFence;
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    New,
    Connected,
    Disconnected,
}

#[derive(Debug)]
struct WindowInner {
    state: WindowState,
    buffers: VecDeque<QueuedBuffer>,
}

#[derive(Debug)]
pub struct QueuedBuffer {
    pub buffer: GraphicBuffer,
    pub acquire_fence: Option<SyncFence>,
}

#[derive(Clone, Debug)]
pub struct ANativeWindow {
    inner: Arc<Mutex<WindowInner>>,
}

impl ANativeWindow {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(WindowInner {
                state: WindowState::New,
                buffers: VecDeque::with_capacity(3),
            })),
        }
    }
    pub fn connect(&self) -> Result<(), WindowError> {
        self.transition(WindowState::Connected)
    }
    pub fn disconnect(&self) -> Result<(), WindowError> {
        self.transition(WindowState::Disconnected)
    }
    pub fn state(&self) -> WindowState {
        self.inner.lock().unwrap().state
    }
    pub fn set_buffer(
        &self,
        buffer: GraphicBuffer,
        acquire: Option<SyncFence>,
    ) -> Result<(), WindowError> {
        let mut inner = self.inner.lock().map_err(|_| WindowError::Poisoned)?;
        if inner.state != WindowState::Connected {
            return Err(WindowError::InvalidState);
        }
        if inner.buffers.len() == 3 {
            return Err(WindowError::QueueFull);
        }
        inner.buffers.push_back(QueuedBuffer {
            buffer,
            acquire_fence: acquire,
        });
        Ok(())
    }
    pub fn dequeue(&self) -> Result<GraphicBuffer, WindowError> {
        Ok(self.dequeue_with_fence()?.buffer)
    }
    pub fn dequeue_with_fence(&self) -> Result<QueuedBuffer, WindowError> {
        let mut inner = self.inner.lock().map_err(|_| WindowError::Poisoned)?;
        if inner.state != WindowState::Connected {
            return Err(WindowError::InvalidState);
        }
        inner.buffers.pop_front().ok_or(WindowError::NoBuffer)
    }
    fn transition(&self, state: WindowState) -> Result<(), WindowError> {
        let mut inner = self.inner.lock().map_err(|_| WindowError::Poisoned)?;
        match (inner.state, state) {
            (WindowState::New, WindowState::Connected)
            | (WindowState::Connected, WindowState::Disconnected) => {
                inner.state = state;
                Ok(())
            }
            _ => Err(WindowError::InvalidState),
        }
    }
}

impl Default for ANativeWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowError {
    InvalidState,
    NoBuffer,
    Poisoned,
    QueueFull,
}
impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native window error: {:?}", self)
    }
}
impl std::error::Error for WindowError {}
