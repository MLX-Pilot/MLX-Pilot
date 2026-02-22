//! Skill runtime for the AgentLoop.
//!
//! Maintains the loaded skills in memory and provides the prompt string,
//! resolution by name, and other helper functions.

use mlx_agent_skills::{SkillLimits, SkillLoader, SkillPackage, SkillPrompt};
use std::collections::HashMap;
use tracing::{debug, info};

pub struct SkillRuntime {
    skills: HashMap<String, SkillPackage>,
    prompt: Option<SkillPrompt>,
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
            prompt: None,
        }
    }

    /// Load all skills from the workspace root.
    pub async fn load_from_workspace(&mut self, workspace_root: &std::path::Path) {
        let loader = SkillLoader::from_workspace(workspace_root, SkillLimits::default());
        match loader.load_all().await {
            Ok(packages) => {
                info!(count = packages.len(), "Loaded skills from workspace");
                let prompt_data = loader.build_prompt(&packages);

                self.skills.clear();
                for pkg in packages {
                    self.skills.insert(pkg.name.clone(), pkg);
                }

                self.prompt = Some(prompt_data);
            }
            Err(e) => {
                debug!(error = %e, "Failed to load skills");
            }
        }
    }

    /// Get the system prompt text for the loaded skills.
    pub fn system_prompt_text(&self) -> Option<&str> {
        self.prompt.as_ref().map(|p| p.text.as_str())
    }

    /// Retrieve a skill by its exact name.
    pub fn get(&self, name: &str) -> Option<&SkillPackage> {
        self.skills.get(name)
    }

    /// Returns all loaded skills.
    pub fn all(&self) -> impl Iterator<Item = &SkillPackage> {
        self.skills.values()
    }
}
