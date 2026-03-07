//! `ReadFileTool` — reads a file within the sandbox.

use crate::sandbox::assert_sandbox_path;
use crate::types::{ExecutionMode, ParamSchema, ToolContext, ToolError, ToolResult};
use serde_json::Value;
use std::collections::HashMap;

/// Reads the contents of a file within the workspace.
pub struct ReadFileTool {
    schema: ParamSchema,
}

impl ReadFileTool {
    pub fn new() -> Self {
        Self {
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file (relative to workspace root)"
                    }
                },
                "required": ["path"]
            }),
        }
    }
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Path is relative to the workspace root."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if ctx.mode == ExecutionMode::Locked {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }

        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams {
                details: "missing 'path' string".into(),
            })?;

        let safe_path = assert_sandbox_path(&ctx.workspace_root, path_str)?;

        let content = tokio::fs::read_to_string(&safe_path).await.map_err(|e| {
            ToolError::ExecutionFailed {
                message: format!("failed to read '{}': {e}", safe_path.display()),
            }
        })?;

        Ok(ToolResult {
            output: content,
            is_error: false,
            metadata: HashMap::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tool;
    use std::fs;

    #[tokio::test]
    async fn read_file_success() {
        let tmp = std::env::temp_dir().join("tool_read_file_test");
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("test.txt"), "hello world").unwrap();

        let tool = ReadFileTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(&serde_json::json!({"path": "test.txt"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.output, "hello world");
        assert!(!result.is_error);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn read_file_blocks_escape() {
        let tmp = std::env::temp_dir().join("tool_read_escape_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = ReadFileTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(&serde_json::json!({"path": "../../../etc/passwd"}), &ctx)
            .await;
        assert!(result.is_err());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn read_file_tool_definition() {
        let tool = ReadFileTool::new();
        assert_eq!(tool.name(), "read_file");
        let def = tool.to_definition();
        assert!(def.parameters["required"]
            .as_array()
            .unwrap()
            .contains(&Value::String("path".into())));
    }
}
