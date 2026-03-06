//! Skill runtime for the AgentLoop.
//!
//! Maintains the loaded skills in memory and provides compact summaries,
//! resolution by name, and helper functions.

use mlx_agent_skills::{SkillLimits, SkillLoader, SkillPackage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use tracing::{debug, info};

pub struct SkillRuntime {
    skills: HashMap<String, SkillPackage>,
}

impl Default for SkillRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillRuntime {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Load all skills from the workspace root.
    pub async fn load_from_workspace(&mut self, workspace_root: &std::path::Path) {
        let loader = SkillLoader::from_workspace(workspace_root, SkillLimits::default());
        match loader.load_all().await {
            Ok(packages) => {
                info!(count = packages.len(), "Loaded skills from workspace");

                self.skills.clear();
                for pkg in packages {
                    self.skills.insert(pkg.name.clone(), pkg);
                }
            }
            Err(e) => {
                debug!(error = %e, "Failed to load skills");
            }
        }
    }

    /// Retrieve a skill by its exact name.
    pub fn get(&self, name: &str) -> Option<&SkillPackage> {
        self.skills.get(name)
    }

    /// Remove a skill by name.
    pub fn remove(&mut self, name: &str) -> Option<SkillPackage> {
        self.skills.remove(name)
    }

    /// Returns all loaded skills.
    pub fn all(&self) -> impl Iterator<Item = &SkillPackage> {
        self.skills.values()
    }

    /// Return compact one-line summaries: `name: summary`.
    /// The skill body is never injected here.
    pub fn compact_summaries(&self, max_skills: usize, max_line_chars: usize) -> Vec<String> {
        self.compact_summaries_filtered(max_skills, max_line_chars, None)
    }

    /// Return compact one-line summaries with optional allowlist filtering.
    pub fn compact_summaries_filtered(
        &self,
        max_skills: usize,
        max_line_chars: usize,
        allowed: Option<&[String]>,
    ) -> Vec<String> {
        let allowed_set = allowed
            .map(|list| {
                list.iter()
                    .map(|v| v.to_ascii_lowercase())
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();

        let mut skills = self.skills.values().collect::<Vec<_>>();
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        skills
            .into_iter()
            .filter(|skill| {
                allowed
                    .map(|_| allowed_set.contains(&skill.name.to_ascii_lowercase()))
                    .unwrap_or(true)
            })
            .take(max_skills)
            .map(|skill| {
                let summary = extract_skill_summary_line(skill, max_line_chars);
                format!("{}: {}", skill.name, summary)
            })
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        let mut names = self.skills.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeHealth {
    Unknown,
    Idle,
    Ready,
    Degraded,
    Error,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub id: String,
    pub enabled: bool,
    pub configured: bool,
    pub loaded: bool,
    pub health: RuntimeHealth,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub last_transition_epoch_ms: Option<u128>,
}

#[derive(Debug, Clone, Default)]
struct RuntimeSlot {
    loaded: bool,
    errors: Vec<String>,
    last_transition_epoch_ms: Option<u128>,
}

#[derive(Debug, Clone, Default)]
pub struct LazyRuntimeRegistry {
    slots: HashMap<String, RuntimeSlot>,
}

impl LazyRuntimeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_loaded(&mut self, id: &str) {
        let slot = self.slots.entry(id.to_ascii_lowercase()).or_default();
        slot.loaded = true;
        slot.errors.clear();
        slot.last_transition_epoch_ms = Some(epoch_ms_now());
    }

    pub fn mark_unloaded(&mut self, id: &str) {
        let slot = self.slots.entry(id.to_ascii_lowercase()).or_default();
        slot.loaded = false;
        slot.last_transition_epoch_ms = Some(epoch_ms_now());
    }

    pub fn mark_error(&mut self, id: &str, error: impl Into<String>) {
        let slot = self.slots.entry(id.to_ascii_lowercase()).or_default();
        slot.loaded = false;
        slot.errors.push(error.into());
        slot.last_transition_epoch_ms = Some(epoch_ms_now());
    }

    pub fn clear_errors(&mut self, id: &str) {
        let slot = self.slots.entry(id.to_ascii_lowercase()).or_default();
        slot.errors.clear();
        slot.last_transition_epoch_ms = Some(epoch_ms_now());
    }

    pub fn snapshot(&self, id: &str, enabled: bool, configured: bool) -> RuntimeStatus {
        let normalized = id.to_ascii_lowercase();
        let slot = self.slots.get(&normalized);
        let loaded = slot.map(|value| value.loaded).unwrap_or(false);
        let errors = slot.map(|value| value.errors.clone()).unwrap_or_default();
        let health = if !enabled {
            RuntimeHealth::Disabled
        } else if !errors.is_empty() {
            RuntimeHealth::Error
        } else if loaded {
            RuntimeHealth::Ready
        } else if configured {
            RuntimeHealth::Idle
        } else {
            RuntimeHealth::Unknown
        };

        RuntimeStatus {
            id: normalized,
            enabled,
            configured,
            loaded,
            health,
            errors,
            last_transition_epoch_ms: slot.and_then(|value| value.last_transition_epoch_ms),
        }
    }
}

fn extract_skill_summary_line(skill: &SkillPackage, max_chars: usize) -> String {
    let base = if !skill.description.trim().is_empty() {
        skill.description.trim()
    } else {
        skill
            .body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or("No summary")
    };

    let mut one_line = base
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" - ", " ")
        .replace(" • ", " ");

    if one_line.chars().count() > max_chars {
        one_line = truncate_chars(&one_line, max_chars);
    }

    one_line
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return text.to_string();
    }

    let keep = max_chars.saturating_sub(3);
    let mut out = chars.into_iter().take(keep).collect::<String>();
    out.push_str("...");
    out
}

fn epoch_ms_now() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_runtime_registry_reports_idle_and_ready() {
        let mut registry = LazyRuntimeRegistry::new();

        let idle = registry.snapshot("telegram", true, true);
        assert_eq!(idle.health, RuntimeHealth::Idle);
        assert!(!idle.loaded);

        registry.mark_loaded("telegram");
        let ready = registry.snapshot("telegram", true, true);
        assert_eq!(ready.health, RuntimeHealth::Ready);
        assert!(ready.loaded);
    }

    #[test]
    fn lazy_runtime_registry_reports_errors() {
        let mut registry = LazyRuntimeRegistry::new();
        registry.mark_error("memory", "missing local index");
        let status = registry.snapshot("memory", true, true);
        assert_eq!(status.health, RuntimeHealth::Error);
        assert_eq!(status.errors, vec!["missing local index"]);
    }
}
