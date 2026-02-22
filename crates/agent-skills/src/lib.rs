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
    check_requirements, matches_current_os, parse_frontmatter, to_skill_package, ParsedSkill,
    RawFrontmatter, RequirementCheck,
};
pub use loader::{SkillLimits, SkillLoader, SkillPrompt};
pub use resolver::{RegistrySkillMeta, ResolverError, SkillResolver};
pub use types::{
    FilesystemScope, InstallKind, InstallSpec, NetworkScope, SkillCapabilities, SkillPackage,
    SkillRequirements, SkillSource, TrustLevel,
};
