//! Hermes-inspired runtime orchestration layered on top of `AgentLoop`.

use crate::agent_loop::{AgentError, AgentLoop, AgentLoopConfig, AgentResponse};
use crate::approval::ApprovalService;
use crate::audit::AuditLog;
use crate::events::{AgentEvent, EventBus};
use crate::memory::{MemoryPromotionDecision, MemoryRecord, MemoryStore};
use crate::memory_manager::{MemoryContextBlock, MemoryLifecycleResult, MemoryManager};
use crate::policy::PolicyEngine;
use crate::registry::ToolRegistry;
use crate::runtime::SkillRuntime;
use crate::session::{SessionMessage, SessionMeta, SessionStore};
use chrono::Utc;
use mlx_ollama_core::ModelProvider;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeVariant {
    #[default]
    Classic,
    HermesInspired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Completed,
    MaxIterations,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionContextEnvelope {
    #[serde(default)]
    pub origin_kind: String,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub source_channel: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub sender_id: String,
    #[serde(default)]
    pub correlation_id: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemorySnapshotMode {
    Off,
    #[default]
    Session,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GatewayContext {
    #[serde(default)]
    pub source_channel: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub sender_id: String,
    #[serde(default)]
    pub correlation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryQuery {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DelegateTaskRequest {
    pub prompt: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<usize>,
    #[serde(default)]
    pub toolset_id: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub handoff_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRuntimeConfig {
    #[serde(default)]
    pub variant: RuntimeVariant,
    #[serde(default)]
    pub persist_tool_events: bool,
    #[serde(default = "default_memory_profile")]
    pub memory_profile: String,
    #[serde(default)]
    pub session_search_enabled: bool,
    #[serde(default)]
    pub delegate_depth: usize,
    #[serde(default)]
    pub session_context: Option<SessionContextEnvelope>,
    #[serde(default)]
    pub session_name: Option<String>,
    #[serde(default = "default_toolset_id")]
    pub toolset_id: String,
    #[serde(default)]
    pub memory_snapshot_mode: MemorySnapshotMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTurnEvent {
    pub session_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct AgentRuntimeResponse {
    pub response: AgentResponse,
    pub stop_reason: StopReason,
    pub memory_context: MemoryContextBlock,
}

#[derive(Clone)]
pub struct AgentRuntime {
    config: AgentRuntimeConfig,
    sessions: Arc<SessionStore>,
    memory: Arc<MemoryStore>,
    event_bus: Arc<EventBus>,
}

impl AgentRuntime {
    pub fn new(
        config: AgentRuntimeConfig,
        sessions: Arc<SessionStore>,
        memory: Arc<MemoryStore>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            config,
            sessions,
            memory,
            event_bus,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        mut loop_config: AgentLoopConfig,
        provider_id: &str,
        provider: Arc<dyn ModelProvider>,
        tool_registry: ToolRegistry,
        skill_runtime: SkillRuntime,
        policy: Arc<dyn PolicyEngine>,
        approval: Arc<dyn ApprovalService>,
        audit: Arc<AuditLog>,
        user_message: &str,
    ) -> Result<AgentRuntimeResponse, AgentError> {
        let memory_manager = MemoryManager::new(self.memory.clone(), self.sessions.clone());
        let session_id = if loop_config.session_id.trim().is_empty() {
            SessionStore::new_session_id()
        } else {
            loop_config.session_id.clone()
        };
        loop_config.session_id = session_id.clone();

        let memory_context = if self.config.variant == RuntimeVariant::HermesInspired {
            memory_manager
                .hydrate_context(
                    user_message,
                    &session_id,
                    &self.config.memory_profile,
                    self.config.session_search_enabled,
                    &self.config.memory_snapshot_mode,
                )
                .await
                .unwrap_or_default()
        } else {
            MemoryContextBlock::default()
        };

        loop_config.system_prompt = merge_system_prompt(
            loop_config.system_prompt.take(),
            self.config.session_context.as_ref(),
            &memory_context.text,
        );

        self.ensure_session_meta(&session_id, provider_id, &loop_config)
            .await
            .map_err(anyhow::Error::from)?;
        self.event_bus.emit(AgentEvent::SessionIngress {
            session_id: session_id.clone(),
            origin_kind: self
                .config
                .session_context
                .as_ref()
                .map(|ctx| ctx.origin_kind.clone())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "local".to_string()),
            source_channel: self
                .config
                .session_context
                .as_ref()
                .map(|ctx| ctx.source_channel.clone())
                .filter(|value| !value.trim().is_empty()),
            thread_id: self
                .config
                .session_context
                .as_ref()
                .map(|ctx| ctx.thread_id.clone())
                .filter(|value| !value.trim().is_empty()),
            correlation_id: self
                .config
                .session_context
                .as_ref()
                .map(|ctx| ctx.correlation_id.clone())
                .filter(|value| !value.trim().is_empty()),
        });
        self.sessions
            .append(&session_id, &SessionMessage::user(user_message.to_string()))
            .await
            .map_err(anyhow::Error::from)?;

        let listener = self.spawn_persistence_listener(&session_id);

        let mut loop_runner = AgentLoop::new(
            loop_config,
            provider,
            tool_registry,
            skill_runtime,
            policy,
            approval,
            self.event_bus.clone(),
            audit,
        );

        let result = loop_runner.run(user_message).await;
        if let Some(task) = listener {
            let _ = task.await;
        }

        match result {
            Ok(response) => {
                self.sessions
                    .append(
                        &session_id,
                        &SessionMessage::assistant(response.content.clone()),
                    )
                    .await
                    .map_err(anyhow::Error::from)?;
                if !memory_context.text.trim().is_empty() {
                    self.sessions
                        .append(
                            &session_id,
                            &SessionMessage::system_snapshot(memory_context.text.clone()),
                        )
                        .await
                        .map_err(anyhow::Error::from)?;
                }
                let lifecycle = memory_manager
                    .capture_turn_result(
                        &session_id,
                        user_message,
                        &response,
                        &memory_context,
                        &self.config.memory_snapshot_mode,
                    )
                    .await
                    .unwrap_or_default();
                self.persist_lifecycle(&session_id, lifecycle).await;
                Ok(AgentRuntimeResponse {
                    response,
                    stop_reason: StopReason::Completed,
                    memory_context,
                })
            }
            Err(error) => Err(error),
        }
    }

    async fn ensure_session_meta(
        &self,
        session_id: &str,
        provider_id: &str,
        loop_config: &AgentLoopConfig,
    ) -> std::io::Result<()> {
        let now = Utc::now();
        let session_context = self.config.session_context.clone().unwrap_or_default();
        let meta = SessionMeta {
            id: session_id.to_string(),
            name: self
                .config
                .session_name
                .clone()
                .unwrap_or_else(|| "Nova conversa".to_string()),
            updated_at: now,
            message_count: 0,
            provider_id: provider_id.to_string(),
            model_id: loop_config.model_id.clone(),
            workspace_root: loop_config.workspace_root.display().to_string(),
            origin_kind: if session_context.origin_kind.trim().is_empty() {
                "local".to_string()
            } else {
                session_context.origin_kind
            },
            parent_session_id: session_context.parent_session_id,
            status: "active".to_string(),
            created_at: now,
            last_activity_at: now,
            summary: String::new(),
            source_channel: session_context.source_channel,
            thread_id: session_context.thread_id,
            correlation_id: session_context.correlation_id,
        };
        self.sessions.ensure_session_with_meta(meta).await
    }

    async fn persist_lifecycle(&self, session_id: &str, lifecycle: MemoryLifecycleResult) {
        if !lifecycle.summary.trim().is_empty() {
            let _ = self
                .sessions
                .save_summary(
                    session_id,
                    &lifecycle.summary,
                    lifecycle.summary_json.clone(),
                )
                .await;
            self.event_bus.emit(AgentEvent::SessionSummaryUpdated {
                session_id: session_id.to_string(),
                summary: lifecycle.summary.clone(),
            });
        }

        if self.config.memory_snapshot_mode != MemorySnapshotMode::Off
            && !lifecycle.snapshot_text.trim().is_empty()
        {
            let _ = self
                .sessions
                .save_snapshot(
                    session_id,
                    &lifecycle.snapshot_text,
                    lifecycle.snapshot_json.clone(),
                )
                .await;
        }

        let mut records = lifecycle
            .promotions
            .into_iter()
            .map(|decision| decision.record)
            .collect::<Vec<_>>();
        records.extend(response_artifact_records(
            session_id,
            &lifecycle.artifact_records,
        ));
        if !records.is_empty() {
            let _ = self.memory.upsert(&records).await;
        }
    }

    fn spawn_persistence_listener(&self, session_id: &str) -> Option<tokio::task::JoinHandle<()>> {
        if !self.config.persist_tool_events {
            return None;
        }

        let mut subscription = self.event_bus.subscribe();
        let session_id = session_id.to_string();
        let sessions = self.sessions.clone();
        Some(tokio::spawn(async move {
            while let Ok(event) = subscription.recv().await {
                match event {
                    AgentEvent::ToolCallStarted {
                        session_id: event_session_id,
                        tool,
                        params,
                        call_id,
                    } if event_session_id == session_id => {
                        let _ = sessions
                            .append(
                                &session_id,
                                &SessionMessage::tool_call(tool, call_id, params),
                            )
                            .await;
                    }
                    AgentEvent::ToolCallCompleted {
                        session_id: event_session_id,
                        tool,
                        call_id,
                        result,
                        ..
                    } if event_session_id == session_id => {
                        let _ = sessions
                            .append(
                                &session_id,
                                &SessionMessage::tool_result_event(tool, call_id, result),
                            )
                            .await;
                    }
                    AgentEvent::RunCompleted {
                        session_id: event_session_id,
                        ..
                    }
                    | AgentEvent::RunFailed {
                        session_id: event_session_id,
                        ..
                    } if event_session_id == session_id => break,
                    _ => {}
                }
            }
        }))
    }
}

fn default_memory_profile() -> String {
    "balanced".to_string()
}

fn default_toolset_id() -> String {
    "general".to_string()
}

fn merge_system_prompt(
    base: Option<String>,
    session_context: Option<&SessionContextEnvelope>,
    memory_block: &str,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(value) = base.filter(|value| !value.trim().is_empty()) {
        parts.push(value);
    }
    if let Some(context) = session_context {
        if !context.origin_kind.trim().is_empty() || !context.metadata.is_empty() {
            let mut lines = vec!["## Session Context".to_string()];
            if !context.origin_kind.trim().is_empty() {
                lines.push(format!("Origin: {}", context.origin_kind.trim()));
            }
            if let Some(parent) = context.parent_session_id.as_deref() {
                lines.push(format!("Parent session: {parent}"));
            }
            if !context.source_channel.trim().is_empty() {
                lines.push(format!("Source channel: {}", context.source_channel.trim()));
            }
            if !context.thread_id.trim().is_empty() {
                lines.push(format!("Thread: {}", context.thread_id.trim()));
            }
            if !context.sender_id.trim().is_empty() {
                lines.push(format!("Sender: {}", context.sender_id.trim()));
            }
            if !context.correlation_id.trim().is_empty() {
                lines.push(format!("Correlation: {}", context.correlation_id.trim()));
            }
            for (key, value) in &context.metadata {
                if !value.trim().is_empty() {
                    lines.push(format!("{key}: {value}"));
                }
            }
            parts.push(lines.join("\n"));
        }
    }
    if !memory_block.trim().is_empty() {
        parts.push(memory_block.trim().to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn response_artifact_records(
    session_id: &str,
    decisions: &[MemoryPromotionDecision],
) -> Vec<MemoryRecord> {
    decisions
        .iter()
        .map(|decision| {
            let mut record = decision.record.clone();
            if record.session_id.trim().is_empty() {
                record.session_id = session_id.to_string();
            }
            record
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::DefaultApprovalService;
    use crate::audit::AuditLog;
    use crate::policy::{PolicyDecision, PolicyEngine};
    use crate::tool_catalog::ToolProfileName;
    use mlx_agent_tools::ExecutionMode;
    use mlx_ollama_core::{
        ChatMessage, ChatResponse, ChatToolsRequest, MessageRole, ModelDescriptor, ProviderError,
        TokenUsage, ToolCallRequest,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockProvider {
        calls: AtomicUsize,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl ModelProvider for MockProvider {
        fn provider_id(&self) -> &'static str {
            "mock"
        }

        async fn list_models(&self) -> Result<Vec<ModelDescriptor>, ProviderError> {
            Ok(vec![ModelDescriptor {
                id: "mock-model".to_string(),
                name: "Mock Model".to_string(),
                provider: "mock".to_string(),
                path: String::new(),
                is_available: true,
                agent_tool_mode: None,
                agent_tool_reason: None,
                agent_recommended: true,
            }])
        }

        async fn chat(
            &self,
            request: mlx_ollama_core::ChatRequest,
        ) -> Result<ChatResponse, ProviderError> {
            Ok(ChatResponse {
                model_id: request.model_id,
                provider: "mock".to_string(),
                message: ChatMessage::text(MessageRole::Assistant, "done"),
                usage: TokenUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
                latency_ms: 1,
                raw_output: None,
            })
        }

        async fn chat_with_tools(
            &self,
            request: ChatToolsRequest,
        ) -> Result<ChatResponse, ProviderError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Ok(ChatResponse {
                    model_id: request.model_id,
                    provider: "mock".to_string(),
                    message: ChatMessage {
                        role: MessageRole::Assistant,
                        content: String::new(),
                        tool_calls: vec![ToolCallRequest {
                            id: "call_1".to_string(),
                            name: "list_dir".to_string(),
                            arguments: r#"{"path":"."}"#.to_string(),
                        }],
                        tool_call_id: None,
                    },
                    usage: TokenUsage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                    },
                    latency_ms: 1,
                    raw_output: None,
                });
            }

            Ok(ChatResponse {
                model_id: request.model_id,
                provider: "mock".to_string(),
                message: ChatMessage::text(MessageRole::Assistant, "final answer"),
                usage: TokenUsage {
                    prompt_tokens: 12,
                    completion_tokens: 6,
                    total_tokens: 18,
                },
                latency_ms: 1,
                raw_output: None,
            })
        }
    }

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

    #[tokio::test]
    async fn hermes_inspired_runtime_persists_tool_events() {
        let temp = tempfile::tempdir().unwrap();
        let sessions = Arc::new(
            SessionStore::new(temp.path().join("sessions"))
                .await
                .unwrap(),
        );
        let memory = Arc::new(MemoryStore::new(temp.path().join("memory")).await.unwrap());
        let event_bus = Arc::new(EventBus::default());
        let runtime = AgentRuntime::new(
            AgentRuntimeConfig {
                variant: RuntimeVariant::HermesInspired,
                persist_tool_events: true,
                memory_profile: "balanced".to_string(),
                session_search_enabled: false,
                delegate_depth: 0,
                session_context: None,
                session_name: Some("Test".to_string()),
                toolset_id: "general".to_string(),
                memory_snapshot_mode: MemorySnapshotMode::Session,
            },
            sessions.clone(),
            memory,
            event_bus.clone(),
        );

        let response = runtime
            .run(
                AgentLoopConfig {
                    session_id: SessionStore::new_session_id(),
                    model_id: "mock-model".to_string(),
                    workspace_root: temp.path().to_path_buf(),
                    initial_history: Vec::new(),
                    system_prompt: None,
                    max_iterations: 4,
                    max_prompt_tokens: None,
                    max_history_messages: None,
                    max_tools_in_prompt: None,
                    provider_runtime: None,
                    max_tokens_per_turn: 512,
                    temperature: None,
                    aggressive_tool_filtering: false,
                    enable_tool_call_fallback: true,
                    mode: ExecutionMode::Full,
                    tool_profile: ToolProfileName::Coding,
                    skill_filter: None,
                },
                "mock",
                Arc::new(MockProvider::new()),
                crate::registry::ToolRegistry::with_builtins(),
                crate::runtime::SkillRuntime::new(),
                Arc::new(AllowAllPolicy),
                Arc::new(DefaultApprovalService::new()),
                Arc::new(AuditLog::new(temp.path().join("audit"))),
                "list the workspace",
            )
            .await
            .unwrap();

        assert_eq!(response.response.content, "final answer");
        let events = sessions.load(&response.response.session_id).await.unwrap();
        assert!(events.iter().any(|event| event.kind == "tool_call"));
        assert!(events.iter().any(|event| event.kind == "tool_result"));
        assert!(events.iter().any(|event| event.kind == "assistant"));
        let summary = sessions
            .load_summary(&response.response.session_id)
            .await
            .unwrap();
        assert!(summary.is_some());
        let snapshot = sessions
            .latest_snapshot(&response.response.session_id)
            .await
            .unwrap();
        assert!(snapshot.is_some());
        let promoted = runtime.memory.search("list workspace", 10).await.unwrap();
        assert!(!promoted.is_empty());
    }
}
