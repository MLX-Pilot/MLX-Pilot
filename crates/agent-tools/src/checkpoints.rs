use crate::sandbox::assert_sandbox_path;
use crate::types::ToolError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

const CHECKPOINT_ROOT_DIR: &str = ".mlx-pilot/checkpoints";
const CHECKPOINT_INDEX_FILE: &str = "index.jsonl";
const CHECKPOINT_PAYLOAD_DIR: &str = "payloads";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCheckpointRecord {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub relative_path: String,
    pub existed_before: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_payload_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCheckpointSummary {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub relative_path: String,
    pub existed_before: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCheckpointRestoreResult {
    pub checkpoint_id: String,
    pub relative_path: String,
    pub restored: bool,
    pub deleted_generated_file: bool,
}

impl From<FileCheckpointRecord> for FileCheckpointSummary {
    fn from(value: FileCheckpointRecord) -> Self {
        Self {
            id: value.id,
            session_id: value.session_id,
            tool_name: value.tool_name,
            relative_path: value.relative_path,
            existed_before: value.existed_before,
            created_at: value.created_at,
            git_head: value.git_head,
            git_branch: value.git_branch,
            after_sha256: value.after_sha256,
        }
    }
}

pub async fn record_file_checkpoint(
    workspace_root: &Path,
    session_id: &str,
    tool_name: &str,
    relative_path: &str,
    before_bytes: Option<&[u8]>,
    after_bytes: &[u8],
) -> Result<FileCheckpointRecord, ToolError> {
    let checkpoint_root = checkpoint_root(workspace_root);
    tokio::fs::create_dir_all(checkpoint_root.join(CHECKPOINT_PAYLOAD_DIR))
        .await
        .map_err(io_tool_error)?;

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = chrono::Utc::now();
    let before_payload_path = if let Some(bytes) = before_bytes {
        let payload_path = checkpoint_root
            .join(CHECKPOINT_PAYLOAD_DIR)
            .join(format!("{id}.bin"));
        tokio::fs::write(&payload_path, bytes)
            .await
            .map_err(io_tool_error)?;
        Some(path_relative_to_root(&checkpoint_root, &payload_path))
    } else {
        None
    };

    let git_state = capture_git_state(workspace_root).await;
    let record = FileCheckpointRecord {
        id,
        session_id: session_id.to_string(),
        tool_name: tool_name.to_string(),
        relative_path: normalize_relative_path(relative_path),
        existed_before: before_bytes.is_some(),
        created_at,
        before_payload_path,
        git_head: git_state.as_ref().and_then(|value| value.head.clone()),
        git_branch: git_state.and_then(|value| value.branch),
        after_sha256: Some(sha256_hex(after_bytes)),
    };

    append_checkpoint_record(&checkpoint_root, &record).await?;
    Ok(record)
}

pub async fn list_file_checkpoints(
    workspace_root: &Path,
    session_id: Option<&str>,
    limit: usize,
) -> Result<Vec<FileCheckpointSummary>, ToolError> {
    let records = read_checkpoint_records(workspace_root).await?;
    let mut filtered = records
        .into_iter()
        .filter(|record| {
            session_id
                .map(|value| value.trim() == record.session_id)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    filtered.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    filtered.truncate(limit.clamp(1, 200));
    Ok(filtered
        .into_iter()
        .map(FileCheckpointSummary::from)
        .collect())
}

pub async fn restore_file_checkpoint(
    workspace_root: &Path,
    checkpoint_id: &str,
) -> Result<FileCheckpointRestoreResult, ToolError> {
    let checkpoint_root = checkpoint_root(workspace_root);
    let records = read_checkpoint_records(workspace_root).await?;
    let record = records
        .into_iter()
        .find(|value| value.id == checkpoint_id)
        .ok_or_else(|| ToolError::ExecutionFailed {
            message: format!("checkpoint '{checkpoint_id}' not found"),
        })?;

    let target_path = assert_sandbox_path(workspace_root, &record.relative_path)?;
    if !record.existed_before {
        if target_path.exists() {
            tokio::fs::remove_file(&target_path)
                .await
                .map_err(io_tool_error)?;
        }
        return Ok(FileCheckpointRestoreResult {
            checkpoint_id: record.id,
            relative_path: record.relative_path,
            restored: true,
            deleted_generated_file: true,
        });
    }

    let payload_relative =
        record
            .before_payload_path
            .as_deref()
            .ok_or_else(|| ToolError::ExecutionFailed {
                message: format!("checkpoint '{}' is missing payload metadata", record.id),
            })?;
    let payload_path = checkpoint_root.join(payload_relative);
    let previous_bytes = tokio::fs::read(&payload_path)
        .await
        .map_err(io_tool_error)?;
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(io_tool_error)?;
    }
    tokio::fs::write(&target_path, previous_bytes)
        .await
        .map_err(io_tool_error)?;

    Ok(FileCheckpointRestoreResult {
        checkpoint_id: record.id,
        relative_path: record.relative_path,
        restored: true,
        deleted_generated_file: false,
    })
}

async fn append_checkpoint_record(
    checkpoint_root: &Path,
    record: &FileCheckpointRecord,
) -> Result<(), ToolError> {
    let path = checkpoint_root.join(CHECKPOINT_INDEX_FILE);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(io_tool_error)?;
    let mut writer = BufWriter::new(file);
    let mut line = serde_json::to_string(record).map_err(|error| ToolError::ExecutionFailed {
        message: format!("failed to serialize checkpoint: {error}"),
    })?;
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(io_tool_error)?;
    writer.flush().await.map_err(io_tool_error)?;
    Ok(())
}

async fn read_checkpoint_records(
    workspace_root: &Path,
) -> Result<Vec<FileCheckpointRecord>, ToolError> {
    let checkpoint_root = checkpoint_root(workspace_root);
    let path = checkpoint_root.join(CHECKPOINT_INDEX_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path).await.map_err(io_tool_error)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut records = Vec::new();
    while let Some(line) = lines.next_line().await.map_err(io_tool_error)? {
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str::<FileCheckpointRecord>(&line).map_err(|error| {
            ToolError::ExecutionFailed {
                message: format!("failed to parse checkpoint index: {error}"),
            }
        })?;
        records.push(record);
    }
    Ok(records)
}

fn checkpoint_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join(CHECKPOINT_ROOT_DIR)
}

fn path_relative_to_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn normalize_relative_path(value: &str) -> String {
    value.trim().replace('\\', "/")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone)]
struct GitState {
    head: Option<String>,
    branch: Option<String>,
}

async fn capture_git_state(workspace_root: &Path) -> Option<GitState> {
    let head = capture_git_output(workspace_root, &["rev-parse", "HEAD"]).await;
    let branch = capture_git_output(workspace_root, &["rev-parse", "--abbrev-ref", "HEAD"]).await;
    if head.is_none() && branch.is_none() {
        return None;
    }
    Some(GitState { head, branch })
}

async fn capture_git_output(workspace_root: &Path, args: &[&str]) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn io_tool_error(error: std::io::Error) -> ToolError {
    ToolError::ExecutionFailed {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn checkpoint_roundtrip_restores_previous_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let target = workspace.join("sample.txt");
        tokio::fs::write(&target, b"before").await.unwrap();

        let _record = record_file_checkpoint(
            workspace,
            "sess-1",
            "write_file",
            "sample.txt",
            Some(b"before".as_slice()),
            b"after",
        )
        .await
        .unwrap();
        tokio::fs::write(&target, b"after").await.unwrap();

        let checkpoints = list_file_checkpoints(workspace, Some("sess-1"), 10)
            .await
            .unwrap();
        assert_eq!(checkpoints.len(), 1);

        let restored = restore_file_checkpoint(workspace, &checkpoints[0].id)
            .await
            .unwrap();
        assert!(restored.restored);
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"before");
    }

    #[tokio::test]
    async fn checkpoint_restore_deletes_created_file() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let target = workspace.join("created.txt");

        let record = record_file_checkpoint(
            workspace,
            "sess-2",
            "write_file",
            "created.txt",
            None,
            b"new file",
        )
        .await
        .unwrap();
        tokio::fs::write(&target, b"new file").await.unwrap();

        let restored = restore_file_checkpoint(workspace, &record.id)
            .await
            .unwrap();
        assert!(restored.deleted_generated_file);
        assert!(!target.exists());
    }
}
