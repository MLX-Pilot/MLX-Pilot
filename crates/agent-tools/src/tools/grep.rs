//! `GrepTool` - searches workspace files with regular expressions.

use crate::sandbox::assert_sandbox_path;
use crate::types::{ExecutionMode, ParamSchema, ToolContext, ToolError, ToolResult};
use glob_match::glob_match;
use regex::RegexBuilder;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

const DEFAULT_LIMIT: usize = 200;
const MAX_FILE_BYTES: u64 = 1_048_576;

pub struct GrepTool {
    schema: ParamSchema,
}

impl GrepTool {
    pub fn new() -> Self {
        Self {
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Expressao regular a procurar"
                    },
                    "path": {
                        "type": "string",
                        "description": "Glob relativo ao workspace para filtrar arquivos. Default: `**/*`"
                    },
                    "base_path": {
                        "type": "string",
                        "description": "Diretorio base relativo ao workspace. Default: `.`"
                    },
                    "case_sensitive": {
                        "type": "boolean",
                        "description": "Se verdadeiro, preserva caixa. Default: false"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Numero maximo de matches retornados. Use 0 ou omita para o default (200). Maximo: 1000"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Procura texto ou regex em arquivos do workspace e retorna caminhos, linhas e trechos."
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
        // `path` e um filtro opcional. Modelos mandam `"path": ""` querendo dizer "sem
        // filtro"; tratar a string vazia como glob fazia a busca nao casar com nada e o
        // agente concluia que o termo nao existia no projeto.
        let path_glob = params["path"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("**/*");
        let base_path = params["base_path"].as_str().unwrap_or(".");
        let case_sensitive = params["case_sensitive"].as_bool().unwrap_or(false);
        // `limit: 0` vem do modelo querendo dizer "sem limite". Fazer clamp(1, ..) devolvia
        // exatamente UM match, e o agente concluia que as outras ocorrencias nao existiam.
        // Tratar 0 como "use o default" e o comportamento menos surpreendente.
        let limit = params["limit"]
            .as_u64()
            .filter(|value| *value > 0)
            .map(|value| value.min(1000) as usize)
            .unwrap_or(DEFAULT_LIMIT);

        let regex = RegexBuilder::new(pattern)
            .case_insensitive(!case_sensitive)
            .build()
            .map_err(|error| ToolError::InvalidParams {
                details: format!("invalid regex: {error}"),
            })?;
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
        collect_matches(
            &workspace_root,
            &safe_base,
            path_glob,
            &regex,
            &mut matches,
            limit,
        )?;

        Ok(ToolResult {
            output: if matches.is_empty() {
                "(sem ocorrencias)".to_string()
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
    path_glob: &str,
    regex: &regex::Regex,
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
            collect_matches(workspace_root, &path, path_glob, regex, matches, limit)?;
            continue;
        }

        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            continue;
        }

        let relative = relative_path(workspace_root, &path);
        if !matches_glob_pattern(path_glob, &relative) {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(value) => value,
            Err(_) => continue,
        };

        for (index, line) in content.lines().enumerate() {
            if matches.len() >= limit {
                break;
            }
            if regex.is_match(line) {
                matches.push(format!("{relative}:{}:{}", index + 1, line.trim()));
            }
        }
    }

    Ok(())
}

fn matches_glob_pattern(pattern: &str, path: &str) -> bool {
    if glob_match(pattern, path)
        || pattern
            .contains("/**/")
            .then(|| pattern.replace("/**/", "/"))
            .is_some_and(|alternate| glob_match(&alternate, path))
        || pattern
            .strip_prefix("**/")
            .is_some_and(|alternate| glob_match(alternate, path))
    {
        return true;
    }

    // Mesma regra do `glob`: um filtro sem barra ("*.rs") e busca por nome de arquivo,
    // nao um arquivo ancorado na raiz.
    if !pattern.contains('/') {
        let file_name = path.rsplit('/').next().unwrap_or(path);
        return glob_match(pattern, file_name);
    }

    false
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
    async fn grep_finds_matches_with_line_numbers() {
        let tmp = std::env::temp_dir().join("tool_grep_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(
            tmp.join("src").join("main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();

        let tool = GrepTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(
                &serde_json::json!({"pattern": "println", "path": "src/**/*.rs"}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.output.contains("src/main.rs:2:println!(\"hello\");"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn grep_glob_helper_matches_shallow_and_nested() {
        assert!(matches_glob_pattern("src/**/*.rs", "src/main.rs"));
        assert!(matches_glob_pattern("src/**/*.rs", "src/nested/lib.rs"));

        // Filtro sem barra busca pelo nome do arquivo em qualquer profundidade.
        assert!(matches_glob_pattern("*.rs", "src/nested/lib.rs"));
        assert!(!matches_glob_pattern("*.rs", "src/main.ts"));
    }

    /// Regressão: o modelo manda `"path": ""` querendo dizer "sem filtro". Tratar a
    /// string vazia como glob fazia a busca não casar com nada, e o agente respondia que
    /// o termo não existia no projeto.
    #[tokio::test]
    async fn grep_treats_empty_path_filter_as_no_filter() {
        let tmp = std::env::temp_dir().join(format!("mlx-grep-empty-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(
            tmp.join("src/config.rs"),
            "pub const MAX_RETRIES: u32 = 3;\n",
        )
        .unwrap();

        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            mode: ExecutionMode::Full,
            session_id: "test".to_string(),
            active_skill: None,
        };
        let result = GrepTool::new()
            .execute(
                &serde_json::json!({"pattern": "MAX_RETRIES", "path": "", "limit": 0}),
                &ctx,
            )
            .await
            .unwrap();

        assert!(
            result.output.contains("src/config.rs"),
            "obtido: {}",
            result.output
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}
