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
    Ask { prompt: String, approval_id: String },
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
            exec_deny_patterns: vec!["rm -rf /".into(), "sudo".into(), "chmod 777".into()],
            file_deny_paths: vec!["~/.ssh/".into(), "~/.aws/".into(), "~/.gnupg/".into()],
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

/// A concrete implementation of `PolicyEngine` backed by a `PolicyConfig`.
pub struct DefaultPolicyEngine {
    config: PolicyConfig,
}

impl DefaultPolicyEngine {
    pub fn new(config: PolicyConfig) -> Self {
        Self { config }
    }
}

#[async_trait::async_trait]
impl PolicyEngine for DefaultPolicyEngine {
    async fn check_tool_call(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
        skill: Option<&SkillPackage>,
        mode: ExecutionMode,
    ) -> PolicyDecision {
        if mode == ExecutionMode::Locked {
            return PolicyDecision::Deny {
                reason: "Execution mode is Locked".into(),
            };
        }

        if !self.config.tool_allowlist.is_empty()
            && !self.config.tool_allowlist.contains(&tool_name.to_string())
        {
            return PolicyDecision::Deny {
                reason: format!("Tool {} is not in the allowlist", tool_name),
            };
        }

        if self.config.tool_denylist.contains(&tool_name.to_string()) {
            return PolicyDecision::Deny {
                reason: format!("Tool {} is in the denylist", tool_name),
            };
        }

        // Enforce capabilities if a skill is executing the tool
        if let Some(sk) = skill {
            let caps = &sk.capabilities;

            // Example mapping:
            // "run_command" requires `exec`
            if tool_name == "run_command" && !caps.exec {
                return PolicyDecision::Deny {
                    reason: format!("Skill {} does not have 'exec' capability", sk.name),
                };
            }

            // "read_file", "write_file", "edit_file", "list_dir" require `filesystem`
            if (tool_name == "read_file"
                || tool_name == "write_file"
                || tool_name == "edit_file"
                || tool_name == "list_dir")
                && caps.filesystem == mlx_agent_skills::FilesystemScope::None
            {
                return PolicyDecision::Deny {
                    reason: format!("Skill {} does not have 'filesystem' capability", sk.name),
                };
            }

            // "search_web", "fetch_url" require `network`
            if (tool_name == "search_web" || tool_name == "fetch_url")
                && caps.network == mlx_agent_skills::NetworkScope::None
            {
                return PolicyDecision::Deny {
                    reason: format!("Skill {} does not have 'network' capability", sk.name),
                };
            }
        } else if self.config.require_capabilities {
            // Strict mode: tools cannot be run outside of a skill context, unless we allow basic agent tools
            // We can ask for approval if uncertain, but for MVP let's just Ask for `run_command`
            if tool_name == "run_command" {
                let cmd = params
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                return PolicyDecision::Ask {
                    prompt: format!("The agent wants to run a command: `{}`", cmd),
                    approval_id: uuid::Uuid::new_v4().to_string(),
                };
            }
        }

        // Catch-all: Always ask for approval for `run_command` regardless of skill capability (defense in depth),
        // unless it's in `safe_bins`.
        if tool_name == "run_command" {
            let cmd = params.get("command").and_then(|v| v.as_str()).unwrap_or("");

            // Check deny patterns
            for deny in &self.config.exec_deny_patterns {
                if cmd.contains(deny) {
                    return PolicyDecision::Deny {
                        reason: format!("Command matches deny pattern: {}", deny),
                    };
                }
            }

            // Check safe bins (simple prefix check)
            let is_safe = self
                .config
                .exec_safe_bins
                .iter()
                .any(|bin| cmd == bin || cmd.starts_with(&format!("{} ", bin)));

            if !is_safe {
                return PolicyDecision::Ask {
                    prompt: format!(
                        "The agent wants to run a potentially unsafe command: `{}`",
                        cmd
                    ),
                    approval_id: uuid::Uuid::new_v4().to_string(),
                };
            }
        }

        PolicyDecision::Allow
    }

    async fn check_skill_load(&self, skill: &SkillPackage) -> PolicyDecision {
        // Enforce trust level.
        if (skill.trust_level as u8) < (self.config.min_trust_level as u8) {
            return PolicyDecision::Deny {
                reason: format!(
                    "Skill trust level {:?} is below minimum {:?}",
                    skill.trust_level, self.config.min_trust_level
                ),
            };
        }

        PolicyDecision::Allow
    }

    fn check_file_access(&self, path: &std::path::Path, _write: bool) -> PolicyDecision {
        let path_str = path.to_string_lossy();

        // Very basic deny paths checking
        for deny_path in &self.config.file_deny_paths {
            // Remove `~/` prefix for simple substring matching if necessary,
            // though proper expansion is better done elsewhere.
            let clean_deny = deny_path.trim_start_matches("~/");
            if path_str.contains(clean_deny) {
                return PolicyDecision::Deny {
                    reason: format!("Path matches deny list: {}", deny_path),
                };
            }
        }

        PolicyDecision::Allow
    }

    fn check_network(&self, url: &str, _method: &str) -> PolicyDecision {
        if !self.config.network_allow_domains.is_empty() {
            let allowed = self
                .config
                .network_allow_domains
                .iter()
                .any(|domain| url.contains(domain));
            if !allowed {
                return PolicyDecision::Deny {
                    reason: format!("URL {} is not in allowed domains", url),
                };
            }
        }
        PolicyDecision::Allow
    }
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
