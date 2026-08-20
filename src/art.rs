use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;

use crate::api_policy::{ApiPolicy, ApiPolicyError, BundleTierMetadata};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtConfig {
    pub libart: Option<PathBuf>,
    pub dex2oat: Option<PathBuf>,
    pub classpath: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidRuntimeBundle {
    pub root: PathBuf,
    pub libart: PathBuf,
    pub dex2oat: PathBuf,
    pub core_oj: PathBuf,
    pub core_libart: PathBuf,
    pub framework: PathBuf,
    pub metadata: BundleTierMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBundleCatalog {
    pub root: PathBuf,
    pub metadata: Vec<BundleTierMetadata>,
}

impl RuntimeBundleCatalog {
    pub fn load(root: impl Into<PathBuf>, manifest: impl AsRef<Path>) -> Result<Self, ArtError> {
        let root = root.into();
        let contents = fs::read_to_string(manifest).map_err(ArtError::Io)?;
        let metadata = ApiPolicy::default()
            .resolve_manifest(&contents)
            .map_err(ArtError::Policy)?;
        Ok(Self { root, metadata })
    }

    pub fn resolve_for_apk(
        &self,
        min_sdk: Option<u32>,
        target_sdk: Option<u32>,
    ) -> Result<AndroidRuntimeBundle, ArtError> {
        let policy = ApiPolicy::default();
        let metadata = policy
            .resolve_for_apk(&self.metadata, min_sdk, target_sdk)
            .map_err(ArtError::Policy)?;
        let root = self.root.join(format!("android{}", metadata.api.0 - 20));
        AndroidRuntimeBundle::discover_for_policy(root, &policy, metadata.clone())
    }
}

impl AndroidRuntimeBundle {
    pub fn discover(root: impl Into<PathBuf>) -> Result<Self, ArtError> {
        Self::discover_with_metadata(
            root,
            BundleTierMetadata::new(36, "unversioned", Some("local".into()), "ready"),
        )
    }

    pub fn discover_with_metadata(
        root: impl Into<PathBuf>,
        metadata: BundleTierMetadata,
    ) -> Result<Self, ArtError> {
        let root = root.into();
        ApiPolicy::default()
            .validate(&metadata)
            .map_err(ArtError::Policy)?;
        let dex2oat = [
            root.join("system/apex/com.android.art/bin/dex2oat"),
            root.join("system/apex/com.android.art/bin/dex2oat64"),
            root.join("apex/com.android.art/bin/dex2oat"),
            root.join("apex/com.android.art/bin/dex2oat64"),
            root.join("system/bin/dex2oat"),
            root.join("system/bin/dex2oat64"),
        ];
        let candidates = [
            (
                root.join("system/apex/com.android.art/lib64/libart.so"),
                root.join("system/apex/com.android.art/javalib/core-oj.jar"),
                root.join("system/apex/com.android.art/javalib/core-libart.jar"),
            ),
            (
                root.join("apex/com.android.art/lib64/libart.so"),
                root.join("apex/com.android.art/javalib/core-oj.jar"),
                root.join("apex/com.android.art/javalib/core-libart.jar"),
            ),
            (
                root.join("system/lib64/libart.so"),
                root.join("system/framework/core-oj.jar"),
                root.join("system/framework/core-libart.jar"),
            ),
        ];
        for (libart, core_oj, core_libart) in candidates {
            let framework = root.join("system/framework/framework.jar");
            if let Some(dex2oat) = dex2oat.iter().find(|path| path.is_file())
                && [&libart, &core_oj, &core_libart, &framework]
                    .iter()
                    .all(|path| path.is_file())
            {
                return Ok(Self {
                    root,
                    libart,
                    dex2oat: dex2oat.clone(),
                    core_oj,
                    core_libart,
                    framework,
                    metadata,
                });
            }
        }
        Err(ArtError::InvalidBundle)
    }

    pub fn discover_for_policy(
        root: impl Into<PathBuf>,
        policy: &ApiPolicy,
        metadata: BundleTierMetadata,
    ) -> Result<Self, ArtError> {
        let root = root.into();
        policy.validate(&metadata).map_err(ArtError::Policy)?;
        Self::discover_with_metadata(root, metadata)
    }

    pub fn art_config(&self) -> ArtConfig {
        ArtConfig {
            libart: Some(self.libart.clone()),
            dex2oat: Some(self.dex2oat.clone()),
            classpath: vec![
                self.core_oj.clone(),
                self.core_libart.clone(),
                self.framework.clone(),
            ],
        }
    }
}

#[derive(Clone)]
pub struct ArtRuntime {
    config: ArtConfig,
    initialized: bool,
    backend: Option<Arc<dyn ArtBackend>>,
}

impl std::fmt::Debug for ArtRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtRuntime")
            .field("config", &self.config)
            .field("initialized", &self.initialized)
            .finish()
    }
}

impl PartialEq for ArtRuntime {
    fn eq(&self, other: &Self) -> bool {
        self.config == other.config && self.initialized == other.initialized
    }
}

impl Eq for ArtRuntime {}

pub trait ArtBackend: Send + Sync {
    fn initialize(&self, config: &ArtConfig) -> Result<(), ArtError>;
    fn start_application(&self, package: &str, dex: Option<&Path>) -> Result<(), ArtError>;
}

#[derive(Debug, Default)]
pub struct ProcessArtBackend {
    dex2oat: Mutex<Option<PathBuf>>,
    output_root: PathBuf,
}

impl ProcessArtBackend {
    pub fn new(output_root: impl Into<PathBuf>) -> Self {
        Self {
            dex2oat: Mutex::new(None),
            output_root: output_root.into(),
        }
    }
}

impl ArtBackend for ProcessArtBackend {
    fn initialize(&self, config: &ArtConfig) -> Result<(), ArtError> {
        let dex2oat = config.dex2oat.clone().ok_or(ArtError::Unavailable)?;
        if !dex2oat.is_file() {
            return Err(ArtError::Unavailable);
        }
        fs::create_dir_all(&self.output_root).map_err(ArtError::Io)?;
        *self.dex2oat.lock().unwrap() = Some(dex2oat);
        Ok(())
    }

    fn start_application(&self, package: &str, dex: Option<&Path>) -> Result<(), ArtError> {
        let dex = dex.ok_or(ArtError::MissingDex)?;
        let dex2oat = self
            .dex2oat
            .lock()
            .unwrap()
            .clone()
            .ok_or(ArtError::NotInitialized)?;
        let oat = self.output_root.join(format!("{package}.odex"));
        let status = Command::new(dex2oat)
            .arg(format!("--dex-file={}", dex.display()))
            .arg(format!("--oat-file={}", oat.display()))
            .arg("--instruction-set=arm64")
            .arg("--compiler-filter=speed-profile")
            .status()
            .map_err(ArtError::Io)?;
        if status.success() {
            Ok(())
        } else {
            Err(ArtError::CommandFailed)
        }
    }
}

#[derive(Debug, Default)]
pub struct FakeArtBackend {
    started: std::sync::Mutex<Vec<String>>,
}

impl FakeArtBackend {
    pub fn started_packages(&self) -> Vec<String> {
        self.started.lock().unwrap().clone()
    }
}

impl ArtBackend for FakeArtBackend {
    fn initialize(&self, _config: &ArtConfig) -> Result<(), ArtError> {
        Ok(())
    }
    fn start_application(&self, package: &str, _dex: Option<&Path>) -> Result<(), ArtError> {
        self.started.lock().unwrap().push(package.to_owned());
        Ok(())
    }
}

impl ArtRuntime {
    pub fn discover() -> Self {
        Self::with_config(ArtConfig {
            libart: find_library("libart.so"),
            dex2oat: find_executable("dex2oat"),
            classpath: Vec::new(),
        })
    }

    pub fn with_config(config: ArtConfig) -> Self {
        Self {
            config,
            initialized: false,
            backend: None,
        }
    }

    pub fn from_bundle(bundle: &AndroidRuntimeBundle) -> Self {
        Self::with_config(bundle.art_config())
    }

    pub fn initialize(&mut self) -> Result<(), ArtError> {
        if let Some(backend) = &self.backend {
            backend.initialize(&self.config)?;
            self.initialized = true;
            return Ok(());
        }
        if self
            .config
            .libart
            .as_ref()
            .is_none_or(|path| !path.is_file())
            || self
                .config
                .dex2oat
                .as_ref()
                .is_none_or(|path| !path.is_file())
            || self.config.classpath.is_empty()
            || self.config.classpath.iter().any(|path| !path.is_file())
        {
            return Err(ArtError::Unavailable);
        }
        self.initialized = true;
        Ok(())
    }

    pub fn with_backend(mut self, backend: Arc<dyn ArtBackend>) -> Self {
        self.backend = Some(backend);
        self
    }

    pub fn start_application(&self, package: &str, dex: Option<&Path>) -> Result<(), ArtError> {
        if !self.initialized {
            return Err(ArtError::NotInitialized);
        }
        self.backend
            .as_ref()
            .ok_or(ArtError::Unavailable)?
            .start_application(package, dex)
    }

    pub fn is_available(&self) -> bool {
        self.backend.is_some() || self.config.libart.is_some() || self.config.dex2oat.is_some()
    }
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
    pub fn config(&self) -> &ArtConfig {
        &self.config
    }

    pub fn dex2oat_version(&self) -> Result<String, ArtError> {
        let path = self.config.dex2oat.as_ref().ok_or(ArtError::Unavailable)?;
        let output = Command::new(path)
            .arg("--version")
            .output()
            .map_err(ArtError::Io)?;
        if !output.status.success() {
            return Err(ArtError::CommandFailed);
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

#[derive(Debug)]
pub enum ArtError {
    Unavailable,
    InvalidBundle,
    NotInitialized,
    CommandFailed,
    Io(std::io::Error),
    Policy(ApiPolicyError),
    MissingDex,
}
impl std::fmt::Display for ArtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ART error: {:?}", self)
    }
}
impl std::error::Error for ArtError {}

fn find_executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|p| p.join(name))
            .find(|p| p.is_file())
    })
}
fn find_library(name: &str) -> Option<PathBuf> {
    ["/system/lib64", "/system/lib", "/usr/lib", "/usr/lib64"]
        .iter()
        .map(Path::new)
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}
