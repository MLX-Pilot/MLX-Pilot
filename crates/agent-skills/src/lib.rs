//! # mlx-agent-skills
//!
//! Skill loading, frontmatter parsing, ClawHub resolver, integrity
//! verification, and content scanning for the MLX-Pilot agent.

pub mod frontmatter;
pub mod loader;
pub mod resolver;
pub mod types;

// Re-exports for convenience.
pub use frontmatter::{
    check_requirements, check_requirements_with_context, check_skill_requirements, current_os_tag,
    matches_current_os, parse_frontmatter, to_skill_package, ParsedSkill, RawFrontmatter,
    RequirementCheck,
};
pub use loader::{DiscoveredSkill, SkillLimits, SkillLoader, SkillPrompt};
pub use resolver::{RegistrySkillMeta, ResolverError, SkillResolver};
pub use types::{
    normalize_config_key, normalize_env_key, FilesystemScope, InstallKind, InstallSpec,
    NetworkScope, RequirementContext, SkillCapabilities, SkillPackage, SkillRequirements,
    SkillSource, TrustLevel,
};
