//! Skill resolver trait and ClawHub registry types.

use crate::types::{SkillPackage, TrustLevel};
use serde::{Deserialize, Serialize};

/// Metadata about a skill from a registry (e.g. ClawHub).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySkillMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub sha256: String,
    pub author: String,
    pub published_at: String,
    pub download_url: String,
    pub trust_level: TrustLevel,
}

/// Errors that can occur during skill resolution.
#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error("skill not found: {name}")]
    NotFound { name: String },

    #[error("network error: {message}")]
    Network { message: String },

    #[error("integrity check failed for {name}: expected {expected}")]
    IntegrityFailed { name: String, expected: String },

    #[error("parse error: {message}")]
    Parse { message: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Trait for resolving skills from a remote registry.
///
/// The default implementation will be `ClawHubResolver` (Phase 2).
#[async_trait::async_trait]
pub trait SkillResolver: Send + Sync {
    /// Search the registry for skills matching a query.
    async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RegistrySkillMeta>, ResolverError>;

    /// Fetch metadata for a specific skill.
    async fn get(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<RegistrySkillMeta, ResolverError>;

    /// Download a skill archive and return the local path.
    async fn download(
        &self,
        meta: &RegistrySkillMeta,
        dest: &std::path::Path,
    ) -> Result<std::path::PathBuf, ResolverError>;

    /// Verify a downloaded skill against its registry hash.
    async fn verify_integrity(
        &self,
        path: &std::path::Path,
        expected_sha256: &str,
    ) -> Result<bool, ResolverError>;

    /// List all installed skills from this resolver's source.
    async fn list_installed(
        &self,
        managed_dir: &std::path::Path,
    ) -> Result<Vec<SkillPackage>, ResolverError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_skill_meta_serializes() {
        let meta = RegistrySkillMeta {
            name: "weather".into(),
            version: "1.0.0".into(),
            description: "Get weather info".into(),
            sha256: "abc123".into(),
            author: "test".into(),
            published_at: "2026-01-01".into(),
            download_url: "https://example.com/weather.tar.gz".into(),
            trust_level: TrustLevel::Community,
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("weather"));
    }
}
