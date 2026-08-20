use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLifecycle {
    Created,
    Started,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Key { code: u32, pressed: bool },
    Touch { x: i32, y: i32, pressed: bool },
}

#[derive(Debug, Default, Clone)]
pub struct InputDispatcher {
    events: Arc<Mutex<Vec<InputEvent>>>,
}

impl InputDispatcher {
    pub fn push(&self, event: InputEvent) {
        self.events.lock().unwrap().push(event);
    }
    pub fn drain(&self) -> Vec<InputEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
    pub fn is_empty(&self) -> bool {
        self.events.lock().unwrap().is_empty()
    }
}
