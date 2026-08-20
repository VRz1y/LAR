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
#[derive(Debug, Default)]
pub struct PackageManager {
    packages: HashMap<String, PackageInfo>,
    applications: HashMap<String, ApplicationInfo>,
}
impl PackageManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn install(&mut self, info: PackageInfo) {
        self.packages.insert(info.name.clone(), info);
    }
    pub fn install_application(&mut self, application: ApplicationInfo) {
        self.packages
            .entry(application.package.clone())
            .or_insert(PackageInfo {
                name: application.package.clone(),
                version_code: 1,
                permissions: HashSet::new(),
            });
        self.applications
            .insert(application.package.clone(), application);
    }
    pub fn application(&self, package: &str) -> Option<&ApplicationInfo> {
        self.applications.get(package)
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
    pub fn get(&self, name: &str) -> Option<&PackageInfo> {
        self.packages.get(name)
    }
    pub fn has_permission(&self, package: &str, permission: &str) -> bool {
        self.get(package)
            .is_some_and(|p| p.permissions.contains(permission))
    }
    pub fn len(&self) -> usize {
        self.packages.len()
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
#[derive(Debug, Default)]
pub struct ActivityManager {
    next_id: u64,
    stack: Vec<ActivityRecord>,
}
impl ActivityManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            stack: Vec::new(),
        }
    }
    pub fn start(&mut self, package: impl Into<String>, name: impl Into<String>) -> u64 {
        if let Some(top) = self.stack.last_mut() {
            if top.state == ActivityState::Resumed {
                top.state = ActivityState::Paused;
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.stack.push(ActivityRecord {
            id,
            package: package.into(),
            name: name.into(),
            state: ActivityState::Resumed,
        });
        id
    }
    pub fn finish(&mut self, id: u64) -> bool {
        let Some(index) = self.stack.iter().position(|a| a.id == id) else {
            return false;
        };
        self.stack[index].state = ActivityState::Destroyed;
        self.stack.remove(index);
        if let Some(top) = self.stack.last_mut() {
            top.state = ActivityState::Resumed;
        }
        true
    }
    pub fn top(&self) -> Option<&ActivityRecord> {
        self.stack.last()
    }
    pub fn stack(&self) -> &[ActivityRecord] {
        &self.stack
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
#[derive(Debug, Default)]
pub struct WindowManager {
    windows: HashMap<u64, WindowGeometry>,
}
impl WindowManager {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn create(&mut self, id: u64, geometry: WindowGeometry) {
        self.windows.insert(id, geometry);
    }
    pub fn resize(&mut self, id: u64, width: u32, height: u32) -> Result<(), ManagerError> {
        let window = self
            .windows
            .get_mut(&id)
            .ok_or(ManagerError::UnknownWindow)?;
        window.width = width;
        window.height = height;
        Ok(())
    }
    pub fn geometry(&self, id: u64) -> Option<WindowGeometry> {
        self.windows.get(&id).copied()
    }
    pub fn len(&self) -> usize {
        self.windows.len()
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
