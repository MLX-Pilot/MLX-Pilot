//! Session recall/search support for Hermes-inspired runtime flows.

use crate::session::SessionStore;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRecallHit {
    pub session_id: String,
    pub name: String,
    pub preview: String,
    pub score: i64,
    pub updated_at: DateTime<Utc>,
    pub provider_id: String,
    pub model_id: String,
    pub origin_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
}

#[derive(Clone)]
pub struct SessionRecall {
    sessions: Arc<SessionStore>,
}

impl SessionRecall {
    pub fn new(sessions: Arc<SessionStore>) -> Self {
        Self { sessions }
    }

    pub async fn search(
        &self,
        query: &str,
        current_session_id: Option<&str>,
        limit: usize,
    ) -> io::Result<Vec<SessionRecallHit>> {
        self.sessions.search(query, current_session_id, limit).await
    }
}
