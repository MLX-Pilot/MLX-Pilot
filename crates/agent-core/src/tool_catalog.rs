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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolsetDelegationPolicy {
    #[serde(default)]
    pub max_depth: usize,
    #[serde(default = "default_true")]
    pub inherit_parent_policy: bool,
    #[serde(default)]
    pub allow_delegate_tool: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolsetProfile {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub capability_requirements: Vec<String>,
    #[serde(default)]
    pub delegation_policy: ToolsetDelegationPolicy,
}

pub fn tool_catalog() -> Vec<ToolCatalogEntry> {
    vec![
        entry(
            "read_file",
            ToolSection::Filesystem,
            ToolRisk::Low,
            "Ler o conteudo de um arquivo dentro do workspace. Equivalente a Read.",
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
            "Listar arquivos e diretorios dentro do workspace. Equivalente a LS.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "glob",
            ToolSection::Filesystem,
            ToolRisk::Low,
            "Encontrar arquivos por padrao glob no workspace. Equivalente a Glob.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "grep",
            ToolSection::Filesystem,
            ToolRisk::Low,
            "Pesquisar texto ou regex em arquivos do workspace. Equivalente a Grep.",
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
            "Criar ou sobrescrever um arquivo no workspace. Equivalente a Write.",
            &[ToolProfileName::Coding, ToolProfileName::Full],
        ),
        entry(
            "edit_file",
            ToolSection::Filesystem,
            ToolRisk::High,
            "Aplicar uma edicao precisa de texto em arquivo. Equivalente a Edit ou MultiEdit.",
            &[ToolProfileName::Coding, ToolProfileName::Full],
        ),
        entry(
            "checkpoints_list",
            ToolSection::Filesystem,
            ToolRisk::Low,
            "Listar checkpoints locais de rollback criados por ferramentas de arquivo.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "checkpoint_restore",
            ToolSection::Filesystem,
            ToolRisk::High,
            "Restaurar um checkpoint local para desfazer uma alteracao de arquivo.",
            &[ToolProfileName::Coding, ToolProfileName::Full],
        ),
        entry(
            "exec",
            ToolSection::Execution,
            ToolRisk::Critical,
            "Executar programa local no workspace com fila local e sem operadores de shell.",
            &[ToolProfileName::Coding, ToolProfileName::Full],
        ),
        entry(
            "message",
            ToolSection::Messaging,
            ToolRisk::High,
            "Enviar mensagem por um canal configurado.",
            &[ToolProfileName::Messaging, ToolProfileName::Full],
        ),
        entry(
            "sessions_list",
            ToolSection::Sessions,
            ToolRisk::Low,
            "Listar sessoes locais do agent.",
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
            "Ler o historico de mensagens de uma sessao local.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "toolsets_list",
            ToolSection::Sessions,
            ToolRisk::Low,
            "Listar toolsets nomeados disponiveis para runs Hermes-inspired e delegacao.",
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
            "Criar uma nova sessao local do agent. Equivalente funcional a Agent ou Task.",
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
            "Enviar mensagem para uma sessao local existente.",
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
            "Inspecionar metadados e status atual de uma sessao local.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "session_search",
            ToolSection::Sessions,
            ToolRisk::Low,
            "Pesquisar sessoes anteriores relevantes e reutilizar contexto entre conversas.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "delegate_session",
            ToolSection::Sessions,
            ToolRisk::High,
            "Executar uma subsessao delegada com contexto isolado e retornar apenas o resumo.",
            &[
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "memory_search",
            ToolSection::Memory,
            ToolRisk::Low,
            "Pesquisar memorias compactadas geradas por sessoes anteriores.",
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
            "Ler um artefato de memoria local pelo id.",
            &[
                ToolProfileName::Minimal,
                ToolProfileName::Coding,
                ToolProfileName::Messaging,
                ToolProfileName::Full,
            ],
        ),
        entry(
            "memory_write",
            ToolSection::Memory,
            ToolRisk::Medium,
            "Persistir memoria local duravel para reuso em sessoes futuras.",
            &[
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

pub fn toolset_profiles() -> Vec<ToolsetProfile> {
    vec![
        ToolsetProfile {
            id: "general".to_string(),
            description:
                "Balanced Hermes-inspired local agent toolset for coding and memory-aware sessions."
                    .to_string(),
            enabled_tools: profile_tool_names(ToolProfileName::Coding)
                .into_iter()
                .collect(),
            capability_requirements: vec!["fs:read".to_string(), "fs:write".to_string()],
            delegation_policy: ToolsetDelegationPolicy {
                max_depth: 1,
                inherit_parent_policy: true,
                allow_delegate_tool: true,
            },
        },
        ToolsetProfile {
            id: "messaging".to_string(),
            description: "Session, memory, and channel tools for communication-heavy agents."
                .to_string(),
            enabled_tools: profile_tool_names(ToolProfileName::Messaging)
                .into_iter()
                .collect(),
            capability_requirements: vec!["network:http".to_string()],
            delegation_policy: ToolsetDelegationPolicy {
                max_depth: 1,
                inherit_parent_policy: true,
                allow_delegate_tool: true,
            },
        },
        ToolsetProfile {
            id: "full".to_string(),
            description: "Full local tool access, subject to policy/capability enforcement."
                .to_string(),
            enabled_tools: profile_tool_names(ToolProfileName::Full)
                .into_iter()
                .collect(),
            capability_requirements: vec![
                "fs:read".to_string(),
                "fs:write".to_string(),
                "process:spawn".to_string(),
            ],
            delegation_policy: ToolsetDelegationPolicy {
                max_depth: 1,
                inherit_parent_policy: true,
                allow_delegate_tool: true,
            },
        },
        ToolsetProfile {
            id: "safe_readonly".to_string(),
            description: "Read-heavy toolset with memory/recall and no mutating filesystem tools."
                .to_string(),
            enabled_tools: profile_tool_names(ToolProfileName::Minimal)
                .into_iter()
                .chain(
                    ["session_search", "memory_search", "memory_get"]
                        .into_iter()
                        .map(str::to_string),
                )
                .collect(),
            capability_requirements: vec!["fs:read".to_string()],
            delegation_policy: ToolsetDelegationPolicy {
                max_depth: 0,
                inherit_parent_policy: true,
                allow_delegate_tool: false,
            },
        },
    ]
}

pub fn toolset_profile(id: &str) -> Option<ToolsetProfile> {
    let normalized = id.trim().to_ascii_lowercase();
    toolset_profiles()
        .into_iter()
        .find(|toolset| toolset.id.eq_ignore_ascii_case(&normalized))
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
        assert!(tools.contains("glob"));
        assert!(tools.contains("grep"));
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
