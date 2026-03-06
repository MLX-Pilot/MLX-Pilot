//! Skill loader — discovers and loads skills from `workspace_root/skills/`.
//!
//! Each skill is a directory containing a `SKILL.md` file with YAML
//! frontmatter. The loader parses, filters, and assembles
//! [`SkillPackage`] instances ready for the agent.

use crate::frontmatter::{check_skill_requirements, parse_frontmatter, to_skill_package};
use crate::resolver::ResolverError;
use crate::types::{RequirementContext, SkillPackage, SkillSource, TrustLevel};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Configuration limits for skill loading.
#[derive(Debug, Clone)]
pub struct SkillLimits {
    /// Maximum number of skills included in a single prompt.
    pub max_skills_in_prompt: usize,
    /// Maximum size (bytes) of a single SKILL.md file.
    pub max_skill_file_bytes: usize,
    /// Maximum total characters from all skill bodies combined.
    pub max_skills_prompt_chars: usize,
}

impl Default for SkillLimits {
    fn default() -> Self {
        Self {
            max_skills_in_prompt: 20,
            max_skill_file_bytes: 64 * 1024, // 64 KB
            max_skills_prompt_chars: 128_000,
        }
    }
}

/// Loads skills from the filesystem.
pub struct SkillLoader {
    /// Root directory to search for skills (e.g. `workspace_root/skills/`).
    skills_dir: PathBuf,
    /// Limits for skill loading.
    limits: SkillLimits,
}

#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    pub package: SkillPackage,
    pub requirements: crate::frontmatter::RequirementCheck,
}

impl SkillLoader {
    /// Create a new loader pointing at `skills_dir`.
    pub fn new(skills_dir: PathBuf, limits: SkillLimits) -> Self {
        Self { skills_dir, limits }
    }

    /// Create a loader from a workspace root (appends `skills/`).
    pub fn from_workspace(workspace_root: &Path, limits: SkillLimits) -> Self {
        Self::new(workspace_root.join("skills"), limits)
    }

    /// Discover and load all skills from the skills directory.
    ///
    /// Skills that fail to parse or don't match the current OS are skipped
    /// (logged as warnings). Returns the successfully loaded skills.
    pub async fn load_all(&self) -> Result<Vec<SkillPackage>, ResolverError> {
        self.load_all_with_context(&RequirementContext::from_current_env())
            .await
    }

    pub async fn load_all_with_context(
        &self,
        context: &RequirementContext,
    ) -> Result<Vec<SkillPackage>, ResolverError> {
        Ok(self
            .discover_all_with_context(context)
            .await?
            .into_iter()
            .filter(|entry| entry.requirements.satisfied)
            .map(|entry| entry.package)
            .collect())
    }

    pub async fn discover_all(&self) -> Result<Vec<DiscoveredSkill>, ResolverError> {
        self.discover_all_with_context(&RequirementContext::from_current_env())
            .await
    }

    pub async fn discover_all_with_context(
        &self,
        context: &RequirementContext,
    ) -> Result<Vec<DiscoveredSkill>, ResolverError> {
        if !self.skills_dir.exists() {
            debug!(dir = %self.skills_dir.display(), "skills directory not found, returning empty");
            return Ok(Vec::new());
        }

        let mut skills = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.skills_dir)
            .await
            .map_err(ResolverError::Io)?;

        while let Some(entry) = entries.next_entry().await.map_err(ResolverError::Io)? {
            let path = entry.path();

            // Each skill is a directory containing SKILL.md.
            let skill_file = if path.is_dir() {
                path.join("SKILL.md")
            } else if path.is_file()
                && path
                    .file_name()
                    .is_some_and(|f| f.eq_ignore_ascii_case("SKILL.md"))
            {
                // Also accept SKILL.md directly in skills/ (flat layout).
                path.clone()
            } else {
                continue;
            };

            if !skill_file.exists() {
                continue;
            }

            match self.load_single(&skill_file).await {
                Ok(pkg) => {
                    let req_check = check_skill_requirements(&pkg, context);
                    if !req_check.satisfied {
                        debug!(
                            skill = %pkg.name,
                            os_supported = req_check.os_supported,
                            missing_bins = ?req_check.missing_bins,
                            missing_any_bins = ?req_check.missing_any_bins,
                            missing_env = ?req_check.missing_env,
                            missing_config = ?req_check.missing_config,
                            "skill discovered but not currently eligible"
                        );
                    }

                    skills.push(DiscoveredSkill {
                        package: pkg,
                        requirements: req_check,
                    });
                }
                Err(e) => {
                    warn!(
                        file = %skill_file.display(),
                        error = %e,
                        "failed to parse SKILL.md, skipping"
                    );
                }
            }
        }

        Ok(skills)
    }

    /// Load a single SKILL.md file.
    async fn load_single(&self, skill_file: &Path) -> Result<SkillPackage, ResolverError> {
        let content = tokio::fs::read_to_string(skill_file)
            .await
            .map_err(ResolverError::Io)?;

        // Check file size limit.
        if content.len() > self.limits.max_skill_file_bytes {
            return Err(ResolverError::Parse {
                message: format!(
                    "SKILL.md at '{}' exceeds max size ({} > {} bytes)",
                    skill_file.display(),
                    content.len(),
                    self.limits.max_skill_file_bytes
                ),
            });
        }

        let parsed = parse_frontmatter(&content)?;
        let mut package = to_skill_package(
            &parsed,
            skill_file,
            SkillSource::Workspace,
            TrustLevel::Local,
        );
        package.sha256 = Some(sha256_hex(content.as_bytes()));
        Ok(package)
    }

    /// Generate the combined skill prompt from loaded skills.
    ///
    /// Applies limits (`max_skills_in_prompt`, `max_skills_prompt_chars`).
    /// Always-included skills come first (not counted against the limit).
    pub fn build_prompt(&self, skills: &[SkillPackage]) -> SkillPrompt {
        let mut always_skills = Vec::new();
        let mut regular_skills = Vec::new();

        for skill in skills {
            if skill.always {
                always_skills.push(skill);
            } else {
                regular_skills.push(skill);
            }
        }

        let mut prompt_parts = Vec::new();
        let mut total_chars = 0;
        let mut included_count = 0;
        let mut truncated = false;

        // Always-skills first (not counted against max_skills_in_prompt).
        for skill in &always_skills {
            let summary = format_skill_summary(skill);
            total_chars += summary.len();
            prompt_parts.push(summary);
        }

        // Regular skills, up to limit.
        for skill in &regular_skills {
            if included_count >= self.limits.max_skills_in_prompt {
                truncated = true;
                break;
            }

            let summary = format_skill_summary(skill);

            if total_chars + summary.len() > self.limits.max_skills_prompt_chars {
                truncated = true;
                break;
            }

            total_chars += summary.len();
            included_count += 1;
            prompt_parts.push(summary);
        }

        SkillPrompt {
            text: prompt_parts.join("\n\n---\n\n"),
            total_skills: skills.len(),
            included_skills: always_skills.len() + included_count,
            total_chars,
            truncated,
        }
    }
}

fn sha256_hex(input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

/// Result of building a skill prompt.
#[derive(Debug, Clone)]
pub struct SkillPrompt {
    /// The combined text to inject into the system prompt.
    pub text: String,
    /// Total number of skills discovered.
    pub total_skills: usize,
    /// How many were actually included.
    pub included_skills: usize,
    /// Total character count.
    pub total_chars: usize,
    /// Whether some skills were truncated due to limits.
    pub truncated: bool,
}

/// Format a single skill as a prompt section.
fn format_skill_summary(skill: &SkillPackage) -> String {
    let summary = if !skill.description.trim().is_empty() {
        skill.description.trim().to_string()
    } else {
        skill
            .body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or("No summary")
            .to_string()
    };

    format!(
        "{}: {}",
        skill.name,
        truncate_chars(&normalize_whitespace(&summary), 160)
    )
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut out = chars.into_iter().take(keep).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RequirementContext;
    use std::fs;

    fn create_test_skill(dir: &Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[tokio::test]
    async fn load_all_from_workspace() {
        let tmp = std::env::temp_dir().join("skill_loader_test");
        let skills = tmp.join("skills");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&skills).unwrap();

        create_test_skill(
            &skills,
            "test-skill",
            r#"---
name: test-skill
description: A test skill.
---

# Test Skill
Hello!"#,
        );

        create_test_skill(
            &skills,
            "another-skill",
            r#"---
name: another-skill
description: Another test.
always: true
---

# Another
World!"#,
        );

        let loader = SkillLoader::from_workspace(&tmp, SkillLimits::default());
        let loaded = loader.load_all().await.unwrap();
        assert_eq!(loaded.len(), 2);

        let names: Vec<&str> = loaded.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"test-skill"));
        assert!(names.contains(&"another-skill"));
        assert!(loaded.iter().all(|skill| skill.sha256.is_some()));

        let always = loaded.iter().find(|s| s.name == "another-skill").unwrap();
        assert!(always.always);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn load_skips_nonexistent_dir() {
        let loader = SkillLoader::from_workspace(
            Path::new("/nonexistent/path/12345"),
            SkillLimits::default(),
        );
        let loaded = loader.load_all().await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn load_respects_file_size_limit() {
        let tmp = std::env::temp_dir().join("skill_loader_size_test");
        let skills = tmp.join("skills");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&skills).unwrap();

        // Create a skill that's too big.
        let big_body = "x".repeat(200);
        let content = format!("---\nname: big-skill\ndescription: Too big.\n---\n\n{big_body}");
        create_test_skill(&skills, "big-skill", &content);

        let limits = SkillLimits {
            max_skill_file_bytes: 100, // Very small limit.
            ..Default::default()
        };
        let loader = SkillLoader::from_workspace(&tmp, limits);
        let loaded = loader.load_all().await.unwrap();
        assert!(loaded.is_empty(), "big skill should be skipped");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn discover_reports_eligibility_for_critical_skills() {
        let tmp = std::env::temp_dir().join("skill_loader_catalog_test");
        let skills = tmp.join("skills");
        let bins = tmp.join("bin");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&skills).unwrap();
        fs::create_dir_all(&bins).unwrap();

        for bin in ["obsidian", "wa-cli", "gh", "curl"] {
            let path = bins.join(bin);
            fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&path).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&path, perms).unwrap();
            }
        }

        let original_path = std::env::var_os("PATH");
        let mut path_entries = vec![bins.clone()];
        path_entries.extend(std::env::split_paths(
            &original_path.clone().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(path_entries).unwrap());

        create_test_skill(
            &skills,
            "obsidian",
            r#"---
name: obsidian
description: Obsidian integration.
metadata: {"openclaw":{"requires":{"bins":["obsidian"]}}}
---
"#,
        );
        create_test_skill(
            &skills,
            "wacli",
            r#"---
name: wacli
description: WhatsApp CLI integration.
metadata: {"openclaw":{"requires":{"bins":["wa-cli"]}}}
---
"#,
        );
        create_test_skill(
            &skills,
            "gog",
            r#"---
name: gog
description: GOG downloads.
metadata: {"openclaw":{"requires":{"anyBins":["gogdl","lgogdownloader"]}}}
---
"#,
        );
        create_test_skill(
            &skills,
            "github",
            r#"---
name: github
description: GitHub operations.
metadata:
  openclaw:
    primaryEnv: GITHUB_TOKEN
    requires:
      bins: ["gh"]
      env: ["GITHUB_TOKEN"]
---
"#,
        );
        create_test_skill(
            &skills,
            "weather",
            r#"---
name: weather
description: Weather lookup.
metadata: {"openclaw":{"requires":{"bins":["curl"]}}}
---
"#,
        );
        create_test_skill(
            &skills,
            "summarize",
            r#"---
name: summarize
description: Summaries.
metadata:
  openclaw:
    requires:
      config: ["provider"]
---
"#,
        );

        let loader = SkillLoader::from_workspace(&tmp, SkillLimits::default());
        let context = RequirementContext::from_current_env()
            .with_env_keys(["GITHUB_TOKEN"])
            .with_config_keys(["provider"]);
        let discovered = loader.discover_all_with_context(&context).await.unwrap();

        let obsidian = discovered
            .iter()
            .find(|item| item.package.name == "obsidian")
            .unwrap();
        assert!(obsidian.requirements.satisfied);

        let wacli = discovered
            .iter()
            .find(|item| item.package.name == "wacli")
            .unwrap();
        assert!(wacli.requirements.satisfied);

        let gog = discovered
            .iter()
            .find(|item| item.package.name == "gog")
            .unwrap();
        assert!(!gog.requirements.satisfied);
        assert_eq!(gog.requirements.missing_any_bins.len(), 2);

        let github = discovered
            .iter()
            .find(|item| item.package.name == "github")
            .unwrap();
        assert!(github.requirements.satisfied);
        assert_eq!(github.package.primary_env.as_deref(), Some("GITHUB_TOKEN"));

        let weather = discovered
            .iter()
            .find(|item| item.package.name == "weather")
            .unwrap();
        assert!(weather.requirements.satisfied);

        let summarize = discovered
            .iter()
            .find(|item| item.package.name == "summarize")
            .unwrap();
        assert!(summarize.requirements.satisfied);

        match original_path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn build_prompt_respects_max_skills() {
        let limits = SkillLimits {
            max_skills_in_prompt: 2,
            ..Default::default()
        };
        let loader = SkillLoader::new(PathBuf::from("."), limits);

        let skills: Vec<SkillPackage> = (0..5)
            .map(|i| SkillPackage {
                name: format!("skill-{i}"),
                description: format!("Description {i}"),
                homepage: None,
                emoji: None,
                always: false,
                os: Vec::new(),
                primary_env: None,
                source: SkillSource::Workspace,
                file_path: PathBuf::from(format!("skills/skill-{i}/SKILL.md")),
                base_dir: PathBuf::from(format!("skills/skill-{i}")),
                body: format!("Body {i}"),
                requires: Default::default(),
                capabilities: Default::default(),
                install: Vec::new(),
                sha256: None,
                trust_level: TrustLevel::Local,
            })
            .collect();

        let prompt = loader.build_prompt(&skills);
        assert_eq!(prompt.included_skills, 2);
        assert_eq!(prompt.total_skills, 5);
        assert!(prompt.truncated);
    }

    #[test]
    fn build_prompt_respects_char_limit() {
        let limits = SkillLimits {
            max_skills_in_prompt: 100,
            max_skills_prompt_chars: 50, // Very small.
            ..Default::default()
        };
        let loader = SkillLoader::new(PathBuf::from("."), limits);

        let skills: Vec<SkillPackage> = (0..3)
            .map(|i| SkillPackage {
                name: format!("skill-{i}"),
                description: "A".repeat(30),
                homepage: None,
                emoji: None,
                always: false,
                os: Vec::new(),
                primary_env: None,
                source: SkillSource::Workspace,
                file_path: PathBuf::from(format!("skills/skill-{i}/SKILL.md")),
                base_dir: PathBuf::from(format!("skills/skill-{i}")),
                body: String::new(),
                requires: Default::default(),
                capabilities: Default::default(),
                install: Vec::new(),
                sha256: None,
                trust_level: TrustLevel::Local,
            })
            .collect();

        let prompt = loader.build_prompt(&skills);
        assert!(prompt.truncated);
        assert!(prompt.total_chars <= 50 + 50); // First skill may exceed, but second is blocked.
    }

    #[test]
    fn build_prompt_always_skills_first() {
        let limits = SkillLimits {
            max_skills_in_prompt: 1, // Only 1 regular allowed.
            ..Default::default()
        };
        let loader = SkillLoader::new(PathBuf::from("."), limits);

        let make = |name: &str, always: bool| SkillPackage {
            name: name.into(),
            description: format!("{name} desc"),
            homepage: None,
            emoji: None,
            always,
            os: Vec::new(),
            primary_env: None,
            source: SkillSource::Workspace,
            file_path: PathBuf::from(format!("skills/{name}/SKILL.md")),
            base_dir: PathBuf::from(format!("skills/{name}")),
            body: String::new(),
            requires: Default::default(),
            capabilities: Default::default(),
            install: Vec::new(),
            sha256: None,
            trust_level: TrustLevel::Local,
        };

        let skills = vec![
            make("regular-1", false),
            make("always-1", true),
            make("regular-2", false),
        ];

        let prompt = loader.build_prompt(&skills);
        // always-1 + 1 regular = 2 included.
        assert_eq!(prompt.included_skills, 2);
        assert!(prompt.text.contains("always-1"));
        assert!(prompt.truncated); // 2nd regular was truncated.
    }
}
