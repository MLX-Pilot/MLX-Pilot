//! Skill types: `SkillPackage`, capabilities, requirements, sources,
//! trust levels, and install specifications.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Capabilities ───────────────────────────────────────────────────

/// Declares what a skill is allowed to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCapabilities {
    #[serde(default)]
    pub exec: bool,
    #[serde(default)]
    pub exec_commands: Vec<String>,
    #[serde(default)]
    pub filesystem: FilesystemScope,
    #[serde(default)]
    pub network: NetworkScope,
    #[serde(default)]
    pub network_domains: Vec<String>,
    #[serde(default)]
    pub env_read: Vec<String>,
    #[serde(default)]
    pub secrets: bool,
    #[serde(default)]
    pub spawn: bool,
}

impl Default for SkillCapabilities {
    fn default() -> Self {
        Self {
            exec: false,
            exec_commands: Vec::new(),
            filesystem: FilesystemScope::None,
            network: NetworkScope::None,
            network_domains: Vec::new(),
            env_read: Vec::new(),
            secrets: false,
            spawn: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FilesystemScope {
    Workspace,
    ReadOnly,
    #[default]
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NetworkScope {
    Read,
    ReadWrite,
    #[default]
    None,
}

// ── Requirements ───────────────────────────────────────────────────

/// External dependencies a skill needs to be loaded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillRequirements {
    #[serde(default)]
    pub bins: Vec<String>,
    #[serde(default)]
    pub any_bins: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub config: Vec<String>,
}

// ── Install specification ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallSpec {
    pub id: Option<String>,
    pub kind: InstallKind,
    pub label: Option<String>,
    #[serde(default)]
    pub bins: Vec<String>,
    #[serde(default)]
    pub os: Vec<String>,
    pub formula: Option<String>,
    pub package: Option<String>,
    pub module: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    Brew,
    Node,
    Go,
    Uv,
    Download,
}

// ── Skill package ──────────────────────────────────────────────────

/// A fully parsed skill package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPackage {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default)]
    pub always: bool,
    #[serde(default)]
    pub os: Vec<String>,
    pub source: SkillSource,
    pub file_path: PathBuf,
    pub base_dir: PathBuf,
    pub body: String,
    #[serde(default)]
    pub requires: SkillRequirements,
    #[serde(default)]
    pub capabilities: SkillCapabilities,
    #[serde(default)]
    pub install: Vec<InstallSpec>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub trust_level: TrustLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    Bundled,
    Managed,
    Workspace,
    ClawHub,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    Verified,
    Community,
    Local,
    #[default]
    Unknown,
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_restrictive() {
        let cap = SkillCapabilities::default();
        assert!(!cap.exec);
        assert!(!cap.secrets);
        assert!(!cap.spawn);
        assert_eq!(cap.filesystem, FilesystemScope::None);
        assert_eq!(cap.network, NetworkScope::None);
    }

    #[test]
    fn trust_level_ordering() {
        assert!(TrustLevel::Verified < TrustLevel::Community);
        assert!(TrustLevel::Community < TrustLevel::Local);
        assert!(TrustLevel::Local < TrustLevel::Unknown);
    }

    #[test]
    fn skill_requirements_default_empty() {
        let req = SkillRequirements::default();
        assert!(req.bins.is_empty());
        assert!(req.env.is_empty());
    }
}
