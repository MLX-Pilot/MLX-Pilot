//! Skill runtime for the AgentLoop.
//!
//! Maintains the loaded skills in memory and provides compact summaries,
//! resolution by name, and helper functions.

use mlx_agent_skills::{SkillLimits, SkillLoader, SkillPackage};
use std::collections::HashMap;
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

    /// Returns all loaded skills.
    pub fn all(&self) -> impl Iterator<Item = &SkillPackage> {
        self.skills.values()
    }

    /// Return compact one-line summaries: `name: summary`.
    /// The skill body is never injected here.
    pub fn compact_summaries(&self, max_skills: usize, max_line_chars: usize) -> Vec<String> {
        let mut skills = self.skills.values().collect::<Vec<_>>();
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        skills
            .into_iter()
            .take(max_skills)
            .map(|skill| {
                let summary = extract_skill_summary_line(skill, max_line_chars);
                format!("{}: {}", skill.name, summary)
            })
            .collect()
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
