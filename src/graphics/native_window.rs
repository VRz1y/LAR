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
    available: VecDeque<GraphicBuffer>,
    dequeued: Vec<GraphicBuffer>,
    queued: VecDeque<QueuedBuffer>,
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
                available: VecDeque::with_capacity(3),
                dequeued: Vec::with_capacity(3),
                queued: VecDeque::with_capacity(3),
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
        _acquire: Option<SyncFence>,
    ) -> Result<(), WindowError> {
        let mut inner = self.inner.lock().map_err(|_| WindowError::Poisoned)?;
        if inner.state != WindowState::Connected {
            return Err(WindowError::InvalidState);
        }
        if inner.available.len() + inner.dequeued.len() + inner.queued.len() >= 3 {
            return Err(WindowError::QueueFull);
        }
        inner.available.push_back(buffer);
        Ok(())
    }
    pub fn queue_buffer(
        &self,
        buffer: GraphicBuffer,
        acquire: Option<SyncFence>,
    ) -> Result<(), WindowError> {
        let mut inner = self.inner.lock().map_err(|_| WindowError::Poisoned)?;
        if inner.state != WindowState::Connected {
            return Err(WindowError::InvalidState);
        }
        let Some(index) = inner
            .dequeued
            .iter()
            .position(|candidate| candidate == &buffer)
        else {
            return Err(WindowError::NotDequeued);
        };
        inner.dequeued.swap_remove(index);
        inner.queued.push_back(QueuedBuffer {
            buffer,
            acquire_fence: acquire,
        });
        Ok(())
    }
    pub fn dequeue(&self) -> Result<GraphicBuffer, WindowError> {
        let mut inner = self.inner.lock().map_err(|_| WindowError::Poisoned)?;
        if inner.state != WindowState::Connected {
            return Err(WindowError::InvalidState);
        }
        let buffer = inner.available.pop_front().ok_or(WindowError::NoBuffer)?;
        inner.dequeued.push(buffer.clone());
        Ok(buffer)
    }
    pub fn acquire_queued(&self) -> Result<QueuedBuffer, WindowError> {
        let mut inner = self.inner.lock().map_err(|_| WindowError::Poisoned)?;
        if inner.state != WindowState::Connected {
            return Err(WindowError::InvalidState);
        }
        inner.queued.pop_front().ok_or(WindowError::NoBuffer)
    }
    pub fn release_buffer(
        &self,
        buffer: GraphicBuffer,
        _release: Option<SyncFence>,
    ) -> Result<(), WindowError> {
        let mut inner = self.inner.lock().map_err(|_| WindowError::Poisoned)?;
        if inner.state != WindowState::Connected {
            return Err(WindowError::InvalidState);
        }
        if inner.available.len() + inner.dequeued.len() + inner.queued.len() >= 3 {
            return Err(WindowError::QueueFull);
        }
        inner.available.push_back(buffer);
        Ok(())
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
    NotDequeued,
}
impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "native window error: {:?}", self)
    }
}
impl std::error::Error for WindowError {}
