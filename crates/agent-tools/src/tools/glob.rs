//! `GlobTool` - finds workspace files by glob pattern.

use crate::sandbox::assert_sandbox_path;
use crate::types::{ExecutionMode, ParamSchema, ToolContext, ToolError, ToolResult};
use glob_match::glob_match;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

const DEFAULT_LIMIT: usize = 200;

pub struct GlobTool {
    schema: ParamSchema,
}

impl GlobTool {
    pub fn new() -> Self {
        Self {
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob relativo ao workspace, por exemplo `src/**/*.ts`"
                    },
                    "base_path": {
                        "type": "string",
                        "description": "Diretorio base relativo ao workspace. Default: `.`"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Numero maximo de caminhos retornados. Default: 200"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }
}

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Encontra arquivos por padrao glob dentro do workspace, como `src/**/*.ts`."
    }

    fn parameters(&self) -> &ParamSchema {
        &self.schema
    }

    async fn execute(&self, params: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if ctx.mode == ExecutionMode::Locked {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }

        let pattern = params["pattern"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ToolError::InvalidParams {
                details: "missing 'pattern' string".into(),
            })?;
        let base_path = params["base_path"].as_str().unwrap_or(".");
        let limit = params["limit"]
            .as_u64()
            .map(|value| value.clamp(1, 1000) as usize)
            .unwrap_or(DEFAULT_LIMIT);

        let workspace_root =
            ctx.workspace_root
                .canonicalize()
                .map_err(|error| ToolError::ExecutionFailed {
                    message: format!(
                        "failed to resolve workspace root '{}': {error}",
                        ctx.workspace_root.display()
                    ),
                })?;
        let safe_base = assert_sandbox_path(&ctx.workspace_root, base_path)?;
        let mut matches = Vec::new();
        collect_matches(&workspace_root, &safe_base, pattern, &mut matches, limit)?;
        matches.sort();

        Ok(ToolResult {
            output: if matches.is_empty() {
                "(sem arquivos correspondentes)".to_string()
            } else {
                matches.join("\n")
            },
            is_error: false,
            metadata: HashMap::from([
                ("pattern".to_string(), Value::String(pattern.to_string())),
                (
                    "count".to_string(),
                    Value::Number((matches.len() as u64).into()),
                ),
            ]),
        })
    }
}

fn collect_matches(
    workspace_root: &Path,
    current_dir: &Path,
    pattern: &str,
    matches: &mut Vec<String>,
    limit: usize,
) -> Result<(), ToolError> {
    if matches.len() >= limit {
        return Ok(());
    }

    let entries = std::fs::read_dir(current_dir).map_err(|error| ToolError::ExecutionFailed {
        message: format!("failed to read dir '{}': {error}", current_dir.display()),
    })?;

    for entry in entries {
        if matches.len() >= limit {
            break;
        }

        let entry = entry.map_err(|error| ToolError::ExecutionFailed {
            message: format!("error reading dir entry: {error}"),
        })?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| ToolError::ExecutionFailed {
                message: format!("failed to read metadata '{}': {error}", path.display()),
            })?;

        if metadata.is_dir() {
            collect_matches(workspace_root, &path, pattern, matches, limit)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        let relative = relative_path(workspace_root, &path);
        if matches_glob_pattern(pattern, &relative) {
            matches.push(relative);
        }
    }

    Ok(())
}

fn matches_glob_pattern(pattern: &str, path: &str) -> bool {
    glob_match(pattern, path)
        || pattern
            .contains("/**/")
            .then(|| pattern.replace("/**/", "/"))
            .is_some_and(|alternate| glob_match(&alternate, path))
        || pattern
            .strip_prefix("**/")
            .is_some_and(|alternate| glob_match(alternate, path))
}

fn relative_path(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tool;
    use std::fs;

    #[tokio::test]
    async fn glob_matches_nested_files() {
        let tmp = std::env::temp_dir().join("tool_glob_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src").join("nested")).unwrap();
        fs::write(tmp.join("src").join("main.ts"), "export const ok = true;").unwrap();
        fs::write(
            tmp.join("src").join("nested").join("util.ts"),
            "export const util = true;",
        )
        .unwrap();
        fs::write(tmp.join("README.md"), "# test").unwrap();

        let tool = GlobTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(&serde_json::json!({"pattern": "src/**/*.ts"}), &ctx)
            .await
            .unwrap();

        assert!(result.output.contains("src/main.ts"));
        assert!(result.output.contains("src/nested/util.ts"));
        assert!(!result.output.contains("README.md"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn glob_helper_matches_shallow_and_nested() {
        assert!(matches_glob_pattern("src/**/*.ts", "src/main.ts"));
        assert!(matches_glob_pattern("src/**/*.ts", "src/nested/util.ts"));
    }
}
