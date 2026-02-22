//! The core `Tool` trait that every built-in and MCP tool implements.

use crate::types::{ParamSchema, ToolContext, ToolDefinition, ToolError, ToolResult};
use serde_json::Value;

/// The core tool trait. Every tool implements this.
///
/// Tools are registered in the `ToolRegistry` (in `agent-core`) and are
/// dispatched by the `AgentLoop` when the LLM emits a function call.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (snake_case, e.g. `"read_file"`).
    fn name(&self) -> &str;

    /// Human-readable description for the LLM.
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's parameters.
    fn parameters(&self) -> &ParamSchema;

    /// Execute the tool with validated parameters.
    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError>;

    /// Convert to an OpenAI-compatible function-calling definition.
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters().clone(),
        }
    }
}
