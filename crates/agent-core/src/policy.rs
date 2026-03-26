//! `PolicyEngine` — security policy enforcement for tool calls,
//! skill loading, file access, and network requests.

use mlx_agent_skills::{SkillPackage, TrustLevel};
use mlx_agent_tools::ExecutionMode;
use serde::Deserialize;

/// A decision made by the policy engine.
#[derive(Debug, Clone)]
pub enum PolicyDecision {
    /// The action is allowed to proceed.
    Allow,
    /// The action is denied with a reason.
    Deny { reason: String },
    /// The action requires user approval before proceeding.
    Ask {
        prompt: String,
        approval_id: String,
    },
}

/// Configuration for the policy engine.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub default_mode: ExecutionMode,
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub tool_denylist: Vec<String>,
    #[serde(default)]
    pub exec_safe_bins: Vec<String>,
    #[serde(default)]
    pub exec_deny_patterns: Vec<String>,
    #[serde(default)]
    pub file_deny_paths: Vec<String>,
    #[serde(default)]
    pub network_allow_domains: Vec<String>,
    #[serde(default)]
    pub min_trust_level: TrustLevel,
    #[serde(default)]
    pub require_capabilities: bool,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            default_mode: ExecutionMode::Full,
            tool_allowlist: Vec::new(),
            tool_denylist: Vec::new(),
            exec_safe_bins: vec![
                "ls".into(),
                "cat".into(),
                "grep".into(),
                "git".into(),
                "curl".into(),
            ],
            exec_deny_patterns: vec![
                "rm -rf /".into(),
                "sudo".into(),
                "chmod 777".into(),
            ],
            file_deny_paths: vec![
                "~/.ssh/".into(),
                "~/.aws/".into(),
                "~/.gnupg/".into(),
            ],
            network_allow_domains: Vec::new(),
            min_trust_level: TrustLevel::Unknown,
            require_capabilities: false,
        }
    }
}

/// Trait for policy enforcement.
///
/// The concrete implementation will be built in Phase 3 (Security).
#[async_trait::async_trait]
pub trait PolicyEngine: Send + Sync {
    /// Check if a tool call is allowed for the active skill.
    async fn check_tool_call(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
        skill: Option<&SkillPackage>,
        mode: ExecutionMode,
    ) -> PolicyDecision;

    /// Check if a skill can be loaded (trust, requirements, capabilities).
    async fn check_skill_load(&self, skill: &SkillPackage) -> PolicyDecision;

    /// Check if a file path access is allowed.
    fn check_file_access(&self, path: &std::path::Path, write: bool) -> PolicyDecision;

    /// Check if a network request is allowed.
    fn check_network(&self, url: &str, method: &str) -> PolicyDecision;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_has_safe_bins() {
        let config = PolicyConfig::default();
        assert!(config.exec_safe_bins.contains(&"git".to_string()));
        assert!(config.exec_safe_bins.contains(&"cat".to_string()));
    }

    #[test]
    fn default_policy_denies_dangerous_paths() {
        let config = PolicyConfig::default();
        assert!(config.file_deny_paths.iter().any(|p| p.contains(".ssh")));
    }
}
