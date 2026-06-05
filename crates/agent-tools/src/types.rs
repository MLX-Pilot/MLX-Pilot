//! Core types for the tool system.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// JSON Schema for a tool's parameters.
/// Must be `{"type": "object", "properties": {...}}`.
pub type ParamSchema = Value;

/// Metadata about a tool for LLM function-calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: ParamSchema,
}

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
}

/// Context passed to every tool execution.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub workspace_root: PathBuf,
    pub session_id: String,
    pub active_skill: Option<String>,
    pub mode: ExecutionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionDomain {
    Inference,
    #[default]
    Tools,
    Memory,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPriority {
    Low,
    #[default]
    Normal,
    High,
}

/// Controls what a tool is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExecutionMode {
    /// Full access — tool can read, write, execute.
    #[default]
    Full,
    /// Read-only — tool may read files/network but not mutate.
    ReadOnly,
    /// Dry-run — tool logs what it *would* do but makes no changes.
    DryRun,
    /// Locked — tool execution is completely disabled.
    Locked,
}

/// Errors that can occur during tool execution.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("invalid parameters: {details}")]
    InvalidParams { details: String },

    #[error("execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("permission denied: {reason}")]
    PermissionDenied { reason: String },

    #[error("tool timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("mode {mode:?} does not allow this operation")]
    ModeRestriction { mode: ExecutionMode },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_mode_default_is_full() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Full);
    }

    #[test]
    fn tool_definition_serializes() {
        let def = ToolDefinition {
            name: "read_file".into(),
            description: "Reads a file".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        assert!(json.contains("read_file"));
    }

    #[test]
    fn tool_result_serializes() {
        let result = ToolResult {
            output: "hello".into(),
            is_error: false,
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("hello"));
    }
}
