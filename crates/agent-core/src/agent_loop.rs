//! `AgentLoop` — the core agent loop that orchestrates
//! LLM calls, tool dispatch, policy checks, and approvals.

use crate::approval::ApprovalService;
use crate::audit::AuditLog;
use crate::events::EventBus;
use crate::policy::PolicyEngine;
use crate::registry::ToolRegistry;
use mlx_agent_tools::ExecutionMode;
use mlx_ollama_core::{
    ChatMessage, ChatToolsRequest, FunctionDef, GenerationOptions, MessageRole, ModelProvider,
    ProviderError, TokenUsage, ToolCallRequest,
};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

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
    pub tool_calls_made: usize,
    pub usage: TokenUsage,
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

impl From<ProviderError> for AgentError {
    fn from(err: ProviderError) -> Self {
        AgentError::ProviderError {
            message: err.to_string(),
        }
    }
}

/// The main agent loop.
///
/// Orchestrates: system prompt → LLM call → tool dispatch → loop until
/// the model responds without tool calls or max_iterations is hit.
pub struct AgentLoop {
    config: AgentLoopConfig,
    provider: Arc<dyn ModelProvider>,
    tool_registry: ToolRegistry,
    #[allow(dead_code)]
    policy: Arc<dyn PolicyEngine>,
    #[allow(dead_code)]
    approval: Arc<dyn ApprovalService>,
    #[allow(dead_code)]
    event_bus: Arc<EventBus>,
    #[allow(dead_code)]
    audit: Arc<AuditLog>,
    /// In-memory conversation history for the current run.
    history: Vec<ChatMessage>,
}

impl AgentLoop {
    /// Create a new agent loop.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AgentLoopConfig,
        provider: Arc<dyn ModelProvider>,
        tool_registry: ToolRegistry,
        policy: Arc<dyn PolicyEngine>,
        approval: Arc<dyn ApprovalService>,
        event_bus: Arc<EventBus>,
        audit: Arc<AuditLog>,
    ) -> Self {
        Self {
            config,
            provider,
            tool_registry,
            policy,
            approval,
            event_bus,
            audit,
            history: Vec::new(),
        }
    }

    /// Run the agent loop with a user message.
    ///
    /// The loop:
    /// 1. Build messages with system prompt + conversation history
    /// 2. Call provider with tool definitions
    /// 3. If response contains tool_calls → execute them, inject results, loop
    /// 4. If response is text-only → return final response
    /// 5. Guard: stop after `max_iterations`
    pub async fn run(&mut self, user_message: &str) -> Result<AgentResponse, AgentError> {
        let started = Instant::now();
        let session_id = uuid::Uuid::new_v4().to_string();

        // Build initial messages.
        let mut messages: Vec<ChatMessage> = Vec::new();

        // System prompt.
        if let Some(ref system) = self.config.system_prompt {
            messages.push(ChatMessage::text(MessageRole::System, system.clone()));
        }

        // Conversation history.
        messages.extend(self.history.iter().cloned());

        // User message.
        messages.push(ChatMessage::text(
            MessageRole::User,
            user_message.to_string(),
        ));

        // Build tool definitions from registry.
        let tool_defs = self.build_tool_definitions();

        let mut iterations = 0;
        let mut total_tool_calls = 0;
        let mut total_usage = TokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        };

        loop {
            iterations += 1;

            if iterations > self.config.max_iterations {
                warn!(
                    session = %session_id,
                    iterations,
                    "agent loop exceeded max iterations"
                );
                return Err(AgentError::MaxIterations {
                    max: self.config.max_iterations,
                });
            }

            debug!(
                session = %session_id,
                iteration = iterations,
                messages = messages.len(),
                "calling provider"
            );

            // Call provider with tools.
            let response = self
                .provider
                .chat_with_tools(ChatToolsRequest {
                    model_id: self.config.model_id.clone(),
                    messages: messages.clone(),
                    tools: tool_defs.clone(),
                    options: GenerationOptions {
                        temperature: Some(0.1),
                        max_tokens: Some(self.config.max_tokens_per_turn),
                        top_p: None,
                    },
                })
                .await?;

            // Accumulate usage.
            total_usage.prompt_tokens += response.usage.prompt_tokens;
            total_usage.completion_tokens += response.usage.completion_tokens;
            total_usage.total_tokens += response.usage.total_tokens;

            let assistant_msg = response.message.clone();

            // Check if there are tool calls.
            if assistant_msg.tool_calls.is_empty() {
                // Final response — no more tool calls.
                info!(
                    session = %session_id,
                    iterations,
                    tool_calls = total_tool_calls,
                    latency_ms = started.elapsed().as_millis() as u64,
                    "agent loop completed"
                );

                // Save to history.
                self.history.push(ChatMessage::text(
                    MessageRole::User,
                    user_message.to_string(),
                ));
                self.history.push(assistant_msg.clone());

                return Ok(AgentResponse {
                    session_id,
                    content: assistant_msg.content,
                    iterations,
                    tool_calls_made: total_tool_calls,
                    usage: total_usage,
                    latency_ms: started.elapsed().as_millis() as u64,
                });
            }

            // Process tool calls.
            messages.push(assistant_msg.clone());

            for tool_call in &assistant_msg.tool_calls {
                total_tool_calls += 1;

                debug!(
                    session = %session_id,
                    tool = %tool_call.name,
                    call_id = %tool_call.id,
                    "executing tool call"
                );

                let result = self.execute_tool_call(tool_call).await;

                let tool_output = match result {
                    Ok(output) => output,
                    Err(e) => format!("Error: {e}"),
                };

                // Inject tool result.
                messages.push(ChatMessage::tool_result(tool_call.id.clone(), tool_output));
            }
        }
    }

    /// Execute a single tool call through the registry.
    async fn execute_tool_call(&self, tool_call: &ToolCallRequest) -> Result<String, AgentError> {
        let params: serde_json::Value =
            serde_json::from_str(&tool_call.arguments).map_err(|e| AgentError::ToolError {
                tool: tool_call.name.clone(),
                message: format!("invalid JSON arguments: {e}"),
            })?;

        let ctx = mlx_agent_tools::ToolContext {
            workspace_root: self.config.workspace_root.clone(),
            session_id: "agent".into(),
            active_skill: None,
            mode: self.config.mode,
        };

        let result = self
            .tool_registry
            .dispatch(&tool_call.name, params, &ctx)
            .await
            .map_err(|e| AgentError::ToolError {
                tool: tool_call.name.clone(),
                message: e.to_string(),
            })?;

        Ok(result.output)
    }

    /// Build tool definitions from the registry.
    fn build_tool_definitions(&self) -> Vec<FunctionDef> {
        self.tool_registry
            .definitions()
            .into_iter()
            .map(|def| FunctionDef {
                name: def.name,
                description: def.description,
                parameters: def.parameters,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::{ApprovalDecision, ApprovalError, ApprovalRequest};
    use crate::audit::AuditLog;
    use crate::events::EventBus;
    use crate::policy::PolicyDecision;
    use mlx_ollama_core::{
        ChatRequest, ChatResponse, ChatToolsRequest, ModelDescriptor, TokenUsage,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ── Mock provider ────────────────────────────────────────────────

    struct MockProvider {
        call_count: AtomicUsize,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl ModelProvider for MockProvider {
        fn provider_id(&self) -> &'static str {
            "mock"
        }

        async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError> {
            Ok(vec![])
        }

        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, ProviderError> {
            Ok(ChatResponse {
                model_id: "mock-model".into(),
                provider: "mock".into(),
                message: ChatMessage::text(MessageRole::Assistant, "direct chat response"),
                usage: TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                },
                latency_ms: 100,
                raw_output: None,
            })
        }

        async fn chat_with_tools(
            &self,
            req: ChatToolsRequest,
        ) -> Result<ChatResponse, ProviderError> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);

            // First call: request a tool call.
            if count == 0 {
                return Ok(ChatResponse {
                    model_id: req.model_id,
                    provider: "mock".into(),
                    message: ChatMessage {
                        role: MessageRole::Assistant,
                        content: String::new(),
                        tool_calls: vec![ToolCallRequest {
                            id: "call_001".into(),
                            name: "list_dir".into(),
                            arguments: r#"{"path": "."}"#.into(),
                        }],
                        tool_call_id: None,
                    },
                    usage: TokenUsage {
                        prompt_tokens: 50,
                        completion_tokens: 20,
                        total_tokens: 70,
                    },
                    latency_ms: 200,
                    raw_output: None,
                });
            }

            // Second call: final text response.
            Ok(ChatResponse {
                model_id: req.model_id,
                provider: "mock".into(),
                message: ChatMessage::text(
                    MessageRole::Assistant,
                    "I see the workspace contains some files. How can I help?",
                ),
                usage: TokenUsage {
                    prompt_tokens: 100,
                    completion_tokens: 15,
                    total_tokens: 115,
                },
                latency_ms: 150,
                raw_output: None,
            })
        }
    }

    // ── Mock policy ──────────────────────────────────────────────────

    struct AllowAllPolicy;

    #[async_trait::async_trait]
    impl PolicyEngine for AllowAllPolicy {
        async fn check_tool_call(
            &self,
            _tool_name: &str,
            _params: &serde_json::Value,
            _skill: Option<&mlx_agent_skills::SkillPackage>,
            _mode: ExecutionMode,
        ) -> PolicyDecision {
            PolicyDecision::Allow
        }

        async fn check_skill_load(
            &self,
            _skill: &mlx_agent_skills::SkillPackage,
        ) -> PolicyDecision {
            PolicyDecision::Allow
        }

        fn check_file_access(&self, _path: &std::path::Path, _write: bool) -> PolicyDecision {
            PolicyDecision::Allow
        }

        fn check_network(&self, _url: &str, _method: &str) -> PolicyDecision {
            PolicyDecision::Allow
        }
    }

    // ── Mock approval ────────────────────────────────────────────────

    struct AutoApproval;

    #[async_trait::async_trait]
    impl ApprovalService for AutoApproval {
        async fn request_approval(
            &self,
            _request: ApprovalRequest,
            _timeout: std::time::Duration,
        ) -> Result<ApprovalDecision, ApprovalError> {
            Ok(ApprovalDecision::AllowOnce)
        }

        fn is_allowed(&self, _tool_name: &str, _params_pattern: &str) -> bool {
            true
        }

        fn add_allowlist_entry(&self, _tool_name: &str, _pattern: String) {}
    }

    // ── Helper ───────────────────────────────────────────────────────

    fn create_test_loop(provider: Arc<dyn ModelProvider>) -> AgentLoop {
        let tmp = std::env::temp_dir().join("agent_loop_test");
        std::fs::create_dir_all(&tmp).unwrap();

        AgentLoop::new(
            AgentLoopConfig {
                model_id: "mock-model".into(),
                workspace_root: tmp,
                system_prompt: Some("You are a helpful assistant.".into()),
                max_iterations: 10,
                max_tokens_per_turn: 4096,
                mode: ExecutionMode::Full,
                skill_filter: None,
            },
            provider,
            ToolRegistry::with_builtins(),
            Arc::new(AllowAllPolicy),
            Arc::new(AutoApproval),
            Arc::new(EventBus::default()),
            Arc::new(AuditLog::new(std::path::PathBuf::from(
                "/tmp/agent-test-audit",
            ))),
        )
    }

    // ── Tests ────────────────────────────────────────────────────────

    #[test]
    fn default_config_values() {
        let config = AgentLoopConfig::default();
        assert_eq!(config.max_iterations, 25);
        assert_eq!(config.max_tokens_per_turn, 4096);
        assert_eq!(config.mode, ExecutionMode::Full);
    }

    #[tokio::test]
    async fn agent_loop_with_tool_call() {
        let provider = Arc::new(MockProvider::new());
        let mut agent = create_test_loop(provider);

        let response = agent.run("What files are in the workspace?").await.unwrap();

        assert_eq!(
            response.iterations, 2,
            "should take 2 iterations (tool call + final)"
        );
        assert_eq!(response.tool_calls_made, 1, "should make 1 tool call");
        assert!(
            response.content.contains("workspace"),
            "response: {}",
            response.content
        );
        assert!(response.usage.total_tokens > 0);
        assert!(response.latency_ms > 0);
    }

    #[tokio::test]
    async fn agent_loop_max_iterations() {
        // Provider that always returns tool calls (infinite loop).
        struct InfiniteToolProvider;

        #[async_trait::async_trait]
        impl ModelProvider for InfiniteToolProvider {
            fn provider_id(&self) -> &'static str {
                "infinite"
            }
            async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError> {
                Ok(vec![])
            }
            async fn chat(&self, _r: ChatRequest) -> Result<ChatResponse, ProviderError> {
                unreachable!()
            }
            async fn chat_with_tools(
                &self,
                req: ChatToolsRequest,
            ) -> Result<ChatResponse, ProviderError> {
                Ok(ChatResponse {
                    model_id: req.model_id,
                    provider: "infinite".into(),
                    message: ChatMessage {
                        role: MessageRole::Assistant,
                        content: String::new(),
                        tool_calls: vec![ToolCallRequest {
                            id: format!("call_{}", uuid::Uuid::new_v4()),
                            name: "list_dir".into(),
                            arguments: r#"{"path": "."}"#.into(),
                        }],
                        tool_call_id: None,
                    },
                    usage: TokenUsage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    },
                    latency_ms: 10,
                    raw_output: None,
                })
            }
        }

        let provider = Arc::new(InfiniteToolProvider);
        let mut agent = create_test_loop(provider);
        agent.config.max_iterations = 3;

        let result = agent.run("do something forever").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("exceeded maximum iterations"), "got: {err}");
    }

    #[tokio::test]
    async fn agent_loop_direct_response() {
        // Provider that never returns tool calls.
        struct DirectProvider;

        #[async_trait::async_trait]
        impl ModelProvider for DirectProvider {
            fn provider_id(&self) -> &'static str {
                "direct"
            }
            async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError> {
                Ok(vec![])
            }
            async fn chat(&self, _r: ChatRequest) -> Result<ChatResponse, ProviderError> {
                unreachable!()
            }
            async fn chat_with_tools(
                &self,
                req: ChatToolsRequest,
            ) -> Result<ChatResponse, ProviderError> {
                Ok(ChatResponse {
                    model_id: req.model_id,
                    provider: "direct".into(),
                    message: ChatMessage::text(MessageRole::Assistant, "Hello! I'm here to help."),
                    usage: TokenUsage {
                        prompt_tokens: 20,
                        completion_tokens: 8,
                        total_tokens: 28,
                    },
                    latency_ms: 50,
                    raw_output: None,
                })
            }
        }

        let provider = Arc::new(DirectProvider);
        let mut agent = create_test_loop(provider);

        let response = agent.run("hello").await.unwrap();
        assert_eq!(response.iterations, 1, "should complete in 1 iteration");
        assert_eq!(response.tool_calls_made, 0);
        assert!(response.content.contains("Hello"));
    }
}
