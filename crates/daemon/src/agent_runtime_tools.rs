use crate::channels::{ChannelService, MessageSendRequest};
use mlx_agent_core::{
    ContextBudgetTelemetry, MemoryStore, SessionMessage, SessionStore, ToolRegistry,
};
use mlx_agent_tools::{ParamSchema, Tool, ToolContext, ToolError, ToolResult};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[derive(Clone)]
pub struct RuntimeToolServices {
    pub sessions: Arc<SessionStore>,
    pub channels: Arc<ChannelService>,
    pub memory: Arc<MemoryStore>,
    pub budget_tracker: Arc<tokio::sync::RwLock<BTreeMap<String, ContextBudgetTelemetry>>>,
}

pub fn register_runtime_tools(registry: &mut ToolRegistry, services: &RuntimeToolServices) {
    registry.register(Arc::new(MessageTool::new(services.channels.clone())));
    registry.register(Arc::new(SessionsListTool::new(services.sessions.clone())));
    registry.register(Arc::new(SessionsHistoryTool::new(
        services.sessions.clone(),
    )));
    registry.register(Arc::new(SessionsSpawnTool::new(services.sessions.clone())));
    registry.register(Arc::new(SessionsSendTool::new(services.sessions.clone())));
    registry.register(Arc::new(SessionsStatusTool::new(
        services.sessions.clone(),
        services.budget_tracker.clone(),
    )));
    registry.register(Arc::new(MemorySearchTool::new(services.memory.clone())));
    registry.register(Arc::new(MemoryGetTool::new(services.memory.clone())));
}

struct MessageTool {
    channels: Arc<ChannelService>,
    schema: ParamSchema,
}

impl MessageTool {
    fn new(channels: Arc<ChannelService>) -> Self {
        Self {
            channels,
            schema: json!({
                "type": "object",
                "properties": {
                    "channel": { "type": "string" },
                    "account_id": { "type": "string" },
                    "preferred_account_id": { "type": "string" },
                    "target": { "type": "string" },
                    "message": { "type": "string" }
                },
                "required": ["channel", "target", "message"]
            }),
        }
    }
}

#[async_trait::async_trait]
impl Tool for MessageTool {
    fn name(&self) -> &str {
        "message"
    }

    fn description(&self) -> &str {
        "Send a message through a configured channel account."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        match ctx.mode {
            mlx_agent_tools::ExecutionMode::Locked | mlx_agent_tools::ExecutionMode::ReadOnly => {
                return Err(ToolError::ModeRestriction { mode: ctx.mode });
            }
            mlx_agent_tools::ExecutionMode::DryRun => {
                return Ok(ok_text(format!(
                    "[DRY RUN] would send message to {} via {}",
                    params["target"].as_str().unwrap_or("<missing>"),
                    params["channel"].as_str().unwrap_or("<missing>")
                )));
            }
            mlx_agent_tools::ExecutionMode::Full => {}
        }

        let request = MessageSendRequest {
            channel: required_string(params, "channel")?,
            account_id: optional_string(params, "account_id"),
            preferred_account_id: optional_string(params, "preferred_account_id"),
            target: required_string(params, "target")?,
            message: required_string(params, "message")?,
        };

        let response = self
            .channels
            .send_message(request)
            .await
            .map_err(to_execution_error)?;

        Ok(ok_json(json!(response)))
    }
}

struct SessionsListTool {
    sessions: Arc<SessionStore>,
    schema: ParamSchema,
}

impl SessionsListTool {
    fn new(sessions: Arc<SessionStore>) -> Self {
        Self {
            sessions,
            schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer" }
                }
            }),
        }
    }
}

#[async_trait::async_trait]
impl Tool for SessionsListTool {
    fn name(&self) -> &str {
        "sessions_list"
    }

    fn description(&self) -> &str {
        "List locally stored agent sessions."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if matches!(ctx.mode, mlx_agent_tools::ExecutionMode::Locked) {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }

        let limit = params["limit"].as_u64().unwrap_or(50) as usize;
        let mut sessions = self.sessions.list_sessions().await.map_err(io_error)?;
        sessions.truncate(limit.clamp(1, 200));
        Ok(ok_json(json!(sessions)))
    }
}

struct SessionsHistoryTool {
    sessions: Arc<SessionStore>,
    schema: ParamSchema,
}

impl SessionsHistoryTool {
    fn new(sessions: Arc<SessionStore>) -> Self {
        Self {
            sessions,
            schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }),
        }
    }
}

#[async_trait::async_trait]
impl Tool for SessionsHistoryTool {
    fn name(&self) -> &str {
        "sessions_history"
    }

    fn description(&self) -> &str {
        "Read message history from a local agent session."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if matches!(ctx.mode, mlx_agent_tools::ExecutionMode::Locked) {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }

        let session_id =
            optional_string(params, "session_id").unwrap_or_else(|| ctx.session_id.clone());
        let mut messages = self.sessions.load(&session_id).await.map_err(io_error)?;
        let limit = params["limit"].as_u64().unwrap_or(messages.len() as u64) as usize;
        if messages.len() > limit {
            messages = messages.split_off(messages.len().saturating_sub(limit));
        }
        Ok(ok_json(json!({
            "session_id": session_id,
            "messages": messages,
        })))
    }
}

struct SessionsSpawnTool {
    sessions: Arc<SessionStore>,
    schema: ParamSchema,
}

impl SessionsSpawnTool {
    fn new(sessions: Arc<SessionStore>) -> Self {
        Self {
            sessions,
            schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                }
            }),
        }
    }
}

#[async_trait::async_trait]
impl Tool for SessionsSpawnTool {
    fn name(&self) -> &str {
        "sessions_spawn"
    }

    fn description(&self) -> &str {
        "Create a new local agent session."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        match ctx.mode {
            mlx_agent_tools::ExecutionMode::Locked | mlx_agent_tools::ExecutionMode::ReadOnly => {
                return Err(ToolError::ModeRestriction { mode: ctx.mode });
            }
            mlx_agent_tools::ExecutionMode::DryRun => {
                return Ok(ok_text("[DRY RUN] would create a session".to_string()));
            }
            mlx_agent_tools::ExecutionMode::Full => {}
        }

        let session_id = SessionStore::new_session_id();
        let name = optional_string(params, "name");
        self.sessions
            .ensure_session(&session_id, name.clone())
            .await
            .map_err(io_error)?;
        Ok(ok_json(json!({
            "session_id": session_id,
            "name": name.unwrap_or_else(|| "Nova conversa".to_string()),
        })))
    }
}

struct SessionsSendTool {
    sessions: Arc<SessionStore>,
    schema: ParamSchema,
}

impl SessionsSendTool {
    fn new(sessions: Arc<SessionStore>) -> Self {
        Self {
            sessions,
            schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "role": { "type": "string", "enum": ["user", "assistant", "tool"] },
                    "message": { "type": "string" }
                },
                "required": ["message"]
            }),
        }
    }
}

#[async_trait::async_trait]
impl Tool for SessionsSendTool {
    fn name(&self) -> &str {
        "sessions_send"
    }

    fn description(&self) -> &str {
        "Append a message to a local agent session."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        match ctx.mode {
            mlx_agent_tools::ExecutionMode::Locked | mlx_agent_tools::ExecutionMode::ReadOnly => {
                return Err(ToolError::ModeRestriction { mode: ctx.mode });
            }
            mlx_agent_tools::ExecutionMode::DryRun => {
                return Ok(ok_text(format!(
                    "[DRY RUN] would append a message to session {}",
                    optional_string(params, "session_id").unwrap_or_else(|| ctx.session_id.clone())
                )));
            }
            mlx_agent_tools::ExecutionMode::Full => {}
        }

        let session_id =
            optional_string(params, "session_id").unwrap_or_else(|| ctx.session_id.clone());
        let role = optional_string(params, "role").unwrap_or_else(|| "user".to_string());
        let message = required_string(params, "message")?;
        self.sessions
            .append(
                &session_id,
                &SessionMessage {
                    role,
                    content: message.clone(),
                    tool_call_id: None,
                    tool_name: None,
                    timestamp: chrono::Utc::now(),
                },
            )
            .await
            .map_err(io_error)?;

        Ok(ok_json(json!({
            "session_id": session_id,
            "appended": true,
            "message_preview": preview(&message),
        })))
    }
}

struct SessionsStatusTool {
    sessions: Arc<SessionStore>,
    budget_tracker: Arc<tokio::sync::RwLock<BTreeMap<String, ContextBudgetTelemetry>>>,
    schema: ParamSchema,
}

impl SessionsStatusTool {
    fn new(
        sessions: Arc<SessionStore>,
        budget_tracker: Arc<tokio::sync::RwLock<BTreeMap<String, ContextBudgetTelemetry>>>,
    ) -> Self {
        Self {
            sessions,
            budget_tracker,
            schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                }
            }),
        }
    }
}

#[async_trait::async_trait]
impl Tool for SessionsStatusTool {
    fn name(&self) -> &str {
        "sessions_status"
    }

    fn description(&self) -> &str {
        "Inspect metadata and current budget status for a local agent session."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if matches!(ctx.mode, mlx_agent_tools::ExecutionMode::Locked) {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }

        let session_id =
            optional_string(params, "session_id").unwrap_or_else(|| ctx.session_id.clone());
        let meta = self
            .sessions
            .list_sessions()
            .await
            .map_err(io_error)?
            .into_iter()
            .find(|entry| entry.id == session_id);
        let budget = self.budget_tracker.read().await.get(&session_id).cloned();

        Ok(ok_json(json!({
            "session_id": session_id,
            "meta": meta,
            "budget": budget,
        })))
    }
}

struct MemorySearchTool {
    memory: Arc<MemoryStore>,
    schema: ParamSchema,
}

impl MemorySearchTool {
    fn new(memory: Arc<MemoryStore>) -> Self {
        Self {
            memory,
            schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["query"]
            }),
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search locally stored compact memory artifacts."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if matches!(ctx.mode, mlx_agent_tools::ExecutionMode::Locked) {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }
        let query = required_string(params, "query")?;
        let limit = params["limit"].as_u64().unwrap_or(10) as usize;
        let results = self.memory.search(&query, limit).await.map_err(io_error)?;
        Ok(ok_json(json!(results)))
    }
}

struct MemoryGetTool {
    memory: Arc<MemoryStore>,
    schema: ParamSchema,
}

impl MemoryGetTool {
    fn new(memory: Arc<MemoryStore>) -> Self {
        Self {
            memory,
            schema: json!({
                "type": "object",
                "properties": {
                    "memory_id": { "type": "string" }
                },
                "required": ["memory_id"]
            }),
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }

    fn description(&self) -> &str {
        "Fetch a locally stored compact memory artifact by id."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if matches!(ctx.mode, mlx_agent_tools::ExecutionMode::Locked) {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }
        let memory_id = required_string(params, "memory_id")?;
        let record = self.memory.get(&memory_id).await.map_err(io_error)?;
        Ok(ok_json(json!({
            "memory": record,
        })))
    }
}

fn required_string(params: &Value, key: &str) -> Result<String, ToolError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ToolError::InvalidParams {
            details: format!("missing '{key}' string"),
        })
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn ok_json(value: Value) -> ToolResult {
    ToolResult {
        output: serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        is_error: false,
        metadata: HashMap::new(),
    }
}

fn ok_text(output: String) -> ToolResult {
    ToolResult {
        output,
        is_error: false,
        metadata: HashMap::new(),
    }
}

fn to_execution_error(error: String) -> ToolError {
    ToolError::ExecutionFailed { message: error }
}

fn io_error(error: std::io::Error) -> ToolError {
    ToolError::ExecutionFailed {
        message: error.to_string(),
    }
}

fn preview(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= 120 {
        return compact;
    }
    let mut out = compact.chars().take(117).collect::<String>();
    out.push_str("...");
    out
}
