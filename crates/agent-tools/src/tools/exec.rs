//! `ExecTool` — direct process execution with queueing, timeouts, and shell hardening.

use crate::scheduler::schedule_exec_task;
use crate::types::{
    ExecutionDomain, ExecutionMode, ExecutionPriority, ParamSchema, ToolContext, ToolError,
    ToolResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::AsyncReadExt;

/// Default timeout for command execution (30 seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Commands that are always denied.
const DENY_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "sudo ",
    "chmod 777",
    "mkfs",
    "dd if=",
    ":(){:|:&};:",
    "format ",
    "del /f /s /q",
    "rd /s /q C:",
    "powershell -ep bypass",
];

/// Shell metacharacters and chaining constructs that are not allowed.
const SHELL_META_PATTERNS: &[&str] = &["&&", "||", "|", ";", ">", "<", "`", "$(", "\n", "\r"];

/// Maximum output size in bytes.
const MAX_OUTPUT_BYTES: usize = 256 * 1024; // 256 KB
const STREAM_CHUNK_BYTES: usize = 8 * 1024;

/// Executes a command within the workspace without an intermediary shell.
pub struct ExecTool {
    schema: ParamSchema,
}

impl ExecTool {
    pub fn new() -> Self {
        Self {
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command line to execute directly without shell metacharacters"
                    },
                    "argv": {
                        "type": "array",
                        "description": "Preferred explicit argv form: [program, arg1, ...]",
                        "items": { "type": "string" },
                        "minItems": 1
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 30)"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["low", "normal", "high"],
                        "description": "Scheduling priority inside the local execution queue"
                    }
                },
                "anyOf": [
                    { "required": ["command"] },
                    { "required": ["argv"] }
                ]
            }),
        }
    }

    /// Check if a command matches any deny pattern.
    fn is_denied(command: &str) -> Option<&'static str> {
        let lower = command.to_lowercase();
        DENY_PATTERNS
            .iter()
            .find(|pattern| lower.contains(*pattern))
            .copied()
    }

    fn contains_shell_metacharacters(command: &str) -> Option<&'static str> {
        SHELL_META_PATTERNS
            .iter()
            .find(|pattern| command.contains(*pattern))
            .copied()
    }
}

impl Default for ExecTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ExecEnvelope {
    output: String,
    is_error: bool,
    exit_code: i64,
    program: String,
    argv: Vec<String>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

#[async_trait::async_trait]
impl crate::Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Execute a direct program invocation in the workspace directory. Commands are queued, timed out, and shell operators like pipes or redirection are blocked."
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
                let preview = command_preview(params);
                return Ok(ToolResult {
                    output: format!("[DRY RUN] would exec: {preview}"),
                    is_error: false,
                    metadata: HashMap::new(),
                });
            }
            ExecutionMode::Full => {}
        }

        let argv = resolve_argv(params)?;
        let timeout_secs = params["timeout_secs"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        let priority = parse_priority(params.get("priority"));

        let command_preview = argv.join(" ");
        if let Some(pattern) = Self::is_denied(&command_preview) {
            return Err(ToolError::PermissionDenied {
                reason: format!("command matches deny pattern: '{pattern}'"),
            });
        }

        let workspace_root = ctx.workspace_root.clone();
        let task_argv = argv.clone();
        let (serialized, scheduling) =
            schedule_exec_task(ExecutionDomain::System, priority, async move {
                let envelope =
                    execute_direct_process(task_argv, workspace_root, timeout_secs).await?;
                serde_json::to_vec(&envelope).map_err(|error| ToolError::ExecutionFailed {
                    message: format!("failed to encode exec result: {error}"),
                })
            })
            .await?;
        let envelope: ExecEnvelope =
            serde_json::from_slice(&serialized).map_err(|error| ToolError::ExecutionFailed {
                message: format!("failed to decode exec result: {error}"),
            })?;

        let mut metadata = HashMap::new();
        metadata.insert("exit_code".into(), Value::Number(envelope.exit_code.into()));
        metadata.insert(
            "program".into(),
            Value::String(envelope.program.to_string()),
        );
        metadata.insert(
            "argv".into(),
            Value::Array(envelope.argv.iter().cloned().map(Value::String).collect()),
        );
        metadata.insert(
            "queue_wait_ms".into(),
            Value::Number((scheduling.queue_wait.as_millis() as u64).into()),
        );
        metadata.insert(
            "execution_domain".into(),
            Value::String("system".to_string()),
        );
        metadata.insert(
            "priority".into(),
            Value::String(
                match priority {
                    ExecutionPriority::Low => "low",
                    ExecutionPriority::Normal => "normal",
                    ExecutionPriority::High => "high",
                }
                .to_string(),
            ),
        );
        metadata.insert(
            "stdout_truncated".into(),
            Value::Bool(envelope.stdout_truncated),
        );
        metadata.insert(
            "stderr_truncated".into(),
            Value::Bool(envelope.stderr_truncated),
        );

        Ok(ToolResult {
            output: envelope.output,
            is_error: envelope.is_error,
            metadata,
        })
    }
}

async fn execute_direct_process(
    argv: Vec<String>,
    workspace_root: std::path::PathBuf,
    timeout_secs: u64,
) -> Result<ExecEnvelope, ToolError> {
    let program = argv
        .first()
        .cloned()
        .ok_or_else(|| ToolError::InvalidParams {
            details: "missing program in argv".to_string(),
        })?;
    let args = argv.iter().skip(1).cloned().collect::<Vec<_>>();

    let mut child = tokio::process::Command::new(&program)
        .args(&args)
        .current_dir(&workspace_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("failed to spawn command '{}': {e}", program),
        })?;

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    let status = match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            return Err(ToolError::ExecutionFailed {
                message: format!("command failed: {e}"),
            });
        }
        Err(_) => {
            drop(child);
            return Err(ToolError::Timeout {
                seconds: timeout_secs,
            });
        }
    };

    let (stdout_bytes, stdout_truncated) = if let Some(mut handle) = stdout_handle {
        read_limited_output(&mut handle, MAX_OUTPUT_BYTES).await
    } else {
        (Vec::new(), false)
    };
    let (stderr_bytes, stderr_truncated) = if let Some(mut handle) = stderr_handle {
        read_limited_output(&mut handle, MAX_OUTPUT_BYTES).await
    } else {
        (Vec::new(), false)
    };

    let mut stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
    let mut stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();
    if stdout_truncated {
        stdout.push_str("\n... (stdout truncated)");
    }
    if stderr_truncated {
        stderr.push_str("\n... (stderr truncated)");
    }

    let exit_code = status.code().unwrap_or(-1) as i64;
    let is_error = !status.success();
    let combined = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        format!("--- stderr ---\n{stderr}")
    } else {
        format!("{stdout}\n--- stderr ---\n{stderr}")
    };

    Ok(ExecEnvelope {
        output: combined,
        is_error,
        exit_code,
        program,
        argv,
        stdout_truncated,
        stderr_truncated,
    })
}

fn resolve_argv(params: &Value) -> Result<Vec<String>, ToolError> {
    if let Some(argv) = params.get("argv").and_then(Value::as_array) {
        let values = argv
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .ok_or_else(|| ToolError::InvalidParams {
                        details: "argv entries must be non-empty strings".to_string(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if values.is_empty() {
            return Err(ToolError::InvalidParams {
                details: "argv cannot be empty".to_string(),
            });
        }
        return Ok(values);
    }

    let command = params["command"]
        .as_str()
        .ok_or_else(|| ToolError::InvalidParams {
            details: "missing 'command' string".into(),
        })?;

    if let Some(pattern) = ExecTool::contains_shell_metacharacters(command) {
        return Err(ToolError::PermissionDenied {
            reason: format!(
                "shell operator '{pattern}' is blocked; provide direct argv without pipes or redirects"
            ),
        });
    }

    tokenize_command(command)
}

fn parse_priority(value: Option<&Value>) -> ExecutionPriority {
    match value.and_then(Value::as_str).unwrap_or("normal") {
        "low" => ExecutionPriority::Low,
        "high" => ExecutionPriority::High,
        _ => ExecutionPriority::Normal,
    }
}

fn command_preview(params: &Value) -> String {
    if let Some(argv) = params.get("argv").and_then(Value::as_array) {
        let joined = argv
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        if !joined.is_empty() {
            return joined;
        }
    }
    params["command"]
        .as_str()
        .unwrap_or("<missing>")
        .to_string()
}

fn tokenize_command(command: &str) -> Result<Vec<String>, ToolError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = QuoteMode::None;
    let mut escape = false;

    for ch in command.chars() {
        if escape {
            current.push(ch);
            escape = false;
            continue;
        }

        match quote {
            QuoteMode::Single => {
                if ch == '\'' {
                    quote = QuoteMode::None;
                } else {
                    current.push(ch);
                }
            }
            QuoteMode::Double => match ch {
                '"' => quote = QuoteMode::None,
                '\\' => escape = true,
                _ => current.push(ch),
            },
            QuoteMode::None => match ch {
                '\'' => quote = QuoteMode::Single,
                '"' => quote = QuoteMode::Double,
                '\\' => escape = true,
                ch if ch.is_whitespace() => {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                }
                _ => current.push(ch),
            },
        }
    }

    if escape {
        return Err(ToolError::InvalidParams {
            details: "command ends with dangling escape".to_string(),
        });
    }
    if !matches!(quote, QuoteMode::None) {
        return Err(ToolError::InvalidParams {
            details: "command contains unclosed quotes".to_string(),
        });
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.is_empty() {
        return Err(ToolError::InvalidParams {
            details: "command cannot be empty".to_string(),
        });
    }
    Ok(tokens)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteMode {
    None,
    Single,
    Double,
}

async fn read_limited_output(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    max_bytes: usize,
) -> (Vec<u8>, bool) {
    let mut output = Vec::with_capacity(max_bytes.min(STREAM_CHUNK_BYTES * 2));
    let mut buffer = [0_u8; STREAM_CHUNK_BYTES];
    let mut truncated = false;

    loop {
        let bytes_read = match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(size) => size,
            Err(_) => break,
        };

        if output.len() < max_bytes {
            let remaining = max_bytes - output.len();
            let keep = remaining.min(bytes_read);
            output.extend_from_slice(&buffer[..keep]);
            if keep < bytes_read {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }

    (output, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tool;
    use std::fs;

    #[test]
    fn denies_dangerous_commands() {
        assert!(ExecTool::is_denied("sudo rm -rf /").is_some());
        assert!(ExecTool::is_denied("chmod 777 /etc").is_some());
        assert!(ExecTool::is_denied("echo hello").is_none());
        assert!(ExecTool::is_denied("git status").is_none());
    }

    #[test]
    fn blocks_shell_metacharacters() {
        assert!(ExecTool::contains_shell_metacharacters("git status | cat").is_some());
        assert!(ExecTool::contains_shell_metacharacters("cargo test && cargo fmt").is_some());
        assert!(ExecTool::contains_shell_metacharacters("echo ok").is_none());
    }

    #[test]
    fn tokenizes_quoted_arguments() {
        let tokens = tokenize_command("git commit -m \"hello world\"").unwrap();
        assert_eq!(tokens, vec!["git", "commit", "-m", "hello world"]);
    }

    #[tokio::test]
    async fn exec_runs_simple_command() {
        let tmp = std::env::temp_dir().join("tool_exec_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = ExecTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let payload = if cfg!(windows) {
            serde_json::json!({"argv": ["cmd", "/C", "echo", "hello"]})
        } else {
            serde_json::json!({"argv": ["echo", "hello"]})
        };
        let result = tool.execute(&payload, &ctx).await.unwrap();
        assert!(result.output.contains("hello"));
        assert!(!result.is_error);

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn exec_blocks_denied_command() {
        let tmp = std::env::temp_dir().join("tool_exec_deny_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = ExecTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let result = tool
            .execute(&serde_json::json!({"command": "sudo rm -rf /"}), &ctx)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("deny pattern"), "got: {err}");

        fs::remove_dir_all(&tmp).unwrap();
    }

    #[tokio::test]
    async fn exec_timeout() {
        let tmp = std::env::temp_dir().join("tool_exec_timeout_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = ExecTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        };

        let argv = if cfg!(windows) {
            serde_json::json!(["ping", "-n", "10", "127.0.0.1"])
        } else {
            serde_json::json!(["sleep", "10"])
        };
        let result = tool
            .execute(&serde_json::json!({"argv": argv, "timeout_secs": 1}), &ctx)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timed out"), "got: {err}");

        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn exec_readonly_blocked() {
        let tmp = std::env::temp_dir().join("tool_exec_ro_test");
        fs::create_dir_all(&tmp).unwrap();

        let tool = ExecTool::new();
        let ctx = ToolContext {
            workspace_root: tmp.clone(),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::ReadOnly,
        };

        let result = tool
            .execute(&serde_json::json!({"command": "echo hi"}), &ctx)
            .await;
        assert!(result.is_err());

        fs::remove_dir_all(&tmp).unwrap();
    }
}
