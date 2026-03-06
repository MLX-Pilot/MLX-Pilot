//! Tool catalog, profiles, and effective policy resolution.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ToolSection {
    Filesystem,
    Execution,
    Sessions,
    Messaging,
    Memory,
    Automation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    Low,
    Medium,
    High,
    Critical,
}

impl ToolRisk {
    pub fn requires_approval(self) -> bool {
        matches!(self, Self::High | Self::Critical)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfileName {
    Minimal,
    Coding,
    Messaging,
    Full,
}

impl ToolProfileName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Coding => "coding",
            Self::Messaging => "messaging",
            Self::Full => "full",
        }
    }
}

impl Default for ToolProfileName {
    fn default() -> Self {
        Self::Coding
    }
}

impl std::str::FromStr for ToolProfileName {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "minimal" => Ok(Self::Minimal),
            "coding" => Ok(Self::Coding),
            "messaging" => Ok(Self::Messaging),
            "full" => Ok(Self::Full),
            other => Err(format!("unknown tool profile '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    pub name: String,
    pub section: ToolSection,
    pub risk: ToolRisk,
    pub description: String,
    #[serde(default)]
    pub profiles: Vec<ToolProfileName>,
    #[serde(default = "default_true")]
    pub implemented: bool,
}

impl ToolCatalogEntry {
    pub fn enabled_in_profile(&self, profile: ToolProfileName) -> bool {
        self.profiles.contains(&profile)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolRuleSet {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolPolicyState {
    #[serde(default)]
    pub profile: ToolProfileName,
    #[serde(default)]
    pub global: ToolRuleSet,
    #[serde(default)]
    pub agents: BTreeMap<String, ToolRuleSet>,
    #[serde(default)]
    pub sessions: BTreeMap<String, ToolRuleSet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolRuleTrace {
    pub scope: String,
    pub action: String,
    pub rule: String,
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolAccessDecision {
    pub tool_name: String,
    pub allowed: bool,
    pub implemented: bool,
    pub risk: ToolRisk,
    pub section: ToolSection,
    pub final_rule: String,
    #[serde(default)]
    pub trace: Vec<ToolRuleTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffectiveToolPolicyEntry {
    pub name: String,
    pub section: ToolSection,
    pub risk: ToolRisk,
    pub description: String,
    pub implemented: bool,
    pub allowed: bool,
    pub final_rule: String,
    #[serde(default)]
    pub trace: Vec<ToolRuleTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffectiveToolPolicy {
    pub profile: ToolProfileName,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub entries: Vec<EffectiveToolPolicyEntry>,
}

pub fn tool_catalog() -> Vec<ToolCatalogEntry> {
    vec![
        entry(
            "read_file",
            ToolSection::Filesystem,
            ToolRisk::Low,
            "Read the contents of a file inside the workspace sandbox.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "list_dir",
            ToolSection::Filesystem,
            ToolRisk::Low,
            "List files and directories inside the workspace sandbox.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "write_file",
            ToolSection::Filesystem,
            ToolRisk::High,
            "Write or create a file inside the workspace sandbox.",
            &[ToolProfileName::Coding, ToolProfileName::Full],
        ),
        entry(
            "edit_file",
            ToolSection::Filesystem,
            ToolRisk::High,
            "Apply an exact-text edit to a file inside the workspace sandbox.",
            &[ToolProfileName::Coding, ToolProfileName::Full],
        ),
        entry(
            "exec",
            ToolSection::Execution,
            ToolRisk::Critical,
            "Run a shell command in the workspace.",
            &[ToolProfileName::Coding, ToolProfileName::Full],
        ),
        entry(
            "message",
            ToolSection::Messaging,
            ToolRisk::High,
            "Send an outbound message through a configured channel account.",
            &[ToolProfileName::Messaging, ToolProfileName::Full],
        ),
        entry(
            "sessions_list",
            ToolSection::Sessions,
            ToolRisk::Low,
            "List locally stored agent sessions.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "sessions_history",
            ToolSection::Sessions,
            ToolRisk::Low,
            "Read message history from a local agent session.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "sessions_spawn",
            ToolSection::Sessions,
            ToolRisk::Medium,
            "Create a new local agent session.",
            &[
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "sessions_send",
            ToolSection::Sessions,
            ToolRisk::Medium,
            "Append a message to a local agent session.",
            &[
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "sessions_status",
            ToolSection::Sessions,
            ToolRisk::Low,
            "Inspect metadata and current status for a local agent session.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "memory_search",
            ToolSection::Memory,
            ToolRisk::Low,
            "Search compact local memory artifacts generated from prior sessions.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "memory_get",
            ToolSection::Memory,
            ToolRisk::Low,
            "Fetch a local memory artifact by id.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
    ]
}

pub fn resolve_effective_tool_policy(
    policy: &ToolPolicyState,
    agent_id: &str,
    session_id: Option<&str>,
) -> EffectiveToolPolicy {
    let mut entries = tool_catalog()
        .into_iter()
        .map(|entry| {
            let decision =
                resolve_tool_access(&entry.name, policy, agent_id, session_id, Some(&entry));
            EffectiveToolPolicyEntry {
                name: entry.name,
                section: entry.section,
                risk: entry.risk,
                description: entry.description,
                implemented: entry.implemented,
                allowed: decision.allowed,
                final_rule: decision.final_rule,
                trace: decision.trace,
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.name.cmp(&right.name));

    EffectiveToolPolicy {
        profile: policy.profile,
        agent_id: normalize_scope_key(agent_id),
        session_id: session_id.map(normalize_scope_key),
        entries,
    }
}

pub fn resolve_tool_access(
    tool_name: &str,
    policy: &ToolPolicyState,
    agent_id: &str,
    session_id: Option<&str>,
    catalog_entry: Option<&ToolCatalogEntry>,
) -> ToolAccessDecision {
    let catalog = catalog_entry.cloned().or_else(|| {
        tool_catalog()
            .into_iter()
            .find(|entry| entry.name == tool_name)
    });

    let Some(entry) = catalog else {
        return ToolAccessDecision {
            tool_name: tool_name.to_string(),
            allowed: false,
            implemented: false,
            risk: ToolRisk::Critical,
            section: ToolSection::Automation,
            final_rule: "catalog:unknown".to_string(),
            trace: vec![ToolRuleTrace {
                scope: "catalog".to_string(),
                action: "deny".to_string(),
                rule: "unknown_tool".to_string(),
                matched: true,
            }],
        };
    };

    let mut allowed = entry.enabled_in_profile(policy.profile) && entry.implemented;
    let mut final_rule = if allowed {
        format!("profile:{}", policy.profile.as_str())
    } else if !entry.implemented {
        "catalog:not_implemented".to_string()
    } else {
        format!("profile:{}:disabled", policy.profile.as_str())
    };
    let mut trace = vec![ToolRuleTrace {
        scope: "profile".to_string(),
        action: if allowed { "allow" } else { "deny" }.to_string(),
        rule: policy.profile.as_str().to_string(),
        matched: true,
    }];

    apply_rules(
        &mut allowed,
        &mut final_rule,
        &mut trace,
        "global",
        &policy.global,
        &entry.name,
    );

    let normalized_agent = normalize_scope_key(agent_id);
    if let Some(rules) = policy.agents.get(&normalized_agent) {
        apply_rules(
            &mut allowed,
            &mut final_rule,
            &mut trace,
            &format!("agent:{normalized_agent}"),
            rules,
            &entry.name,
        );
    }

    if let Some(session) = session_id.map(normalize_scope_key) {
        if let Some(rules) = policy.sessions.get(&session) {
            apply_rules(
                &mut allowed,
                &mut final_rule,
                &mut trace,
                &format!("session:{session}"),
                rules,
                &entry.name,
            );
        }
    }

    ToolAccessDecision {
        tool_name: entry.name,
        allowed,
        implemented: entry.implemented,
        risk: entry.risk,
        section: entry.section,
        final_rule,
        trace,
    }
}

pub fn profile_tool_names(profile: ToolProfileName) -> BTreeSet<String> {
    tool_catalog()
        .into_iter()
        .filter(|entry| entry.implemented && entry.enabled_in_profile(profile))
        .map(|entry| entry.name)
        .collect()
}

pub fn catalog_entry(name: &str) -> Option<ToolCatalogEntry> {
    tool_catalog().into_iter().find(|entry| entry.name == name)
}

fn entry(
    name: &str,
    section: ToolSection,
    risk: ToolRisk,
    description: &str,
    profiles: &[ToolProfileName],
) -> ToolCatalogEntry {
    ToolCatalogEntry {
        name: name.to_string(),
        section,
        risk,
        description: description.to_string(),
        profiles: profiles.to_vec(),
        implemented: true,
    }
}

fn apply_rules(
    allowed: &mut bool,
    final_rule: &mut String,
    trace: &mut Vec<ToolRuleTrace>,
    scope: &str,
    rules: &ToolRuleSet,
    tool_name: &str,
) {
    if let Some(rule) = first_match(&rules.allow, tool_name) {
        *allowed = true;
        *final_rule = format!("{scope}:allow:{rule}");
        trace.push(ToolRuleTrace {
            scope: scope.to_string(),
            action: "allow".to_string(),
            rule,
            matched: true,
        });
    }

    if let Some(rule) = first_match(&rules.deny, tool_name) {
        *allowed = false;
        *final_rule = format!("{scope}:deny:{rule}");
        trace.push(ToolRuleTrace {
            scope: scope.to_string(),
            action: "deny".to_string(),
            rule,
            matched: true,
        });
    }
}

fn first_match(patterns: &[String], tool_name: &str) -> Option<String> {
    patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .find(|pattern| glob_match::glob_match(pattern, tool_name))
        .map(ToString::to_string)
}

fn normalize_scope_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coding_profile_contains_exec() {
        let tools = profile_tool_names(ToolProfileName::Coding);
        assert!(tools.contains("exec"));
        assert!(!tools.contains("message"));
    }

    #[test]
    fn precedence_applies_session_last() {
        let decision = resolve_tool_access(
            "exec",
            &ToolPolicyState {
                profile: ToolProfileName::Minimal,
                global: ToolRuleSet {
                    allow: vec!["exec".to_string()],
                    deny: Vec::new(),
                },
                agents: BTreeMap::from([(
                    "default".to_string(),
                    ToolRuleSet {
                        allow: Vec::new(),
                        deny: vec!["exec".to_string()],
                    },
                )]),
                sessions: BTreeMap::from([(
                    "s-1".to_string(),
                    ToolRuleSet {
                        allow: vec!["exec".to_string()],
                        deny: Vec::new(),
                    },
                )]),
            },
            "default",
            Some("s-1"),
            None,
        );

        assert!(decision.allowed);
        assert_eq!(decision.final_rule, "session:s-1:allow:exec");
    }

    #[test]
    fn unknown_tool_is_denied() {
        let decision = resolve_tool_access(
            "not_real",
            &ToolPolicyState::default(),
            "default",
            None,
            None,
        );
        assert!(!decision.allowed);
        assert_eq!(decision.final_rule, "catalog:unknown");
    }
}
