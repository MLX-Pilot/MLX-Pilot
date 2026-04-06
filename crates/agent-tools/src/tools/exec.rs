//! `ExecTool` — sandboxed command execution with timeout and denylist.

use crate::types::{ExecutionMode, ParamSchema, ToolContext, ToolError, ToolResult};
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

/// Maximum output size in bytes.
const MAX_OUTPUT_BYTES: usize = 256 * 1024; // 256 KB
const STREAM_CHUNK_BYTES: usize = 8 * 1024;

/// Executes a shell command within the workspace.
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
                        "description": "Shell command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 30)"
                    }
                },
                "required": ["command"]
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
}

impl Default for ExecTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the workspace directory. Commands have a timeout and are checked against a denylist."
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
                let cmd = params["command"].as_str().unwrap_or("<missing>");
                return Ok(ToolResult {
                    output: format!("[DRY RUN] would exec: {cmd}"),
                    is_error: false,
                    metadata: HashMap::new(),
                });
            }
            ExecutionMode::Full => {}
        }

        let command = params["command"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams {
                details: "missing 'command' string".into(),
            })?;

        let timeout_secs = params["timeout_secs"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        // Check denylist.
        if let Some(pattern) = Self::is_denied(command) {
            return Err(ToolError::PermissionDenied {
                reason: format!("command matches deny pattern: '{pattern}'"),
            });
        }

        // Build the subprocess.
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let mut child = tokio::process::Command::new(shell)
            .arg(flag)
            .arg(command)
            .current_dir(&ctx.workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true) // Ensure cleanup on drop
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("failed to spawn command: {e}"),
            })?;

        // Take the output handles so we can read them after wait().
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Wait with timeout. kill_on_drop ensures cleanup if we error.
        let status =
            match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait()).await {
                Ok(Ok(status)) => status,
                Ok(Err(e)) => {
                    return Err(ToolError::ExecutionFailed {
                        message: format!("command failed: {e}"),
                    });
                }
                Err(_) => {
                    // Timeout — kill_on_drop handles cleanup when `child` is dropped.
                    drop(child);
                    return Err(ToolError::Timeout {
                        seconds: timeout_secs,
                    });
                }
            };

        // Read captured output incrementally, avoiding unbounded buffering.
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

        let exit_code = status.code().unwrap_or(-1);
        let is_error = !status.success();

        let combined = if stderr.is_empty() {
            stdout
        } else {
            format!("{stdout}\n--- stderr ---\n{stderr}")
        };

        Ok(ToolResult {
            output: combined,
            is_error,
            metadata: {
                let mut m = HashMap::new();
                m.insert("exit_code".into(), Value::Number(exit_code.into()));
                m
            },
        })
    }
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
        assert!(ExecTool::is_denied("ls -la").is_none());
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

        let cmd = "echo hello";
        let result = tool
            .execute(&serde_json::json!({"command": cmd}), &ctx)
            .await
            .unwrap();
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

        // Use a command that sleeps longer than the timeout.
        let cmd = if cfg!(windows) {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };
        let result = tool
            .execute(
                &serde_json::json!({"command": cmd, "timeout_secs": 1}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timed out"), "got: {err}");

        // Wait briefly for the killed process to release file handles (Windows).
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
