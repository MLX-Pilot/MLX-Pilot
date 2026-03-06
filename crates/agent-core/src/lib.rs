//! # mlx-agent-core
//!
//! Core agent infrastructure for the MLX-Pilot: agent loop,
//! tool registry, policy engine, approval service, event bus,
//! audit logging, and session persistence.

pub mod agent_loop;
pub mod approval;
pub mod audit;
pub mod context_budget;
pub mod events;
pub mod memory;
pub mod policy;
pub mod prompt_builder;
pub mod registry;
pub mod runtime;
pub mod session;
pub mod tool_catalog;

// Re-exports for convenience.
pub use agent_loop::{AgentError, AgentLoop, AgentLoopConfig, AgentResponse};
pub use approval::{
    ApprovalDecision, ApprovalError, ApprovalMode, ApprovalRequest, ApprovalService,
};
pub use audit::{AuditEventType, AuditLog, AuditLogEntry};
pub use context_budget::{
    ContextBudgetInput, ContextBudgetManager, ContextBudgetOutput, ContextBudgetTelemetry,
    ContextSummaryArtifact, ResponseStyle,
};
pub use events::{AgentEvent, EventBus};
pub use memory::{MemoryRecord, MemorySearchHit, MemoryStore};
pub use policy::{PolicyConfig, PolicyDecision, PolicyEngine, PolicyToolInspection};
pub use prompt_builder::{
    select_model_prompt_profile, ModelPromptProfile, ModelPromptProfileKind, PromptBuildInput,
    PromptBuildOutput, PromptBuilder, VerbosityLevel,
};
pub use registry::{
    ChannelDescriptor, ChannelRegistry, HelpMetadata, PluginClass, PluginDescriptor,
    PluginRegistry, ToolRegistry,
};
pub use runtime::{LazyRuntimeRegistry, RuntimeHealth, RuntimeStatus, SkillRuntime};
pub use session::{SessionMessage, SessionStore};
pub use tool_catalog::{
    catalog_entry, profile_tool_names, resolve_effective_tool_policy, resolve_tool_access,
    tool_catalog, EffectiveToolPolicy, EffectiveToolPolicyEntry, ToolAccessDecision,
    ToolCatalogEntry, ToolPolicyState, ToolProfileName, ToolRisk, ToolRuleSet, ToolRuleTrace,
    ToolSection,
};
