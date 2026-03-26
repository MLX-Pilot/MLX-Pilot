//! `AgentLoop` — the core agent loop that orchestrates
//! LLM calls, tool dispatch, policy checks, and approvals.

use crate::approval::ApprovalService;
use crate::audit::AuditLog;
use crate::events::EventBus;
use crate::policy::PolicyEngine;
use crate::registry::ToolRegistry;
use crate::session::SessionStore;
use mlx_agent_tools::ExecutionMode;
use std::sync::Arc;

/// Configuration for an `AgentLoop` instance.
#[derive(Debug, Clone)]
pub struct AgentLoopConfig {
    pub model_id: String,
    pub workspace_root: std::path::PathBuf,
    pub system_prompt: Option<String>,
    pub max_iterations: usize,
    pub max_tokens_per_turn: u32,
    pub mode: ExecutionMode,
    pub skill_filter: Option<Vec<String>>,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            workspace_root: std::path::PathBuf::new(),
            system_prompt: None,
            max_iterations: 25,
            max_tokens_per_turn: 4096,
            mode: ExecutionMode::Full,
            skill_filter: None,
        }
    }
}

/// The final response from an agent run.
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub session_id: String,
    pub content: String,
    pub iterations: usize,
    pub latency_ms: u64,
}

/// Errors during agent execution.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("exceeded maximum iterations ({max})")]
    MaxIterations { max: usize },

    #[error("LLM provider error: {message}")]
    ProviderError { message: String },

    #[error("tool error in `{tool}`: {message}")]
    ToolError { tool: String, message: String },

    #[error("policy denied: {reason}")]
    PolicyDenied { reason: String },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// The main agent loop.
///
/// Orchestrates: system prompt → LLM call → tool dispatch → loop.
/// Full implementation in Phase 1 (task 1.7).
#[allow(dead_code)]
pub struct AgentLoop {
    config: AgentLoopConfig,
    tool_registry: ToolRegistry,
    policy: Arc<dyn PolicyEngine>,
    approval: Arc<dyn ApprovalService>,
    event_bus: Arc<EventBus>,
    audit: Arc<AuditLog>,
    session: SessionStore,
}

impl AgentLoop {
    /// Create a new agent loop.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AgentLoopConfig,
        tool_registry: ToolRegistry,
        policy: Arc<dyn PolicyEngine>,
        approval: Arc<dyn ApprovalService>,
        event_bus: Arc<EventBus>,
        audit: Arc<AuditLog>,
        session: SessionStore,
    ) -> Self {
        Self {
            config,
            tool_registry,
            policy,
            approval,
            event_bus,
            audit,
            session,
        }
    }

    /// Run the agent loop with a user message.
    ///
    /// Stub — will be implemented in Phase 1 (task 1.7).
    pub async fn run(&mut self, _user_message: &str) -> Result<AgentResponse, AgentError> {
        todo!("AgentLoop::run() — Phase 1, task 1.7")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = AgentLoopConfig::default();
        assert_eq!(config.max_iterations, 25);
        assert_eq!(config.max_tokens_per_turn, 4096);
        assert_eq!(config.mode, ExecutionMode::Full);
    }
}
