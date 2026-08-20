use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApiTier {
    Unsupported,
    Secondary,
    Primary,
}

impl fmt::Display for ApiTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primary => f.write_str("primary"),
            Self::Secondary => f.write_str("secondary"),
            Self::Unsupported => f.write_str("unsupported"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AndroidApi(pub u32);

impl AndroidApi {
    pub const API_35: Self = Self(35);
    pub const API_36: Self = Self(36);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleTierMetadata {
    pub api: AndroidApi,
    pub tier: ApiTier,
    pub tag: String,
    pub digest: Option<String>,
    pub status: String,
}

impl BundleTierMetadata {
    pub fn from_manifest_line(line: &str) -> Result<Self, ApiPolicyError> {
        let mut fields = line.split('\t');
        let api = fields
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or(ApiPolicyError::InvalidManifest)?;
        let _android = fields.next().ok_or(ApiPolicyError::InvalidManifest)?;
        let tag = fields.next().ok_or(ApiPolicyError::InvalidManifest)?;
        let digest = fields.next().ok_or(ApiPolicyError::InvalidManifest)?;
        let tier = fields.next().ok_or(ApiPolicyError::InvalidManifest)?;
        let status = fields.next().ok_or(ApiPolicyError::InvalidManifest)?;
        let tier = match tier {
            "primary" => ApiTier::Primary,
            "secondary" => ApiTier::Secondary,
            "unsupported" => ApiTier::Unsupported,
            _ => return Err(ApiPolicyError::InvalidManifest),
        };
        Ok(Self {
            api: AndroidApi(api),
            tier,
            tag: tag.to_owned(),
            digest: (digest != "-").then(|| digest.to_owned()),
            status: status.to_owned(),
        })
    }
}

impl BundleTierMetadata {
    pub fn new(
        api: u32,
        tag: impl Into<String>,
        digest: Option<String>,
        status: impl Into<String>,
    ) -> Self {
        Self {
            api: AndroidApi(api),
            tier: ApiPolicy::default().classify(api),
            tag: tag.into(),
            digest,
            status: status.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiPolicy {
    pub primary: AndroidApi,
    pub secondary: AndroidApi,
}

impl Default for ApiPolicy {
    fn default() -> Self {
        Self {
            primary: AndroidApi::API_36,
            secondary: AndroidApi::API_35,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiPolicyError {
    InvalidManifest,
    UnsupportedApi(AndroidApi),
    TierMismatch {
        api: AndroidApi,
        expected: ApiTier,
        actual: ApiTier,
    },
    BundleNotReady(AndroidApi),
    MissingDigest(AndroidApi),
    NoSupportedBundle,
    ApkMinSdkTooHigh {
        min_sdk: u32,
        api: AndroidApi,
    },
    ApkTargetSdkTooHigh {
        target_sdk: u32,
        api: AndroidApi,
    },
}

impl ApiPolicy {
    pub fn resolve_manifest(
        &self,
        manifest: &str,
    ) -> Result<Vec<BundleTierMetadata>, ApiPolicyError> {
        manifest
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with("api\t"))
            .map(BundleTierMetadata::from_manifest_line)
            .collect()
    }

    pub fn classify(self, api: u32) -> ApiTier {
        match AndroidApi(api) {
            value if value == self.primary => ApiTier::Primary,
            value if value == self.secondary => ApiTier::Secondary,
            _ => ApiTier::Unsupported,
        }
    }

    pub fn validate(&self, metadata: &BundleTierMetadata) -> Result<(), ApiPolicyError> {
        let expected = self.classify(metadata.api.0);
        if expected == ApiTier::Unsupported {
            return Err(ApiPolicyError::UnsupportedApi(metadata.api));
        }
        if metadata.tier != expected {
            return Err(ApiPolicyError::TierMismatch {
                api: metadata.api,
                expected,
                actual: metadata.tier,
            });
        }
        if metadata.status != "ready" {
            return Err(ApiPolicyError::BundleNotReady(metadata.api));
        }
        if metadata
            .digest
            .as_deref()
            .is_none_or(|digest| digest.is_empty() || digest == "-")
        {
            return Err(ApiPolicyError::MissingDigest(metadata.api));
        }
        Ok(())
    }

    pub fn resolve<'a>(
        &self,
        bundles: &'a [BundleTierMetadata],
    ) -> Result<&'a BundleTierMetadata, ApiPolicyError> {
        let mut valid = Vec::new();
        for bundle in bundles {
            if self.validate(bundle).is_ok() {
                valid.push(bundle);
            }
        }
        valid
            .iter()
            .find(|bundle| bundle.tier == ApiTier::Primary)
            .copied()
            .or_else(|| {
                valid
                    .iter()
                    .find(|bundle| bundle.tier == ApiTier::Secondary)
                    .copied()
            })
            .ok_or(ApiPolicyError::NoSupportedBundle)
    }

    pub fn resolve_for_apk<'a>(
        &self,
        bundles: &'a [BundleTierMetadata],
        min_sdk: Option<u32>,
        target_sdk: Option<u32>,
    ) -> Result<&'a BundleTierMetadata, ApiPolicyError> {
        let mut compatible = Vec::new();
        for bundle in bundles {
            if self.validate(bundle).is_ok()
                && self
                    .check_apk_compatibility(bundle.api, min_sdk, target_sdk)
                    .is_ok()
            {
                compatible.push(bundle);
            }
        }
        compatible
            .iter()
            .find(|bundle| bundle.tier == ApiTier::Primary)
            .copied()
            .or_else(|| {
                compatible
                    .iter()
                    .find(|bundle| bundle.tier == ApiTier::Secondary)
                    .copied()
            })
            .ok_or(ApiPolicyError::NoSupportedBundle)
    }

    pub fn check_apk_compatibility(
        &self,
        api: AndroidApi,
        min_sdk: Option<u32>,
        target_sdk: Option<u32>,
    ) -> Result<(), ApiPolicyError> {
        if self.classify(api.0) == ApiTier::Unsupported {
            return Err(ApiPolicyError::UnsupportedApi(api));
        }
        if let Some(value) = min_sdk.filter(|value| *value > api.0) {
            return Err(ApiPolicyError::ApkMinSdkTooHigh {
                min_sdk: value,
                api,
            });
        }
        if let Some(value) = target_sdk.filter(|value| *value > api.0) {
            return Err(ApiPolicyError::ApkTargetSdkTooHigh {
                target_sdk: value,
                api,
            });
        }
        Ok(())
    }
}
