//! `SessionStore` — SQLite-backed session persistence with legacy import.

use crate::session_recall::SessionRecallHit;
use crate::state_store::StateStore;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

/// A persisted event/message in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(default = "default_message_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_json: Option<Value>,
}

impl SessionMessage {
    pub fn user(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content,
            tool_call_id: None,
            tool_name: None,
            timestamp: Utc::now(),
            kind: "user".to_string(),
            content_json: None,
            metadata_json: None,
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_call_id: None,
            tool_name: None,
            timestamp: Utc::now(),
            kind: "assistant".to_string(),
            content_json: None,
            metadata_json: None,
        }
    }

    pub fn system_snapshot(content: String) -> Self {
        Self {
            role: "system".to_string(),
            content,
            tool_call_id: None,
            tool_name: None,
            timestamp: Utc::now(),
            kind: "system_snapshot".to_string(),
            content_json: None,
            metadata_json: None,
        }
    }

    pub fn tool_call(tool_name: String, tool_call_id: String, params: Value) -> Self {
        Self {
            role: "assistant".to_string(),
            content: String::new(),
            tool_call_id: Some(tool_call_id),
            tool_name: Some(tool_name),
            timestamp: Utc::now(),
            kind: "tool_call".to_string(),
            content_json: Some(params),
            metadata_json: None,
        }
    }

    pub fn tool_result_event(tool_name: String, tool_call_id: String, output: String) -> Self {
        Self {
            role: "tool".to_string(),
            content: output,
            tool_call_id: Some(tool_call_id),
            tool_name: Some(tool_name),
            timestamp: Utc::now(),
            kind: "tool_result".to_string(),
            content_json: None,
            metadata_json: None,
        }
    }
}

/// Metadata for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub name: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(default = "chrono::Utc::now")]
    pub last_activity_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub workspace_root: String,
    #[serde(default = "default_origin_kind")]
    pub origin_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default = "default_session_status")]
    pub status: String,
    #[serde(default = "chrono::Utc::now")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub source_channel: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub correlation_id: String,
}

impl SessionMeta {
    pub fn basic(id: String, name: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            name,
            updated_at: now,
            last_activity_at: now,
            message_count: 0,
            provider_id: String::new(),
            model_id: String::new(),
            workspace_root: String::new(),
            origin_kind: default_origin_kind(),
            parent_session_id: None,
            status: default_session_status(),
            created_at: now,
            summary: String::new(),
            source_channel: String::new(),
            thread_id: String::new(),
            correlation_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_json: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// SQLite-based session persistence.
pub struct SessionStore {
    pub sessions_dir: PathBuf,
    state: StateStore,
}

impl SessionStore {
    /// Create a new session store targeting the given legacy session directory.
    pub async fn new(sessions_dir: PathBuf) -> std::io::Result<Self> {
        if !sessions_dir.exists() {
            tokio::fs::create_dir_all(&sessions_dir).await?;
        }

        let state = StateStore::new(state_db_path(&sessions_dir)).await?;
        let store = Self {
            sessions_dir,
            state,
        };
        store.import_legacy_if_needed().await?;
        Ok(store)
    }

    /// Generate a new session ID.
    pub fn new_session_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    pub async fn ensure_session(
        &self,
        session_id: &str,
        initial_name: Option<String>,
    ) -> std::io::Result<()> {
        let existing = self.state.get_session_meta(session_id).await?;
        let meta = existing.unwrap_or_else(|| {
            SessionMeta::basic(
                session_id.to_string(),
                initial_name.unwrap_or_else(|| "Nova conversa".to_string()),
            )
        });
        self.state.upsert_session_meta(&meta).await
    }

    pub async fn ensure_session_with_meta(&self, meta: SessionMeta) -> std::io::Result<()> {
        let existing = self.state.get_session_meta(&meta.id).await?;
        let merged = if let Some(existing) = existing {
            SessionMeta {
                id: existing.id,
                name: if meta.name.trim().is_empty() {
                    existing.name
                } else {
                    meta.name
                },
                updated_at: meta.updated_at,
                message_count: existing.message_count,
                provider_id: if meta.provider_id.trim().is_empty() {
                    existing.provider_id
                } else {
                    meta.provider_id
                },
                model_id: if meta.model_id.trim().is_empty() {
                    existing.model_id
                } else {
                    meta.model_id
                },
                workspace_root: if meta.workspace_root.trim().is_empty() {
                    existing.workspace_root
                } else {
                    meta.workspace_root
                },
                origin_kind: if meta.origin_kind.trim().is_empty() {
                    existing.origin_kind
                } else {
                    meta.origin_kind
                },
                parent_session_id: meta.parent_session_id.or(existing.parent_session_id),
                status: if meta.status.trim().is_empty() {
                    existing.status
                } else {
                    meta.status
                },
                created_at: existing.created_at,
                last_activity_at: meta.last_activity_at,
                summary: if meta.summary.trim().is_empty() {
                    existing.summary
                } else {
                    meta.summary
                },
                source_channel: if meta.source_channel.trim().is_empty() {
                    existing.source_channel
                } else {
                    meta.source_channel
                },
                thread_id: if meta.thread_id.trim().is_empty() {
                    existing.thread_id
                } else {
                    meta.thread_id
                },
                correlation_id: if meta.correlation_id.trim().is_empty() {
                    existing.correlation_id
                } else {
                    meta.correlation_id
                },
            }
        } else {
            meta
        };
        self.state.upsert_session_meta(&merged).await
    }

    pub async fn append(
        &self,
        session_id: &str,
        message: &SessionMessage,
    ) -> Result<(), std::io::Error> {
        self.ensure_session(session_id, None).await?;
        self.state.append_session_event(session_id, message).await?;

        if message.kind == "user" && message.content.trim().len() > 0 {
            if let Some(meta) = self.state.get_session_meta(session_id).await? {
                if meta.message_count <= 1 && meta.name == "Nova conversa" {
                    let snippet: String = message.content.chars().take(30).collect();
                    let renamed = SessionMeta {
                        name: if message.content.chars().count() > 30 {
                            format!("{snippet}...")
                        } else {
                            snippet
                        },
                        ..meta
                    };
                    let _ = self.state.upsert_session_meta(&renamed).await;
                }
            }
        }

        Ok(())
    }

    pub async fn load(&self, session_id: &str) -> Result<Vec<SessionMessage>, std::io::Error> {
        self.state.load_session_events(session_id).await
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionMeta>, std::io::Error> {
        self.state.list_sessions().await
    }

    pub async fn save_summary(
        &self,
        session_id: &str,
        summary: &str,
        summary_json: Option<Value>,
    ) -> std::io::Result<()> {
        self.state
            .upsert_session_summary(session_id, summary, summary_json)
            .await
    }

    pub async fn load_summary(&self, session_id: &str) -> std::io::Result<Option<String>> {
        self.state.load_session_summary(session_id).await
    }

    pub async fn save_snapshot(
        &self,
        session_id: &str,
        text: &str,
        snapshot_json: Option<Value>,
    ) -> std::io::Result<()> {
        self.state
            .append_session_snapshot(session_id, text, snapshot_json)
            .await
    }

    pub async fn latest_snapshot(
        &self,
        session_id: &str,
    ) -> std::io::Result<Option<SessionSnapshot>> {
        self.state.latest_session_snapshot(session_id).await
    }

    pub async fn rename(&self, session_id: &str, new_name: &str) -> Result<(), std::io::Error> {
        self.state.rename_session(session_id, new_name).await
    }

    pub async fn delete(&self, session_id: &str) -> std::io::Result<()> {
        self.state.delete_session(session_id).await
    }

    pub async fn export(&self, session_id: &str) -> std::io::Result<String> {
        let messages = self.load(session_id).await?;
        serde_json::to_string_pretty(&messages)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub async fn search(
        &self,
        query: &str,
        current_session_id: Option<&str>,
        limit: usize,
    ) -> io::Result<Vec<SessionRecallHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let current_meta = if let Some(id) = current_session_id {
            self.state.get_session_meta(id).await?
        } else {
            None
        };
        let mut candidates = self
            .state
            .fts_session_search_candidates(query, limit.saturating_mul(3).max(limit))
            .await?;
        if candidates.is_empty() {
            candidates = self.state.session_search_candidates().await?;
        }
        let query_tokens = tokenize(query);
        let normalized_query = query.to_ascii_lowercase();

        let mut deduped = BTreeMap::new();
        for candidate in candidates {
            deduped
                .entry(candidate.meta.id.clone())
                .and_modify(
                    |existing: &mut crate::state_store::SessionSearchCandidate| {
                        if candidate.raw_score > existing.raw_score {
                            *existing = candidate.clone();
                        }
                    },
                )
                .or_insert(candidate);
        }

        let mut hits = deduped
            .into_values()
            .filter(|candidate| {
                if let Some(current_id) = current_session_id {
                    if candidate.meta.id == current_id {
                        return false;
                    }
                    if candidate.meta.parent_session_id.as_deref() == Some(current_id) {
                        return false;
                    }
                    if current_meta
                        .as_ref()
                        .and_then(|meta| meta.parent_session_id.as_deref())
                        == Some(candidate.meta.id.as_str())
                    {
                        return false;
                    }
                }
                true
            })
            .filter_map(|candidate| {
                let haystack = format!(
                    "{} {} {} {}",
                    candidate.meta.name.to_ascii_lowercase(),
                    candidate.meta.model_id.to_ascii_lowercase(),
                    candidate.meta.summary.to_ascii_lowercase(),
                    candidate.transcript.to_ascii_lowercase()
                );
                let recency_bonus =
                    ((Utc::now() - candidate.meta.updated_at).num_days().max(0)).saturating_sub(30);
                let score = score_match(&haystack, &normalized_query, &query_tokens)
                    + candidate.raw_score.max(0)
                    + 30_i64.saturating_sub(recency_bonus);
                if score <= 0 {
                    return None;
                }
                Some(SessionRecallHit {
                    session_id: candidate.meta.id.clone(),
                    name: candidate.meta.name.clone(),
                    preview: if candidate.preview.trim().is_empty() {
                        preview(&candidate.transcript, 220)
                    } else {
                        candidate.preview
                    },
                    score: score + candidate.raw_score.max(0),
                    updated_at: candidate.meta.updated_at,
                    provider_id: candidate.meta.provider_id,
                    model_id: candidate.meta.model_id,
                    origin_kind: candidate.meta.origin_kind,
                    parent_session_id: candidate.meta.parent_session_id,
                })
            })
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
        });
        hits.truncate(limit.max(1));
        Ok(hits)
    }

    async fn import_legacy_if_needed(&self) -> std::io::Result<()> {
        if !self.state.list_sessions().await?.is_empty() {
            return Ok(());
        }

        let index_path = self.sessions_dir.join("index.json");
        let legacy_index = if index_path.exists() {
            tokio::fs::read_to_string(&index_path)
                .await
                .ok()
                .and_then(|raw| {
                    serde_json::from_str::<std::collections::BTreeMap<String, SessionMeta>>(&raw)
                        .ok()
                })
                .unwrap_or_default()
        } else {
            std::collections::BTreeMap::new()
        };

        let mut entries = tokio::fs::read_dir(&self.sessions_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(session_id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let raw = tokio::fs::read_to_string(&path).await?;
            let mut messages = Vec::new();
            for line in raw.lines().filter(|line| !line.trim().is_empty()) {
                let mut message: SessionMessage = serde_json::from_str(line)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
                if message.kind.trim().is_empty() {
                    message.kind = default_kind_for_role(&message.role);
                }
                messages.push(message);
            }

            let meta = legacy_index.get(session_id).cloned().unwrap_or_else(|| {
                let mut meta =
                    SessionMeta::basic(session_id.to_string(), "Nova conversa".to_string());
                meta.message_count = messages.len();
                if let Some(last) = messages.last() {
                    meta.updated_at = last.timestamp;
                }
                meta
            });
            self.ensure_session_with_meta(meta).await?;
            for message in messages {
                self.state
                    .append_session_event(session_id, &message)
                    .await?;
            }
        }

        Ok(())
    }
}

fn state_db_path(sessions_dir: &std::path::Path) -> PathBuf {
    sessions_dir
        .parent()
        .unwrap_or(sessions_dir)
        .join("agent")
        .join("state.sqlite")
}

fn default_message_kind() -> String {
    "message".to_string()
}

fn default_origin_kind() -> String {
    "local".to_string()
}

fn default_session_status() -> String {
    "active".to_string()
}

fn default_kind_for_role(role: &str) -> String {
    match role.trim().to_ascii_lowercase().as_str() {
        "assistant" => "assistant".to_string(),
        "tool" => "tool_result".to_string(),
        "system" => "system_snapshot".to_string(),
        _ => "user".to_string(),
    }
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
        .collect()
}

fn score_match(haystack: &str, query: &str, query_tokens: &[String]) -> i64 {
    let mut score = 0_i64;
    if haystack.contains(query) {
        score += 50;
    }
    for token in query_tokens {
        if haystack.contains(token) {
            score += 10;
        }
    }
    score
}

fn preview(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let mut out = compact
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_id_is_uuid() {
        let id = SessionStore::new_session_id();
        assert_eq!(id.len(), 36);
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[tokio::test]
    async fn session_store_operations() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(temp_dir.path().to_path_buf())
            .await
            .unwrap();

        let messages = store.load("nonexistent").await.unwrap();
        assert!(messages.is_empty());
        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());

        let session_id = SessionStore::new_session_id();
        let msg1 = SessionMessage::user("Hello agent".to_string());
        store.append(&session_id, &msg1).await.unwrap();

        let loaded = store.load(&session_id).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "Hello agent");

        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].message_count, 1);
        assert_eq!(sessions[0].name, "Hello agent");

        store.rename(&session_id, "Greeting test").await.unwrap();
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions[0].name, "Greeting test");

        let results = store.search("hello", Some("other"), 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, session_id);

        store.delete(&session_id).await.unwrap();
        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }
}
