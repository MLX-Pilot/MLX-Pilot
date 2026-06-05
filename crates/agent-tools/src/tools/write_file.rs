//! `WriteFileTool` — writes/creates a file within the sandbox.

use crate::checkpoints::record_file_checkpoint;
use crate::sandbox::assert_sandbox_path;
use crate::types::{ExecutionMode, ParamSchema, ToolContext, ToolError, ToolResult};
use serde_json::Value;
use std::collections::HashMap;

/// Writes content to a file within the workspace.
/// Creates parent directories if needed.
pub struct WriteFileTool {
    schema: ParamSchema,
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self {
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to write (relative to workspace root)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file (and parent dirs) if it doesn't exist. Path is relative to workspace root."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        match ctx.mode {
            ExecutionMode::Locked | ExecutionMode::ReadOnly => {
                return Err(ToolError::ModeRestriction { mode: ctx.mode });
            }
            ExecutionMode::DryRun => {
                let path_str = params["path"].as_str().unwrap_or("<missing>");
                return Ok(ToolResult {
                    output: format!("[DRY RUN] would write to '{path_str}'"),
                    is_error: false,
                    metadata: HashMap::new(),
                });
            }
            ExecutionMode::Full => {}
        }

        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams {
                details: "missing 'path' string".into(),
            })?;
        let content = params["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams {
                details: "missing 'content' string".into(),
            })?;

        let safe_path = assert_sandbox_path(&ctx.workspace_root, path_str)?;
        let previous_bytes = if safe_path.exists() {
            Some(
                tokio::fs::read(&safe_path)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed {
                        message: format!("failed to snapshot '{}': {e}", safe_path.display()),
                    })?,
            )
        } else {
            None
        };

        // Create parent directories.
        if let Some(parent) = safe_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("failed to create dirs for '{}': {e}", safe_path.display()),
                })?;
        }

        tokio::fs::write(&safe_path, content)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to write '{}': {e}", safe_path.display()),
            })?;

        let checkpoint = record_file_checkpoint(
            &ctx.workspace_root,
            &ctx.session_id,
            self.name(),
            path_str,
            previous_bytes.as_deref(),
            content.as_bytes(),
        )
        .await?;

        let mut metadata = HashMap::new();
        metadata.insert("checkpoint_id".to_string(), Value::String(checkpoint.id));
        metadata.insert(
            "checkpoint_path".to_string(),
            Value::String(checkpoint.relative_path),
        );

        Ok(ToolResult {
            output: format!("Wrote {} bytes to '{}'", content.len(), path_str),
            is_error: false,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tool;
    use std::fs;

    #[tokio::test]
    async fn write_file_creates_new() {
        let tmp = std::env::temp_dir().join("tool_write_new_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = WriteFileTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(
                &serde_json::json!({"path": "sub/new.txt", "content": "hello"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.output.contains("5 bytes"));
        assert_eq!(
            fs::read_to_string(tmp.join("sub/new.txt")).unwrap(),
            "hello"
        );

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn write_file_blocks_outside_workspace() {
        let tmp = std::env::temp_dir().join("tool_write_escape_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = WriteFileTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(
                &serde_json::json!({"path": "../../escape.txt", "content": "evil"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn write_file_readonly_blocked() {
        let tmp = std::env::temp_dir().join("tool_write_ro_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = WriteFileTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::ReadOnly,
        };

        let result = tool
            .execute(
                &serde_json::json!({"path": "test.txt", "content": "nope"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn write_file_dry_run() {
        let tmp = std::env::temp_dir().join("tool_write_dry_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = WriteFileTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::DryRun,
        };

        let result = tool
            .execute(
                &serde_json::json!({"path": "test.txt", "content": "data"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.output.contains("DRY RUN"));
        assert!(!tmp.join("test.txt").exists());

        fs::remove_dir_all(&tmp).unwrap();
    }
}
