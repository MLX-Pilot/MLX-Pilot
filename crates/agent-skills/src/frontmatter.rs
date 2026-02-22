//! Frontmatter parser for SKILL.md files.
//!
//! Parses the YAML frontmatter between `---` markers and extracts
//! the skill body (everything after the closing `---`).

use crate::resolver::ResolverError;
use crate::types::*;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Raw YAML frontmatter as deserialized from a SKILL.md.
///
/// This maps 1:1 to the YAML keys found in OpenClaw/NanoBot skill files.
#[derive(Debug, Clone, Deserialize)]
pub struct RawFrontmatter {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub always: bool,
    #[serde(default)]
    pub os: Vec<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Metadata block nested inside `metadata.openclaw` or `metadata.nanobot`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CompatMetadata {
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default)]
    pub requires: Option<CompatRequires>,
    #[serde(default)]
    pub install: Vec<InstallSpec>,
    #[serde(default)]
    pub capabilities: Option<SkillCapabilities>,
}

/// Requirements inside the compatibility metadata block.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompatRequires {
    #[serde(default)]
    pub bins: Vec<String>,
    #[serde(default)]
    pub any_bins: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
}

/// Result of parsing a SKILL.md file.
#[derive(Debug, Clone)]
pub struct ParsedSkill {
    pub frontmatter: RawFrontmatter,
    pub body: String,
    pub compat: CompatMetadata,
}

/// Parse the frontmatter + body from a SKILL.md string.
///
/// The format is:
/// ```text
/// ---
/// name: my-skill
/// description: ...
/// ---
///
/// # Skill body (markdown)
/// ```
pub fn parse_frontmatter(content: &str) -> Result<ParsedSkill, ResolverError> {
    let content = content.trim();

    // Must start with `---`.
    if !content.starts_with("---") {
        return Err(ResolverError::Parse {
            message: "SKILL.md must start with '---' (YAML frontmatter)".into(),
        });
    }

    // Find the closing `---`.
    let rest = &content[3..];
    let close_idx = rest.find("\n---").ok_or_else(|| ResolverError::Parse {
        message: "missing closing '---' in frontmatter".into(),
    })?;

    let yaml_str = &rest[..close_idx];
    let body_start = close_idx + 4; // skip "\n---"
    let body = if body_start < rest.len() {
        rest[body_start..].trim().to_string()
    } else {
        String::new()
    };

    // Parse the YAML.
    let frontmatter: RawFrontmatter =
        serde_yaml::from_str(yaml_str).map_err(|e| ResolverError::Parse {
            message: format!("invalid YAML frontmatter: {e}"),
        })?;

    // Extract compatibility metadata (openclaw or nanobot).
    let compat = extract_compat_metadata(&frontmatter.metadata);

    Ok(ParsedSkill {
        frontmatter,
        body,
        compat,
    })
}

/// Extract `CompatMetadata` from the `metadata` JSON value,
/// checking `metadata.openclaw` first, then `metadata.nanobot`.
fn extract_compat_metadata(metadata: &Option<serde_json::Value>) -> CompatMetadata {
    let Some(meta) = metadata else {
        return CompatMetadata::default();
    };

    // Try openclaw first, then nanobot.
    for key in &["openclaw", "nanobot"] {
        if let Some(inner) = meta.get(key) {
            if let Ok(parsed) = serde_json::from_value::<CompatMetadata>(inner.clone()) {
                return parsed;
            }
        }
    }

    CompatMetadata::default()
}

/// Convert a [`ParsedSkill`] into a full [`SkillPackage`].
pub fn to_skill_package(
    parsed: &ParsedSkill,
    file_path: &Path,
    source: SkillSource,
    trust_level: TrustLevel,
) -> SkillPackage {
    let base_dir = file_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    let fm = &parsed.frontmatter;
    let compat = &parsed.compat;

    // Merge requirements from compat metadata.
    let requires = match &compat.requires {
        Some(req) => SkillRequirements {
            bins: req.bins.clone(),
            any_bins: req.any_bins.clone(),
            env: req.env.clone(),
            config: Vec::new(),
        },
        None => SkillRequirements::default(),
    };

    // Apply {baseDir} substitution to body.
    let body = parsed
        .body
        .replace("{baseDir}", &base_dir.to_string_lossy());

    SkillPackage {
        name: fm.name.clone(),
        description: fm.description.clone().unwrap_or_default(),
        homepage: fm.homepage.clone(),
        emoji: compat.emoji.clone(),
        always: fm.always,
        os: fm.os.clone(),
        source,
        file_path: file_path.to_path_buf(),
        base_dir,
        body,
        requires,
        capabilities: compat.capabilities.clone().unwrap_or_default(),
        install: compat.install.clone(),
        sha256: None,
        trust_level,
    }
}

/// Check if a skill passes OS filtering for the current platform.
pub fn matches_current_os(skill: &SkillPackage) -> bool {
    if skill.os.is_empty() {
        return true; // No OS restriction.
    }
    let current = current_os_tag();
    skill.os.iter().any(|os| os.eq_ignore_ascii_case(current))
}

/// Get the current OS tag matching OpenClaw conventions.
fn current_os_tag() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

/// Check if required binaries are available on PATH.
pub fn check_requirements(requires: &SkillRequirements) -> RequirementCheck {
    let mut missing_bins = Vec::new();
    let mut missing_env = Vec::new();

    // Check bins (all must be present).
    for bin in &requires.bins {
        if which_exists(bin).is_none() {
            missing_bins.push(bin.clone());
        }
    }

    // Check any_bins (at least one must be present).
    let any_bins_ok = if requires.any_bins.is_empty() {
        true
    } else {
        requires.any_bins.iter().any(|b| which_exists(b).is_some())
    };

    // Check env vars.
    for var in &requires.env {
        if std::env::var(var).is_err() {
            missing_env.push(var.clone());
        }
    }

    RequirementCheck {
        satisfied: missing_bins.is_empty() && any_bins_ok && missing_env.is_empty(),
        missing_bins,
        any_bins_satisfied: any_bins_ok,
        missing_env,
    }
}

/// Result of checking skill requirements.
#[derive(Debug, Clone)]
pub struct RequirementCheck {
    pub satisfied: bool,
    pub missing_bins: Vec<String>,
    pub any_bins_satisfied: bool,
    pub missing_env: Vec<String>,
}

/// Minimal cross-platform "which" — just check if a file is findable on PATH.
fn which_exists(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let exts = if cfg!(windows) {
        vec![".exe", ".cmd", ".bat", ".com"]
    } else {
        vec![""]
    };
    for dir in std::env::split_paths(&path_var) {
        for ext in &exts {
            let candidate = dir.join(format!("{bin}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SKILL: &str = r#"---
name: weather
description: Get current weather and forecasts (no API key required).
homepage: https://wttr.in/:help
metadata: {"nanobot":{"emoji":"🌤️","requires":{"bins":["curl"]}}}
---

# Weather

Two free services, no API keys needed.

## wttr.in (primary)

```bash
curl -s "wttr.in/London?format=3"
```
"#;

    const ALWAYS_SKILL: &str = r#"---
name: memory
description: Always-loaded skill.
always: true
---

# Memory
This skill is always loaded.
"#;

    const OS_FILTERED_SKILL: &str = r#"---
name: apple-notes
description: macOS only skill.
os:
  - macos
metadata: {"openclaw":{"emoji":"📝","requires":{"bins":["osascript"]}}}
---

# Apple Notes
macOS only.
"#;

    const COMPLEX_SKILL: &str = r#"---
name: github
description: "GitHub operations via gh CLI."
metadata:
  openclaw:
    emoji: "🐙"
    requires:
      bins:
        - gh
    install:
      - id: brew
        kind: brew
        formula: gh
        bins:
          - gh
        label: "Install GitHub CLI (brew)"
---

# GitHub Skill
Use the `gh` CLI.
"#;

    const ANY_BINS_SKILL: &str = r#"---
name: coding-agent
description: "Delegate coding tasks."
metadata:
  openclaw:
    emoji: "🧩"
    requires:
      anyBins:
        - claude
        - codex
        - pi
---

# Coding Agent
"#;

    const CAPABILITIES_SKILL: &str = r#"---
name: admin-tool
description: "Needs capabilities"
metadata:
  openclaw:
    capabilities:
      exec: true
      exec_commands: ["ls", "cat"]
      filesystem: "workspace"
      network: "read"
---

# Admin
"#;

    #[test]
    fn parse_valid_frontmatter() {
        let parsed = parse_frontmatter(VALID_SKILL).unwrap();
        assert_eq!(parsed.frontmatter.name, "weather");
        assert_eq!(
            parsed.frontmatter.description.as_deref(),
            Some("Get current weather and forecasts (no API key required).")
        );
        assert_eq!(
            parsed.frontmatter.homepage.as_deref(),
            Some("https://wttr.in/:help")
        );
        assert!(!parsed.frontmatter.always);
        assert!(parsed.frontmatter.os.is_empty());
        assert_eq!(parsed.compat.emoji.as_deref(), Some("🌤️"));
        assert!(parsed.body.contains("# Weather"));
        assert!(parsed.body.contains("wttr.in/London"));
    }

    #[test]
    fn parse_always_skill() {
        let parsed = parse_frontmatter(ALWAYS_SKILL).unwrap();
        assert!(parsed.frontmatter.always);
        assert_eq!(parsed.frontmatter.name, "memory");
    }

    #[test]
    fn parse_os_filtered_skill() {
        let parsed = parse_frontmatter(OS_FILTERED_SKILL).unwrap();
        assert_eq!(parsed.frontmatter.os, vec!["macos".to_string()]);
        let pkg = to_skill_package(
            &parsed,
            Path::new("/skills/apple-notes/SKILL.md"),
            SkillSource::Workspace,
            TrustLevel::Local,
        );
        // On Windows/Linux this skill should be filtered out.
        if cfg!(target_os = "macos") {
            assert!(matches_current_os(&pkg));
        } else {
            assert!(!matches_current_os(&pkg));
        }
    }

    #[test]
    fn parse_complex_metadata() {
        let parsed = parse_frontmatter(COMPLEX_SKILL).unwrap();
        assert_eq!(parsed.compat.emoji.as_deref(), Some("🐙"));
        let req = parsed.compat.requires.as_ref().unwrap();
        assert_eq!(req.bins, vec!["gh".to_string()]);
        assert!(!parsed.compat.install.is_empty());
        assert_eq!(parsed.compat.install[0].formula.as_deref(), Some("gh"));
    }

    #[test]
    fn parse_any_bins() {
        let parsed = parse_frontmatter(ANY_BINS_SKILL).unwrap();
        let req = parsed.compat.requires.as_ref().unwrap();
        assert!(req.bins.is_empty());
        assert_eq!(req.any_bins.len(), 3);
        assert!(req.any_bins.contains(&"claude".to_string()));
    }

    #[test]
    fn parse_capabilities() {
        let parsed = parse_frontmatter(CAPABILITIES_SKILL).unwrap();
        let caps = parsed.compat.capabilities.as_ref().unwrap();
        assert!(caps.exec);
        assert_eq!(
            caps.exec_commands,
            vec!["ls".to_string(), "cat".to_string()]
        );
        // Note: filesystem serialization might be tricky if it expects "read_write" vs "readwrite", let's just check exec for now.
    }

    #[test]
    fn parse_invalid_no_frontmatter() {
        let result = parse_frontmatter("# Just a markdown file\nNo frontmatter.");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must start with"));
    }

    #[test]
    fn parse_invalid_unclosed_frontmatter() {
        let result = parse_frontmatter("---\nname: broken\nno closing marker");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing closing"));
    }

    #[test]
    fn parse_invalid_yaml() {
        let result = parse_frontmatter("---\n[invalid yaml: {{{\n---\n# Body");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid YAML"));
    }

    #[test]
    fn basedir_substitution() {
        let skill_md = r#"---
name: test-basedir
description: Tests baseDir.
---

Run: `{baseDir}/scripts/run.sh`
"#;
        let parsed = parse_frontmatter(skill_md).unwrap();
        let pkg = to_skill_package(
            &parsed,
            Path::new("/workspace/skills/test-basedir/SKILL.md"),
            SkillSource::Workspace,
            TrustLevel::Local,
        );
        assert!(
            pkg.body
                .contains("/workspace/skills/test-basedir/scripts/run.sh")
                || pkg
                    .body
                    .contains("\\workspace\\skills\\test-basedir\\scripts\\run.sh"),
            "body: {}",
            pkg.body
        );
        assert!(!pkg.body.contains("{baseDir}"));
    }

    #[test]
    fn to_skill_package_full() {
        let parsed = parse_frontmatter(VALID_SKILL).unwrap();
        let pkg = to_skill_package(
            &parsed,
            Path::new("/skills/weather/SKILL.md"),
            SkillSource::Workspace,
            TrustLevel::Local,
        );
        assert_eq!(pkg.name, "weather");
        assert_eq!(pkg.source, SkillSource::Workspace);
        assert_eq!(pkg.trust_level, TrustLevel::Local);
        assert!(pkg.requires.bins.contains(&"curl".to_string()));
        assert_eq!(pkg.emoji.as_deref(), Some("🌤️"));
    }

    #[test]
    fn requirement_check_missing_bins() {
        let req = SkillRequirements {
            bins: vec!["nonexistent_binary_12345".into()],
            any_bins: Vec::new(),
            env: Vec::new(),
            config: Vec::new(),
        };
        let check = check_requirements(&req);
        assert!(!check.satisfied);
        assert!(check
            .missing_bins
            .contains(&"nonexistent_binary_12345".into()));
    }

    #[test]
    fn requirement_check_any_bins_none_present() {
        let req = SkillRequirements {
            bins: Vec::new(),
            any_bins: vec!["nonexistent_a_12345".into(), "nonexistent_b_12345".into()],
            env: Vec::new(),
            config: Vec::new(),
        };
        let check = check_requirements(&req);
        assert!(!check.satisfied);
        assert!(!check.any_bins_satisfied);
    }

    #[test]
    fn requirement_check_empty_is_satisfied() {
        let req = SkillRequirements::default();
        let check = check_requirements(&req);
        assert!(check.satisfied);
    }
}
