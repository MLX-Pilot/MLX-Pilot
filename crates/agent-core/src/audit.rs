//! `AuditLog` — structured JSONL audit logging.

use crate::tool_catalog::ToolRuleTrace;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub event_type: AuditEventType,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tool_name: Option<String>,
    pub skill_name: Option<String>,
    pub params_hash: Option<String>,
    pub params_summary: Option<String>,
    pub result_summary: Option<String>,
    pub duration_ms: Option<u64>,
    pub decision: Option<String>,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub policy_trace: Vec<ToolRuleTrace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_risk: Option<String>,
    pub error: Option<String>,
    pub error_summary: Option<String>,
}

/// The type of auditable event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    PromptBuilt,
    ProviderCall,
    ToolRequested,
    ToolAllowed,
    ToolDenied,
    ToolExecuted,
    ToolFailed,
    ApprovalPending,
    ApprovalDecision,
    ResponseFinal,
    Error,
    // Keep backwards compatibility for manually tested stuff
    SkillLoaded,
    SkillRejected,
    SessionStarted,
    SessionEnded,
}

/// JSONL-based audit log writer.
///
/// Concrete I/O implementation will be added in Phase 3.
pub struct AuditLog {
    pub log_dir: PathBuf,
}

impl AuditLog {
    /// Create a new audit log targeting the given directory.
    pub fn new(log_dir: PathBuf) -> Self {
        Self { log_dir }
    }

    /// Write an entry to today's log file.
    pub async fn write(&self, entry: &AuditLogEntry) -> Result<(), std::io::Error> {
        let date = entry.timestamp.format("%Y-%m-%d").to_string();
        let filename = format!("{}.jsonl", date);
        let path = self.log_dir.join(filename);

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        let mut json = serde_json::to_string(entry).unwrap_or_default();
        if json.is_empty() {
            return Ok(());
        }
        json.push('\n');

        file.write_all(json.as_bytes()).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_entry_serializes() {
        let entry = AuditLogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            session_id: "sess-1".into(),
            event_type: AuditEventType::ToolExecuted,
            provider: None,
            model: None,
            tool_name: Some("read_file".into()),
            skill_name: None,
            params_hash: Some("abc".into()),
            params_summary: Some("path=/tmp/test".into()),
            result_summary: Some("ok".into()),
            duration_ms: Some(12),
            decision: None,
            reason: None,
            policy_rule: None,
            policy_trace: Vec::new(),
            tool_risk: None,
            error: None,
            error_summary: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("tool_executed"));
    }

    #[tokio::test]
    async fn audit_log_write_stub_ok() {
        let log = AuditLog::new(PathBuf::from("/tmp/audit"));
        let entry = AuditLogEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            session_id: "sess-1".into(),
            event_type: AuditEventType::SessionStarted,
            provider: None,
            model: None,
            tool_name: None,
            skill_name: None,
            params_hash: None,
            params_summary: None,
            result_summary: None,
            duration_ms: None,
            decision: None,
            reason: None,
            policy_rule: None,
            policy_trace: Vec::new(),
            tool_risk: None,
            error: None,
            error_summary: None,
        };
        // Should succeed (stub does nothing).
        log.write(&entry).await.unwrap();
    }
}
