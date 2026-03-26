//! `ToolRegistry` — registration, lookup, and dispatch of tools.

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

    /// Dispatch a tool call by name.
    pub async fn dispatch(
        &self,
        tool_name: &str,
        params: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let tool = self.tools.get(tool_name).ok_or_else(|| ToolError::InvalidParams {
            details: format!("unknown tool: {tool_name}"),
        })?;
        tool.execute(params, ctx).await
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
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.get("anything").is_none());
        assert!(reg.definitions().is_empty());
    }
}
