//! # mlx-agent-skills
//!
//! Skill loading, frontmatter parsing, ClawHub resolver, integrity
//! verification, and content scanning for the MLX-Pilot agent.

pub mod resolver;
pub mod types;

// Re-exports for convenience.
pub use resolver::{RegistrySkillMeta, ResolverError, SkillResolver};
pub use types::{
    FilesystemScope, InstallKind, InstallSpec, NetworkScope, SkillCapabilities, SkillPackage,
    SkillRequirements, SkillSource, TrustLevel,
};
