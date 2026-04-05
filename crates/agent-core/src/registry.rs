//! `ToolRegistry` — registration, lookup, dispatch, and JSON Schema
//! validation of tools.

use mlx_agent_tools::{Tool, ToolContext, ToolDefinition, ToolError, ToolResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Central registry of all available tools.
///
/// The `AgentLoop` uses this to convert tool names from the LLM into
/// actual `Tool` implementations and dispatch calls.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Overwrites any existing tool with the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Validate params against the tool's JSON Schema, then dispatch.
    pub async fn dispatch(
        &self,
        tool_name: &str,
        params: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let tool = self
            .tools
            .get(tool_name)
            .ok_or_else(|| ToolError::InvalidParams {
                details: format!("unknown tool: {tool_name}"),
            })?;

        // Validate params against JSON Schema.
        self.validate_params(tool.as_ref(), &params)?;

        tool.execute(params, ctx).await
    }

    /// Validate parameters against a tool's JSON Schema.
    fn validate_params(&self, tool: &dyn Tool, params: &Value) -> Result<(), ToolError> {
        let schema = tool.parameters();

        // Build a JSON Schema validator.
        let validator =
            jsonschema::validator_for(schema).map_err(|e| ToolError::InvalidParams {
                details: format!("invalid schema for tool '{}': {e}", tool.name()),
            })?;

        // Validate and collect errors.
        let errors: Vec<String> = validator
            .iter_errors(params)
            .map(|e| e.to_string())
            .collect();

        if !errors.is_empty() {
            return Err(ToolError::InvalidParams {
                details: format!(
                    "parameter validation failed for '{}': {}",
                    tool.name(),
                    errors.join("; ")
                ),
            });
        }

        Ok(())
    }

    /// Return all tool definitions (for LLM function-calling).
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.to_definition()).collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Returns true when no tools are registered.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Create a registry pre-loaded with all 5 built-in tools.
    pub fn with_builtins() -> Self {
        use mlx_agent_tools::{EditFileTool, ExecTool, ListDirTool, ReadFileTool, WriteFileTool};

        let mut registry = Self::new();
        registry.register(Arc::new(ReadFileTool::new()));
        registry.register(Arc::new(WriteFileTool::new()));
        registry.register(Arc::new(EditFileTool::new()));
        registry.register(Arc::new(ListDirTool::new()));
        registry.register(Arc::new(ExecTool::new()));
        registry
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlx_agent_tools::{ExecutionMode, ToolContext};
    use std::path::PathBuf;

    fn test_ctx() -> ToolContext {
        ToolContext {
            workspace_root: PathBuf::from("."),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        }
    }

    #[test]
    fn empty_registry() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.get("anything").is_none());
        assert!(reg.definitions().is_empty());
    }

    #[test]
    fn with_builtins_has_five_tools() {
        let reg = ToolRegistry::with_builtins();
        assert_eq!(reg.len(), 5);
        assert!(reg.get("read_file").is_some());
        assert!(reg.get("write_file").is_some());
        assert!(reg.get("edit_file").is_some());
        assert!(reg.get("list_dir").is_some());
        assert!(reg.get("exec").is_some());
    }

    #[tokio::test]
    async fn dispatch_unknown_tool_errors() {
        let reg = ToolRegistry::new();
        let result = reg
            .dispatch("nonexistent", serde_json::json!({}), &test_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown tool"));
    }

    #[tokio::test]
    async fn dispatch_validates_schema_rejects_invalid() {
        let reg = ToolRegistry::with_builtins();
        // read_file requires "path" (string), send a number instead.
        let result = reg
            .dispatch("read_file", serde_json::json!({"path": 12345}), &test_ctx())
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("validation failed") || err.contains("invalid"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn dispatch_validates_schema_missing_required() {
        let reg = ToolRegistry::with_builtins();
        // write_file requires "path" and "content".
        let result = reg
            .dispatch(
                "write_file",
                serde_json::json!({"path": "test.txt"}),
                &test_ctx(),
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("validation failed") || err.contains("content"),
            "got: {err}"
        );
    }

    #[test]
    fn definitions_returns_all_tool_schemas() {
        let reg = ToolRegistry::with_builtins();
        let defs = reg.definitions();
        assert_eq!(defs.len(), 5);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"exec"));
    }
}
