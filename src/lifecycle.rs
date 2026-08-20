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

#[derive(Debug, Default)]
pub struct InputDispatcher {
    events: Vec<InputEvent>,
}

impl InputDispatcher {
    pub fn push(&mut self, event: InputEvent) {
        self.events.push(event);
    }
    pub fn drain(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.events)
    }
    pub fn len(&self) -> usize {
        self.events.len()
    }
}
