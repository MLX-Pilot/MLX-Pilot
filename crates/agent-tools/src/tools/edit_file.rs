//! `EditFileTool` — find-and-replace edit within the sandbox.

use crate::sandbox::assert_sandbox_path;
use crate::types::{ExecutionMode, ParamSchema, ToolContext, ToolError, ToolResult};
use serde_json::Value;
use std::collections::HashMap;

/// Edits a file by replacing `old_text` with `new_text`.
pub struct EditFileTool {
    schema: ParamSchema,
}

impl EditFileTool {
    pub fn new() -> Self {
        Self {
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file (relative to workspace root)"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Exact text to find and replace"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "Replacement text"
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        }
    }
}

impl Default for EditFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing exact text. Finds `old_text` and replaces it with `new_text`. Path is relative to workspace root."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        match ctx.mode {
            ExecutionMode::Locked | ExecutionMode::ReadOnly => {
                return Err(ToolError::ModeRestriction { mode: ctx.mode });
            }
            ExecutionMode::DryRun => {
                let path_str = params["path"].as_str().unwrap_or("<missing>");
                return Ok(ToolResult {
                    output: format!("[DRY RUN] would edit '{path_str}'"),
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
        let old_text = params["old_text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams {
                details: "missing 'old_text' string".into(),
            })?;
        let new_text = params["new_text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams {
                details: "missing 'new_text' string".into(),
            })?;

        let safe_path = assert_sandbox_path(&ctx.workspace_root, path_str)?;

        let content = tokio::fs::read_to_string(&safe_path).await.map_err(|e| {
            ToolError::ExecutionFailed {
                message: format!("failed to read '{}': {e}", safe_path.display()),
            }
        })?;

        if !content.contains(old_text) {
            return Err(ToolError::ExecutionFailed {
                message: format!("'old_text' not found in '{path_str}'"),
            });
        }

        let new_content = content.replacen(old_text, new_text, 1);
        tokio::fs::write(&safe_path, &new_content)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to write '{}': {e}", safe_path.display()),
            })?;

        Ok(ToolResult {
            output: format!(
                "Edited '{}': replaced {} bytes with {} bytes",
                path_str,
                old_text.len(),
                new_text.len()
            ),
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
    async fn edit_file_replaces_text() {
        let tmp = std::env::temp_dir().join("tool_edit_test");
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("file.txt"), "hello world").unwrap();

        let tool = EditFileTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "file.txt",
                    "old_text": "world",
                    "new_text": "rust"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("Edited"));
        assert_eq!(
            fs::read_to_string(tmp.join("file.txt")).unwrap(),
            "hello rust"
        );

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn edit_file_errors_when_not_found() {
        let tmp = std::env::temp_dir().join("tool_edit_notfound_test");
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("file.txt"), "hello").unwrap();

        let tool = EditFileTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(
                serde_json::json!({
                    "path": "file.txt",
                    "old_text": "nonexistent",
                    "new_text": "replacement"
                }),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        fs::remove_dir_all(&tmp).unwrap();
    }
}
