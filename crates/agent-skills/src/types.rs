//! Skill types: `SkillPackage`, capabilities, requirements, sources,
//! trust levels, and install specifications.

use serde::Deserializer;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Capabilities ───────────────────────────────────────────────────

/// Declares what a skill is allowed to do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCapabilities {
    #[serde(default)]
    pub fs_read: bool,
    #[serde(default)]
    pub fs_write: bool,
    #[serde(default, deserialize_with = "deserialize_network_flag")]
    pub network: bool,
    #[serde(default)]
    pub exec: bool,
    #[serde(default)]
    pub secrets_access: bool,
    // Legacy compat fields from existing SKILL.md metadata.
    #[serde(default)]
    pub exec_commands: Vec<String>,
    #[serde(default)]
    pub filesystem: FilesystemScope,
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
            fs_read: false,
            fs_write: false,
            network: false,
            exec: false,
            secrets_access: false,
            exec_commands: Vec::new(),
            filesystem: FilesystemScope::None,
            network_domains: Vec::new(),
            env_read: Vec::new(),
            secrets: false,
            spawn: false,
        }
    }
}

impl SkillCapabilities {
    /// Effective read permission considering declarative and legacy fields.
    pub fn allows_fs_read(&self) -> bool {
        self.fs_read
            || matches!(
                self.filesystem,
                FilesystemScope::Workspace | FilesystemScope::ReadOnly
            )
    }

    /// Effective write permission considering declarative and legacy fields.
    pub fn allows_fs_write(&self) -> bool {
        self.fs_write || matches!(self.filesystem, FilesystemScope::Workspace)
    }

    /// Effective network permission considering declarative and legacy fields.
    pub fn allows_network(&self) -> bool {
        self.network || !self.network_domains.is_empty()
    }

    /// Effective command execution permission.
    pub fn allows_exec(&self) -> bool {
        self.exec || self.spawn
    }

    /// Effective secrets access permission.
    pub fn allows_secrets_access(&self) -> bool {
        self.secrets_access || self.secrets
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

fn deserialize_network_flag<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NetworkCompat {
        Bool(bool),
        String(String),
    }

    let value = Option::<NetworkCompat>::deserialize(deserializer)?;
    Ok(match value {
        None => false,
        Some(NetworkCompat::Bool(flag)) => flag,
        Some(NetworkCompat::String(raw)) => !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "" | "none" | "off" | "false" | "0"
        ),
    })
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
        assert!(!cap.allows_exec());
        assert!(!cap.allows_secrets_access());
        assert!(!cap.allows_fs_read());
        assert!(!cap.allows_fs_write());
        assert!(!cap.allows_network());
    }

    #[test]
    fn legacy_network_scope_string_maps_to_network_capability() {
        let caps: SkillCapabilities = serde_yaml::from_str(
            r#"
network: readwrite
"#,
        )
        .unwrap();
        assert!(caps.allows_network());

        let none_caps: SkillCapabilities = serde_yaml::from_str(
            r#"
network: none
"#,
        )
        .unwrap();
        assert!(!none_caps.allows_network());
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
