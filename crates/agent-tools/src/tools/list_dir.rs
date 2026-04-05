//! `ListDirTool` — lists directory contents within the sandbox.

use crate::sandbox::assert_sandbox_path;
use crate::types::{ExecutionMode, ParamSchema, ToolContext, ToolError, ToolResult};
use serde_json::Value;
use std::collections::HashMap;

/// Lists the contents of a directory within the workspace.
pub struct ListDirTool {
    schema: ParamSchema,
}

impl ListDirTool {
    pub fn new() -> Self {
        Self {
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path (relative to workspace root). Defaults to '.' if omitted."
                    }
                },
                "required": []
            }),
        }
    }
}

impl Default for ListDirTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List the contents of a directory. Returns file names, types, and sizes. Path is relative to workspace root."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if ctx.mode == ExecutionMode::Locked {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }

        let path_str = params["path"].as_str().unwrap_or(".");
        let safe_path = assert_sandbox_path(&ctx.workspace_root, path_str)?;

        let mut entries = Vec::new();
        let mut dir =
            tokio::fs::read_dir(&safe_path)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("failed to read dir '{}': {e}", safe_path.display()),
                })?;

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("error reading entry: {e}"),
            })?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().await;
            let (kind, size) = match meta {
                Ok(m) => {
                    let kind = if m.is_dir() {
                        "dir"
                    } else if m.is_symlink() {
                        "symlink"
                    } else {
                        "file"
                    };
                    (kind, m.len())
                }
                Err(_) => ("unknown", 0),
            };
            entries.push(format!("{kind}\t{size}\t{name}"));
        }

        entries.sort();

        Ok(ToolResult {
            output: if entries.is_empty() {
                "(empty directory)".into()
            } else {
                entries.join("\n")
            },
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
    async fn list_dir_shows_files() {
        let tmp = std::env::temp_dir().join("tool_list_dir_test");
        fs::create_dir_all(tmp.join("subdir")).unwrap();
        fs::write(tmp.join("a.txt"), "hello").unwrap();
        fs::write(tmp.join("b.txt"), "world").unwrap();

        let tool = ListDirTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert!(result.output.contains("a.txt"));
        assert!(result.output.contains("b.txt"));
        assert!(result.output.contains("subdir"));

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn list_dir_blocks_escape() {
        let tmp = std::env::temp_dir().join("tool_list_escape_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = ListDirTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(serde_json::json!({"path": "../../.."}), &ctx)
            .await;
        assert!(result.is_err());

        fs::remove_dir_all(&tmp).unwrap();
    }
}
