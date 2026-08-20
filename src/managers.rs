use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    pub name: String,
    pub version_code: u64,
    pub permissions: HashSet<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationInfo {
    pub package: String,
    pub launcher_activity: Option<String>,
    pub dex_path: Option<PathBuf>,
    pub dex: Option<crate::dex::DexMetadata>,
    pub native_libraries: Vec<String>,
}
use std::sync::{Arc, Mutex};

#[derive(Debug, Default, Clone)]
pub struct PackageManager {
    state: Arc<Mutex<PackageManagerState>>,
}
#[derive(Debug, Default)]
struct PackageManagerState {
    packages: HashMap<String, PackageInfo>,
    applications: HashMap<String, ApplicationInfo>,
}
impl PackageManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn install(&mut self, info: PackageInfo) {
        self.state
            .lock()
            .unwrap()
            .packages
            .insert(info.name.clone(), info);
    }
    pub fn install_application(&mut self, application: ApplicationInfo) {
        let mut state = self.state.lock().unwrap();
        state
            .packages
            .entry(application.package.clone())
            .or_insert(PackageInfo {
                name: application.package.clone(),
                version_code: 1,
                permissions: HashSet::new(),
            });
        state
            .applications
            .insert(application.package.clone(), application);
    }
    pub fn application(&self, package: &str) -> Option<ApplicationInfo> {
        self.state
            .lock()
            .unwrap()
            .applications
            .get(package)
            .cloned()
    }
    pub fn parse_manifest(&mut self, manifest: &str) -> Result<PackageInfo, ManagerError> {
        let name = attribute(manifest, "package").ok_or(ManagerError::InvalidManifest)?;
        let version_code = attribute(manifest, "versionCode")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let permissions = manifest
            .lines()
            .filter_map(|line| attribute(line, "name"))
            .filter(|name| name.contains("permission") || name.starts_with("android."))
            .collect();
        let info = PackageInfo {
            name,
            version_code,
            permissions,
        };
        self.install(info.clone());
        Ok(info)
    }
    pub fn get(&self, name: &str) -> Option<PackageInfo> {
        self.state.lock().unwrap().packages.get(name).cloned()
    }
    pub fn has_permission(&self, package: &str, permission: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .packages
            .get(package)
            .is_some_and(|p| p.permissions.contains(permission))
    }
    pub fn len(&self) -> usize {
        self.state.lock().unwrap().packages.len()
    }
    pub fn is_empty(&self) -> bool {
        self.state.lock().unwrap().packages.is_empty()
    }
}

fn attribute(value: &str, key: &str) -> Option<String> {
    let marker = format!("{}=\"", key);
    let start = value.find(&marker)? + marker.len();
    Some(value[start..].split('"').next()?.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Created,
    Resumed,
    Paused,
    Destroyed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityRecord {
    pub id: u64,
    pub package: String,
    pub name: String,
    pub state: ActivityState,
}
#[derive(Debug, Default, Clone)]
pub struct ActivityManager {
    state: Arc<Mutex<ActivityManagerState>>,
}
#[derive(Debug)]
struct ActivityManagerState {
    next_id: u64,
    stack: Vec<ActivityRecord>,
}
impl Default for ActivityManagerState {
    fn default() -> Self {
        Self {
            next_id: 1,
            stack: Vec::new(),
        }
    }
}
impl ActivityManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn start(&mut self, package: impl Into<String>, name: impl Into<String>) -> u64 {
        let mut state = self.state.lock().unwrap();
        if let Some(top) = state.stack.last_mut()
            && top.state == ActivityState::Resumed
        {
            top.state = ActivityState::Paused;
        }
        let id = state.next_id;
        state.next_id += 1;
        state.stack.push(ActivityRecord {
            id,
            package: package.into(),
            name: name.into(),
            state: ActivityState::Resumed,
        });
        id
    }
    pub fn finish(&mut self, id: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some(index) = state.stack.iter().position(|a| a.id == id) else {
            return false;
        };
        state.stack[index].state = ActivityState::Destroyed;
        state.stack.remove(index);
        if let Some(top) = state.stack.last_mut() {
            top.state = ActivityState::Resumed;
        }
        true
    }
    pub fn top(&self) -> Option<ActivityRecord> {
        self.state.lock().unwrap().stack.last().cloned()
    }
    pub fn stack(&self) -> Vec<ActivityRecord> {
        self.state.lock().unwrap().stack.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}
#[derive(Debug, Default, Clone)]
pub struct WindowManager {
    state: Arc<Mutex<HashMap<u64, WindowGeometry>>>,
}
impl WindowManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn create(&mut self, id: u64, geometry: WindowGeometry) {
        self.state.lock().unwrap().insert(id, geometry);
    }
    pub fn resize(&mut self, id: u64, width: u32, height: u32) -> Result<(), ManagerError> {
        let mut state = self.state.lock().unwrap();
        let window = state.get_mut(&id).ok_or(ManagerError::UnknownWindow)?;
        window.width = width;
        window.height = height;
        Ok(())
    }
    pub fn geometry(&self, id: u64) -> Option<WindowGeometry> {
        self.state.lock().unwrap().get(&id).copied()
    }
    pub fn len(&self) -> usize {
        self.state.lock().unwrap().len()
    }
    pub fn is_empty(&self) -> bool {
        self.state.lock().unwrap().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerError {
    InvalidManifest,
    UnknownWindow,
}
impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "manager error: {:?}", self)
    }
}
impl std::error::Error for ManagerError {}
