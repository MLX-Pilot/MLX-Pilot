//! `PolicyEngine` — security policy enforcement for tool calls,
//! skill loading, file access, and network requests.

use mlx_agent_skills::{SkillPackage, TrustLevel};
use mlx_agent_tools::ExecutionMode;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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
    pub block_direct_ip_egress: bool,
    #[serde(default)]
    pub airgapped_mode: bool,
    #[serde(default)]
    pub owner_only_mode: bool,
    #[serde(default)]
    pub workspace_root: Option<PathBuf>,
    #[serde(default)]
    pub min_trust_level: TrustLevel,
    #[serde(default)]
    pub require_capabilities: bool,
    #[serde(default)]
    pub skill_sha256_pins: BTreeMap<String, String>,
    #[serde(default)]
    pub known_skill_hashes: BTreeMap<String, String>,
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
            block_direct_ip_egress: true,
            airgapped_mode: false,
            owner_only_mode: false,
            workspace_root: None,
            min_trust_level: TrustLevel::Unknown,
            require_capabilities: false,
            skill_sha256_pins: BTreeMap::new(),
            known_skill_hashes: BTreeMap::new(),
        }
    }
}

/// Trait for policy enforcement.
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

        if mode == ExecutionMode::ReadOnly && is_mutating_tool(tool_name) {
            return PolicyDecision::Deny {
                reason: format!("Tool '{tool_name}' is blocked in read-only mode"),
            };
        }

        if self.config.airgapped_mode && is_network_tool(tool_name) {
            return PolicyDecision::Deny {
                reason: "airgapped mode blocks all network tools".to_string(),
            };
        }

        if !self.config.tool_allowlist.is_empty()
            && !matches_glob_any(&self.config.tool_allowlist, tool_name)
        {
            return PolicyDecision::Deny {
                reason: format!("Tool '{tool_name}' is not allowed by tool_allowlist"),
            };
        }

        if matches_glob_any(&self.config.tool_denylist, tool_name) {
            return PolicyDecision::Deny {
                reason: format!("Tool '{tool_name}' is denied by tool_denylist"),
            };
        }

        if let Some(sk) = skill {
            let caps = &sk.capabilities;

            if tool_name == "exec" && !caps.allows_exec() {
                return PolicyDecision::Deny {
                    reason: format!("Skill '{}' does not allow 'exec'", sk.name),
                };
            }

            if is_fs_read_tool(tool_name) && !caps.allows_fs_read() {
                return PolicyDecision::Deny {
                    reason: format!("Skill '{}' does not allow 'fs_read'", sk.name),
                };
            }

            if is_fs_write_tool(tool_name) && !caps.allows_fs_write() {
                return PolicyDecision::Deny {
                    reason: format!("Skill '{}' does not allow 'fs_write'", sk.name),
                };
            }

            if is_network_tool(tool_name) && !caps.allows_network() {
                return PolicyDecision::Deny {
                    reason: format!("Skill '{}' does not allow 'network'", sk.name),
                };
            }

            if contains_secret_like_params(params) && !caps.allows_secrets_access() {
                return PolicyDecision::Deny {
                    reason: format!("Skill '{}' does not allow 'secrets_access'", sk.name),
                };
            }
        } else if self.config.require_capabilities && requires_capability(tool_name, params) {
            return PolicyDecision::Ask {
                prompt: format!(
                    "Tool '{}' requires explicit skill capabilities, but no active skill is bound",
                    tool_name
                ),
                approval_id: uuid::Uuid::new_v4().to_string(),
            };
        }

        // Catch-all: ask for approval for unsafe command execution.
        if tool_name == "exec" {
            let cmd = params.get("command").and_then(|v| v.as_str()).unwrap_or("");

            for deny in &self.config.exec_deny_patterns {
                if glob_match::glob_match(deny, cmd) || cmd.contains(deny) {
                    return PolicyDecision::Deny {
                        reason: format!("Command matches deny pattern: {}", deny),
                    };
                }
            }

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

        // File tools: deny sensitive paths before execution.
        if let Some(path_str) = params.get("path").and_then(|v| v.as_str()) {
            let decision = self.check_file_access(Path::new(path_str), is_write_tool(tool_name));
            if !matches!(decision, PolicyDecision::Allow) {
                return decision;
            }
        }

        // Network tools: enforce airgap/IP blocking/allowlist.
        if is_network_tool(tool_name)
            && params
                .get("url")
                .and_then(|v| v.as_str())
                .map(|url| !url.trim().is_empty())
                .unwrap_or(false)
        {
            let url = params
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let method = params
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");
            let decision = self.check_network(url, method);
            if !matches!(decision, PolicyDecision::Allow) {
                return decision;
            }
        }

        PolicyDecision::Allow
    }

    async fn check_skill_load(&self, skill: &SkillPackage) -> PolicyDecision {
        if !trust_meets_minimum(skill.trust_level, self.config.min_trust_level) {
            return PolicyDecision::Deny {
                reason: format!(
                    "Skill trust level {:?} is below minimum {:?}",
                    skill.trust_level, self.config.min_trust_level
                ),
            };
        }

        if let Some(pin) = self
            .config
            .skill_sha256_pins
            .get(&skill.name)
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| !v.is_empty())
        {
            let current = skill
                .sha256
                .as_deref()
                .map(|v| v.trim().to_ascii_lowercase())
                .unwrap_or_default();
            if current.is_empty() || current != pin {
                return PolicyDecision::Deny {
                    reason: format!(
                        "skill '{}' integrity pin mismatch (expected {}, got {})",
                        skill.name,
                        pin,
                        if current.is_empty() {
                            "<missing>"
                        } else {
                            &current
                        }
                    ),
                };
            }
        }

        if let Some(previous) = self
            .config
            .known_skill_hashes
            .get(&skill.name)
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| !v.is_empty())
        {
            let current = skill
                .sha256
                .as_deref()
                .map(|v| v.trim().to_ascii_lowercase())
                .unwrap_or_default();
            if !current.is_empty() && current != previous {
                return PolicyDecision::Ask {
                    prompt: format!(
                        "skill '{}' changed hash since last load (old {}, new {})",
                        skill.name, previous, current
                    ),
                    approval_id: uuid::Uuid::new_v4().to_string(),
                };
            }
        }

        PolicyDecision::Allow
    }

    fn check_file_access(&self, path: &std::path::Path, _write: bool) -> PolicyDecision {
        if self.config.owner_only_mode {
            let Some(root) = self.config.workspace_root.as_ref() else {
                return PolicyDecision::Deny {
                    reason: "owner-only mode is enabled but workspace_root is not configured"
                        .to_string(),
                };
            };
            let root = canonical_or_normalize(root);
            let target_abs = if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            };
            let target = canonical_or_normalize(&target_abs);
            if !target.starts_with(&root) {
                return PolicyDecision::Deny {
                    reason: format!(
                        "owner-only mode blocks access outside workspace: {}",
                        path.display()
                    ),
                };
            }
        }

        let path_str = path.to_string_lossy().to_string();
        let path_lower = path_str.to_lowercase();

        for deny_path in &self.config.file_deny_paths {
            let expanded = expand_home(deny_path);
            let expanded_lower = expanded.to_lowercase();
            if glob_match::glob_match(deny_path, &path_str)
                || glob_match::glob_match(deny_path, &path_lower)
                || glob_match::glob_match(&expanded, &path_str)
                || path_lower.contains(&expanded_lower)
                || path_lower.contains(&deny_path.trim_start_matches("~/").to_lowercase())
            {
                return PolicyDecision::Deny {
                    reason: format!("Path matches deny list: {}", deny_path),
                };
            }
        }

        PolicyDecision::Allow
    }

    fn check_network(&self, url: &str, _method: &str) -> PolicyDecision {
        if self.config.airgapped_mode {
            return PolicyDecision::Deny {
                reason: "airgapped mode blocks all outbound network".to_string(),
            };
        }

        let host = extract_host(url);
        if host.is_empty() {
            return PolicyDecision::Deny {
                reason: format!("cannot extract host from URL: {url}"),
            };
        }

        if self.config.block_direct_ip_egress && is_ip_literal(&host) {
            return PolicyDecision::Deny {
                reason: format!("direct IP egress is blocked: {host}"),
            };
        }

        if !self.config.network_allow_domains.is_empty() {
            let allowed = self
                .config
                .network_allow_domains
                .iter()
                .filter_map(|rule| normalize_domain_rule(rule))
                .any(|rule| host_matches_rule(&host, &rule));
            if !allowed {
                return PolicyDecision::Deny {
                    reason: format!("URL {} is not in allowed domains", url),
                };
            }
        }
        PolicyDecision::Allow
    }
}

fn matches_glob_any(patterns: &[String], value: &str) -> bool {
    patterns
        .iter()
        .filter(|pattern| !pattern.trim().is_empty())
        .any(|pattern| glob_match::glob_match(pattern, value))
}

fn is_mutating_tool(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "edit_file" | "exec")
}

fn is_write_tool(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "edit_file")
}

fn is_fs_read_tool(tool_name: &str) -> bool {
    matches!(tool_name, "read_file" | "list_dir")
}

fn is_fs_write_tool(tool_name: &str) -> bool {
    matches!(tool_name, "write_file" | "edit_file")
}

fn is_network_tool(tool_name: &str) -> bool {
    matches!(tool_name, "web_fetch" | "web_search")
}

fn requires_capability(tool_name: &str, params: &serde_json::Value) -> bool {
    tool_name == "exec"
        || is_fs_read_tool(tool_name)
        || is_fs_write_tool(tool_name)
        || is_network_tool(tool_name)
        || contains_secret_like_params(params)
}

fn contains_secret_like_params(value: &serde_json::Value) -> bool {
    const SECRET_KEYS: [&str; 7] = [
        "api_key",
        "token",
        "secret",
        "password",
        "authorization",
        "bearer",
        "credential",
    ];
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, inner)| {
            let lowered = key.to_ascii_lowercase();
            SECRET_KEYS.iter().any(|needle| lowered.contains(needle))
                || contains_secret_like_params(inner)
        }),
        serde_json::Value::Array(items) => items.iter().any(contains_secret_like_params),
        _ => false,
    }
}

fn extract_host(url: &str) -> String {
    let without_scheme = if let Some(idx) = url.find("://") {
        &url[(idx + 3)..]
    } else {
        url
    };
    let host_port = without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .split('@')
        .next_back()
        .unwrap_or_default();

    if let Some(stripped) = host_port.strip_prefix('[') {
        return stripped
            .split(']')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
    }

    host_port
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn normalize_domain_rule(rule: &str) -> Option<String> {
    let trimmed = rule.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = if trimmed.contains("://") {
        extract_host(&trimmed)
    } else {
        trimmed
    };
    if normalized.is_empty() {
        return None;
    }
    Some(normalized.trim_end_matches('.').to_string())
}

fn host_matches_rule(host: &str, rule: &str) -> bool {
    if rule.contains('*') {
        return glob_match::glob_match(rule, host);
    }
    host == rule || host.ends_with(&format!(".{}", rule))
}

fn is_ip_literal(host: &str) -> bool {
    IpAddr::from_str(host).is_ok()
}

fn trust_meets_minimum(skill_level: TrustLevel, minimum: TrustLevel) -> bool {
    if minimum == TrustLevel::Unknown {
        return true;
    }
    skill_level <= minimum
}

fn expand_home(path: &str) -> String {
    if !path.starts_with("~/") {
        return path.to_string();
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if home.is_empty() {
        return path.to_string();
    }
    PathBuf::from(home)
        .join(path.trim_start_matches("~/"))
        .to_string_lossy()
        .to_string()
}

fn canonical_or_normalize(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| normalize_lexical(path))
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                let _ = out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlx_agent_skills::{SkillCapabilities, SkillRequirements, SkillSource};

    fn sample_skill(name: &str) -> SkillPackage {
        SkillPackage {
            name: name.to_string(),
            description: String::new(),
            homepage: None,
            emoji: None,
            always: false,
            os: Vec::new(),
            source: SkillSource::Workspace,
            file_path: PathBuf::from(format!("skills/{name}/SKILL.md")),
            base_dir: PathBuf::from(format!("skills/{name}")),
            body: String::new(),
            requires: SkillRequirements::default(),
            capabilities: SkillCapabilities::default(),
            install: Vec::new(),
            sha256: Some("abcd".to_string()),
            trust_level: TrustLevel::Local,
        }
    }

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

    #[test]
    fn network_allowlist_blocks_unlisted_domain() {
        let policy = DefaultPolicyEngine::new(PolicyConfig {
            network_allow_domains: vec!["api.github.com".to_string()],
            ..PolicyConfig::default()
        });
        let decision = policy.check_network("https://example.com/resource", "GET");
        assert!(matches!(decision, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn egress_blocks_direct_ip_when_enabled() {
        let policy = DefaultPolicyEngine::new(PolicyConfig {
            block_direct_ip_egress: true,
            ..PolicyConfig::default()
        });
        let decision = policy.check_network("https://1.1.1.1/v1/test", "GET");
        assert!(matches!(decision, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn airgapped_mode_blocks_all_network() {
        let policy = DefaultPolicyEngine::new(PolicyConfig {
            airgapped_mode: true,
            ..PolicyConfig::default()
        });
        let decision = policy.check_network("https://api.github.com", "GET");
        assert!(matches!(decision, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn owner_only_mode_blocks_outside_workspace() {
        let root = std::env::temp_dir().join("policy_owner_only_root");
        let policy = DefaultPolicyEngine::new(PolicyConfig {
            owner_only_mode: true,
            workspace_root: Some(root),
            ..PolicyConfig::default()
        });

        let decision = policy.check_file_access(Path::new("/etc/passwd"), false);
        assert!(matches!(decision, PolicyDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn skill_capability_fs_read_is_enforced() {
        let policy = DefaultPolicyEngine::new(PolicyConfig::default());
        let skill = sample_skill("reader");
        let decision = policy
            .check_tool_call(
                "read_file",
                &serde_json::json!({"path":"README.md"}),
                Some(&skill),
                ExecutionMode::Full,
            )
            .await;
        assert!(matches!(decision, PolicyDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn skill_pin_mismatch_denies_load() {
        let policy = DefaultPolicyEngine::new(PolicyConfig {
            skill_sha256_pins: BTreeMap::from([("reader".to_string(), "ffffffff".to_string())]),
            ..PolicyConfig::default()
        });
        let mut skill = sample_skill("reader");
        skill.sha256 = Some("aaaa".to_string());

        let decision = policy.check_skill_load(&skill).await;
        assert!(matches!(decision, PolicyDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn known_hash_change_returns_warning() {
        let policy = DefaultPolicyEngine::new(PolicyConfig {
            known_skill_hashes: BTreeMap::from([("reader".to_string(), "1111".to_string())]),
            ..PolicyConfig::default()
        });
        let mut skill = sample_skill("reader");
        skill.sha256 = Some("2222".to_string());

        let decision = policy.check_skill_load(&skill).await;
        assert!(matches!(decision, PolicyDecision::Ask { .. }));
    }
}
