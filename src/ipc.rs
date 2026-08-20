use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};

use crate::lifecycle::{InputDispatcher, InputEvent};
use crate::managers::{ActivityManager, ActivityState, PackageManager, WindowManager};

#[derive(Debug, Clone, PartialEq)]
pub enum ParcelValue {
    I32(i32),
    I64(i64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    Null,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parcel {
    values: Vec<ParcelValue>,
    position: usize,
}
impl Parcel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn write_i32(&mut self, value: i32) {
        self.values.push(ParcelValue::I32(value));
    }
    pub fn write_i64(&mut self, value: i64) {
        self.values.push(ParcelValue::I64(value));
    }
    pub fn write_bool(&mut self, value: bool) {
        self.values.push(ParcelValue::Bool(value));
    }
    pub fn write_string(&mut self, value: impl Into<String>) {
        self.values.push(ParcelValue::String(value.into()));
    }
    pub fn write_bytes(&mut self, value: &[u8]) {
        self.values.push(ParcelValue::Bytes(value.to_vec()));
    }
    pub fn read(&mut self) -> Option<ParcelValue> {
        let value = self.values.get(self.position).cloned();
        self.position += value.is_some() as usize;
        value
    }
    pub fn read_i32(&mut self) -> Option<i32> {
        match self.read()? {
            ParcelValue::I32(v) => Some(v),
            _ => None,
        }
    }
    pub fn read_i64(&mut self) -> Option<i64> {
        match self.read()? {
            ParcelValue::I64(v) => Some(v),
            _ => None,
        }
    }
    pub fn read_bool(&mut self) -> Option<bool> {
        match self.read()? {
            ParcelValue::Bool(v) => Some(v),
            _ => None,
        }
    }
    pub fn read_string(&mut self) -> Option<String> {
        match self.read()? {
            ParcelValue::String(v) => Some(v),
            _ => None,
        }
    }
    pub fn reset(&mut self) {
        self.position = 0;
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

pub trait Binder: Send + Sync {
    fn transact(&self, code: u32, data: Parcel) -> Parcel;
}
type ParcelHandler = Box<dyn Fn(Parcel) -> Parcel + Send + Sync>;
type ParcelHandlers = Arc<Mutex<HashMap<u32, ParcelHandler>>>;

#[derive(Clone, Default)]
pub struct MockBinder {
    handlers: ParcelHandlers,
}
impl MockBinder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register<F>(&self, code: u32, handler: F)
    where
        F: Fn(Parcel) -> Parcel + Send + Sync + 'static,
    {
        self.handlers
            .lock()
            .unwrap()
            .insert(code, Box::new(handler));
    }
}
impl Binder for MockBinder {
    fn transact(&self, code: u32, data: Parcel) -> Parcel {
        self.handlers
            .lock()
            .unwrap()
            .get(&code)
            .map(|h| h(data))
            .unwrap_or_default()
    }
}

#[derive(Clone, Default)]
pub struct BinderRegistry {
    services: Arc<Mutex<HashMap<String, Arc<dyn Binder>>>>,
}
impl BinderRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&self, name: impl Into<String>, binder: Arc<dyn Binder>) {
        self.services.lock().unwrap().insert(name.into(), binder);
    }
    pub fn get(&self, name: &str) -> Option<Arc<dyn Binder>> {
        self.services.lock().unwrap().get(name).cloned()
    }
    pub fn is_available(&self) -> bool {
        !self.services.lock().unwrap().is_empty() || std::path::Path::new("/dev/binderfs").exists()
    }

    pub fn register_core_services(&self) {
        self.register("activity", Arc::new(MockBinder::new()));
        self.register("package", Arc::new(MockBinder::new()));
        self.register("window", Arc::new(MockBinder::new()));
        self.register("input", Arc::new(MockBinder::new()));
    }

    pub fn register_core_services_with_state(&self, state: Arc<Mutex<CoreServiceState>>) {
        self.register(
            "activity",
            Arc::new(StateBinder::new(state.clone(), activity_transact)),
        );
        self.register(
            "package",
            Arc::new(StateBinder::new(state.clone(), package_transact)),
        );
        self.register(
            "window",
            Arc::new(StateBinder::new(state.clone(), window_transact)),
        );
        self.register("input", Arc::new(StateBinder::new(state, input_transact)));
    }

    pub fn services(&self) -> Vec<String> {
        self.services.lock().unwrap().keys().cloned().collect()
    }
}

#[derive(Debug, Default)]
pub struct CoreServiceState {
    pub package_manager: PackageManager,
    pub activity_manager: ActivityManager,
    pub window_manager: WindowManager,
    pub input_dispatcher: InputDispatcher,
}

impl CoreServiceState {
    pub fn from_managers(
        package_manager: PackageManager,
        activity_manager: ActivityManager,
        window_manager: WindowManager,
        input_dispatcher: InputDispatcher,
    ) -> Self {
        Self {
            package_manager,
            activity_manager,
            window_manager,
            input_dispatcher,
        }
    }
    pub fn new() -> Self {
        Self::default()
    }
}

pub mod transaction {
    pub mod activity {
        pub const START: u32 = 1;
        pub const FINISH: u32 = 2;
        pub const GET_TOP: u32 = 3;
    }

    pub mod package {
        pub const GET_PACKAGE: u32 = 1;
    }

    pub mod window {
        pub const CREATE: u32 = 1;
        pub const RESIZE: u32 = 2;
        pub const GET_GEOMETRY: u32 = 3;
    }

    pub mod input {
        pub const INJECT: u32 = 1;
        pub const DRAIN: u32 = 2;
    }
}

type ServiceHandler = fn(&mut CoreServiceState, u32, Parcel) -> Parcel;

struct StateBinder {
    state: Arc<Mutex<CoreServiceState>>,
    handler: ServiceHandler,
}

impl StateBinder {
    fn new(state: Arc<Mutex<CoreServiceState>>, handler: ServiceHandler) -> Self {
        Self { state, handler }
    }
}

impl Binder for StateBinder {
    fn transact(&self, code: u32, data: Parcel) -> Parcel {
        let mut state = self.state.lock().unwrap();
        (self.handler)(&mut state, code, data)
    }
}

fn activity_transact(state: &mut CoreServiceState, code: u32, mut data: Parcel) -> Parcel {
    let mut reply = Parcel::new();
    match code {
        transaction::activity::START => {
            let (Some(package), Some(name)) = (data.read_string(), data.read_string()) else {
                return reply;
            };
            reply.write_i64(state.activity_manager.start(package, name) as i64);
        }
        transaction::activity::FINISH => {
            let Some(id) = data.read_i64() else {
                return reply;
            };
            reply.write_bool(state.activity_manager.finish(id as u64));
        }
        transaction::activity::GET_TOP => {
            let Some(activity) = state.activity_manager.top() else {
                reply.write_bool(false);
                return reply;
            };
            reply.write_bool(true);
            reply.write_i64(activity.id as i64);
            reply.write_string(&activity.package);
            reply.write_string(&activity.name);
            reply.write_i32(activity_state_code(activity.state));
        }
        _ => {}
    }
    reply
}

fn package_transact(state: &mut CoreServiceState, code: u32, mut data: Parcel) -> Parcel {
    let mut reply = Parcel::new();
    if code != transaction::package::GET_PACKAGE {
        return reply;
    }
    let Some(name) = data.read_string() else {
        return reply;
    };
    let Some(package) = state.package_manager.get(&name) else {
        reply.write_bool(false);
        return reply;
    };
    reply.write_bool(true);
    reply.write_string(&package.name);
    reply.write_i64(package.version_code as i64);
    reply.write_i32(package.permissions.len() as i32);
    for permission in &package.permissions {
        reply.write_string(permission);
    }
    reply
}

fn window_transact(state: &mut CoreServiceState, code: u32, mut data: Parcel) -> Parcel {
    let mut reply = Parcel::new();
    match code {
        transaction::window::CREATE => {
            let (Some(id), Some(x), Some(y), Some(width), Some(height), Some(dpi)) = (
                data.read_i64(),
                data.read_i32(),
                data.read_i32(),
                data.read_i32(),
                data.read_i32(),
                data.read_i32(),
            ) else {
                return reply;
            };
            state.window_manager.create(
                id as u64,
                crate::managers::WindowGeometry {
                    x,
                    y,
                    width: width.max(0) as u32,
                    height: height.max(0) as u32,
                    dpi: dpi.max(0) as u32,
                },
            );
            reply.write_bool(true);
        }
        transaction::window::RESIZE => {
            let (Some(id), Some(width), Some(height)) =
                (data.read_i64(), data.read_i32(), data.read_i32())
            else {
                return reply;
            };
            reply.write_bool(
                state
                    .window_manager
                    .resize(id as u64, width.max(0) as u32, height.max(0) as u32)
                    .is_ok(),
            );
        }
        transaction::window::GET_GEOMETRY => {
            let Some(id) = data.read_i64() else {
                return reply;
            };
            let Some(geometry) = state.window_manager.geometry(id as u64) else {
                reply.write_bool(false);
                return reply;
            };
            reply.write_bool(true);
            reply.write_i32(geometry.x);
            reply.write_i32(geometry.y);
            reply.write_i32(geometry.width as i32);
            reply.write_i32(geometry.height as i32);
            reply.write_i32(geometry.dpi as i32);
        }
        _ => {}
    }
    reply
}

fn input_transact(state: &mut CoreServiceState, code: u32, mut data: Parcel) -> Parcel {
    let mut reply = Parcel::new();
    match code {
        transaction::input::INJECT => {
            let Some(kind) = data.read_i32() else {
                return reply;
            };
            let event = match kind {
                0 => match (data.read_i32(), data.read_bool()) {
                    (Some(code), Some(pressed)) => Some(InputEvent::Key {
                        code: code as u32,
                        pressed,
                    }),
                    _ => None,
                },
                1 => match (data.read_i32(), data.read_i32(), data.read_bool()) {
                    (Some(x), Some(y), Some(pressed)) => Some(InputEvent::Touch { x, y, pressed }),
                    _ => None,
                },
                _ => None,
            };
            let Some(event) = event else {
                return reply;
            };
            state.input_dispatcher.push(event);
            reply.write_bool(true);
        }
        transaction::input::DRAIN => {
            let events = state.input_dispatcher.drain();
            reply.write_i32(events.len() as i32);
            for event in events {
                match event {
                    InputEvent::Key { code, pressed } => {
                        reply.write_i32(0);
                        reply.write_i32(code as i32);
                        reply.write_bool(pressed);
                    }
                    InputEvent::Touch { x, y, pressed } => {
                        reply.write_i32(1);
                        reply.write_i32(x);
                        reply.write_i32(y);
                        reply.write_bool(pressed);
                    }
                }
            }
        }
        _ => {}
    }
    reply
}

fn activity_state_code(state: ActivityState) -> i32 {
    match state {
        ActivityState::Created => 0,
        ActivityState::Resumed => 1,
        ActivityState::Paused => 2,
        ActivityState::Destroyed => 3,
    }
}

pub struct SharedRingBuffer<T> {
    sender: SyncSender<T>,
    receiver: Mutex<Receiver<T>>,
    capacity: usize,
}

impl<T> SharedRingBuffer<T> {
    pub fn new(capacity: usize) -> Result<Self, IpcError> {
        if capacity == 0 {
            return Err(IpcError::InvalidCapacity);
        }
        let (sender, receiver) = mpsc::sync_channel(capacity);
        Ok(Self {
            sender,
            receiver: Mutex::new(receiver),
            capacity,
        })
    }

    pub fn push(&self, value: T) -> Result<(), T> {
        match self.sender.try_send(value) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(value)) | Err(TrySendError::Disconnected(value)) => Err(value),
        }
    }

    pub fn pop(&self) -> Option<T> {
        match self.receiver.lock().unwrap().try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    InvalidCapacity,
}
