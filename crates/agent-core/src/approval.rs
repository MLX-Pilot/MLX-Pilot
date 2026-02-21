//! `ApprovalService` — user approval flow for dangerous tool calls.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A request sent to the user for approval.
#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub skill_name: Option<String>,
    pub tool_name: String,
    pub description: String,
    pub params_summary: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// The user's decision on an approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowOnce,
    AllowSession,
    AllowAlways { pattern: String },
    Deny,
}

/// Errors from the approval flow.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("approval timed out after {0:?}")]
    Timeout(Duration),

    #[error("approval request not found: {id}")]
    NotFound { id: String },

    #[error("approval service unavailable")]
    Unavailable,
}

/// Trait for requesting and managing user approvals.
///
/// Concrete implementation will bridge to the Tauri UI via WebSocket
/// in Phase 3/4.
#[async_trait::async_trait]
pub trait ApprovalService: Send + Sync {
    /// Request approval from the user. Blocks until decision or timeout.
    async fn request_approval(
        &self,
        request: ApprovalRequest,
        timeout: Duration,
    ) -> Result<ApprovalDecision, ApprovalError>;

    /// Check if a pattern is in the persistent allowlist.
    fn is_allowed(&self, tool_name: &str, params_pattern: &str) -> bool;

    /// Add a pattern to the persistent allowlist.
    fn add_allowlist_entry(&self, tool_name: &str, pattern: String);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_request_serializes() {
        let req = ApprovalRequest {
            id: "test-123".into(),
            skill_name: Some("weather".into()),
            tool_name: "exec".into(),
            description: "Run curl".into(),
            params_summary: "curl wttr.in".into(),
            created_at: Utc::now(),
            expires_at: Utc::now(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("test-123"));
    }

    #[test]
    fn approval_decision_variants() {
        let once = ApprovalDecision::AllowOnce;
        let json = serde_json::to_string(&once).unwrap();
        assert!(json.contains("allow_once"));
    }
}
