//! # mlx-agent-core
//!
//! Core agent infrastructure for the MLX-Pilot: agent loop,
//! tool registry, policy engine, approval service, event bus,
//! audit logging, and session persistence.

pub mod agent_loop;
pub mod approval;
pub mod audit;
pub mod events;
pub mod policy;
pub mod prompt_builder;
pub mod registry;
pub mod runtime;
pub mod session;

// Re-exports for convenience.
pub use agent_loop::{AgentError, AgentLoop, AgentLoopConfig, AgentResponse};
pub use approval::{
    ApprovalDecision, ApprovalError, ApprovalMode, ApprovalRequest, ApprovalService,
};
pub use audit::{AuditEventType, AuditLog, AuditLogEntry};
pub use events::{AgentEvent, EventBus};
pub use policy::{PolicyConfig, PolicyDecision, PolicyEngine};
pub use prompt_builder::{
    select_model_prompt_profile, ModelPromptProfile, ModelPromptProfileKind, PromptBuildInput,
    PromptBuildOutput, PromptBuilder, VerbosityLevel,
};
pub use registry::ToolRegistry;
pub use session::{SessionMessage, SessionStore};
