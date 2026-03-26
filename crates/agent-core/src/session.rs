//! `SessionStore` — JSONL-based session persistence.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A persisted message in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// JSONL-based session persistence.
///
/// Each session is stored as a `.jsonl` file under `sessions_dir/{id}.jsonl`.
pub struct SessionStore {
    pub sessions_dir: PathBuf,
}

impl SessionStore {
    /// Create a new session store targeting the given directory.
    pub fn new(sessions_dir: PathBuf) -> Self {
        Self { sessions_dir }
    }

    /// Generate a new session ID.
    pub fn new_session_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    /// Append a message to a session.
    ///
    /// Stub — will be implemented in Phase 1 (task 1.8).
    pub async fn append(
        &self,
        _session_id: &str,
        _message: &SessionMessage,
    ) -> Result<(), std::io::Error> {
        // TODO: serialize to JSONL and append.
        Ok(())
    }

    /// Load all messages for a session.
    ///
    /// Stub — will be implemented in Phase 1 (task 1.8).
    pub async fn load(&self, _session_id: &str) -> Result<Vec<SessionMessage>, std::io::Error> {
        // TODO: read JSONL and deserialize.
        Ok(Vec::new())
    }

    /// List all session IDs.
    ///
    /// Stub — will be implemented in Phase 1.
    pub async fn list_sessions(&self) -> Result<Vec<String>, std::io::Error> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_id_is_uuid() {
        let id = SessionStore::new_session_id();
        assert_eq!(id.len(), 36); // UUID v4 string length
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[tokio::test]
    async fn session_store_stub_operations() {
        let store = SessionStore::new(PathBuf::from("/tmp/sessions"));
        let messages = store.load("nonexistent").await.unwrap();
        assert!(messages.is_empty());

        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }
}
