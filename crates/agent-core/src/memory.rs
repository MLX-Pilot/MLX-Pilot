//! Local memory store for compact context artifacts and durable agent memory.

use crate::state_store::StateStore;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRecord {
    pub id: String,
    pub session_id: String,
    #[serde(default)]
    pub source_session_id: String,
    #[serde(default = "default_memory_scope")]
    pub scope: String,
    #[serde(default = "default_memory_namespace")]
    pub namespace: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub importance: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<DateTime<Utc>>,
    #[serde(default = "default_pin_state")]
    pub pin_state: String,
    #[serde(default)]
    pub promotion_source: String,
    #[serde(default)]
    pub summary_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemorySearchHit {
    pub id: String,
    pub session_id: String,
    pub kind: String,
    pub title: String,
    pub preview: String,
    pub score: i64,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub namespace: String,
}

pub struct MemoryStore {
    root: PathBuf,
    state: StateStore,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryPromotionDecision {
    pub record: MemoryRecord,
    pub reason: String,
}

impl MemoryStore {
    pub async fn new(root: PathBuf) -> std::io::Result<Self> {
        let db_path = root
            .parent()
            .unwrap_or(root.as_path())
            .join("agent")
            .join("state.sqlite");
        let state = StateStore::new(db_path).await?;
        let store = Self { root, state };
        store.import_legacy_if_needed_blocking()?;
        Ok(store)
    }

    pub async fn upsert(&self, records: &[MemoryRecord]) -> std::io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        self.state.upsert_memory_records(records).await
    }

    pub async fn get(&self, id: &str) -> std::io::Result<Option<MemoryRecord>> {
        self.state.get_memory_record(id).await
    }

    pub async fn search(&self, query: &str, limit: usize) -> std::io::Result<Vec<MemorySearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let fts_hits = self.state.fts_memory_search(query, limit).await?;
        if !fts_hits.is_empty() {
            let mut hits = fts_hits
                .into_iter()
                .map(|(record, preview_text, raw_score)| MemorySearchHit {
                    id: record.id,
                    session_id: record.session_id,
                    kind: record.kind,
                    title: record.title,
                    preview: if preview_text.trim().is_empty() {
                        preview(&record.content, 180)
                    } else {
                        preview_text
                    },
                    score: raw_score + i64::from(record.importance.max(0)),
                    created_at: record.created_at,
                    scope: record.scope,
                    namespace: record.namespace,
                })
                .collect::<Vec<_>>();
            hits.sort_by(|left, right| {
                right
                    .score
                    .cmp(&left.score)
                    .then_with(|| right.created_at.cmp(&left.created_at))
            });
            hits.truncate(limit.max(1));
            return Ok(hits);
        }

        let records = self.state.load_all_memory_records().await?;
        let query_tokens = tokenize(query);
        let normalized_query = query.to_ascii_lowercase();

        let mut hits = records
            .into_iter()
            .filter_map(|record| {
                let haystack = format!(
                    "{} {} {} {} {}",
                    record.title.to_ascii_lowercase(),
                    record.kind.to_ascii_lowercase(),
                    record.scope.to_ascii_lowercase(),
                    record.namespace.to_ascii_lowercase(),
                    record.content.to_ascii_lowercase()
                );
                let score = score_match(&haystack, &normalized_query, &query_tokens)
                    + i64::from(record.importance.max(0));
                if score <= 0 {
                    return None;
                }
                Some(MemorySearchHit {
                    id: record.id,
                    session_id: record.session_id,
                    kind: record.kind,
                    title: record.title,
                    preview: preview(&record.content, 180),
                    score,
                    created_at: record.created_at,
                    scope: record.scope,
                    namespace: record.namespace,
                })
            })
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.created_at.cmp(&left.created_at))
        });
        hits.truncate(limit.max(1));
        Ok(hits)
    }

    fn import_legacy_if_needed_blocking(&self) -> std::io::Result<()> {
        if !self.root.exists() {
            std::fs::create_dir_all(&self.root)?;
        }
        let index_path = self.root.join("index.json");
        if !index_path.exists() {
            return Ok(());
        }

        let raw = std::fs::read_to_string(index_path)?;
        if raw.trim().is_empty() {
            return Ok(());
        }
        let legacy = serde_json::from_str::<BTreeMap<String, MemoryRecord>>(&raw)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let records = legacy.into_values().collect::<Vec<_>>();
        if records.is_empty() {
            return Ok(());
        }
        let rt = tokio::runtime::Handle::try_current().ok();
        if let Some(handle) = rt {
            let _ = handle.block_on(self.state.upsert_memory_records(&records));
        } else {
            let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime for import");
            let _ = runtime.block_on(self.state.upsert_memory_records(&records));
        }
        Ok(())
    }
}

fn default_memory_scope() -> String {
    "session".to_string()
}

fn default_memory_namespace() -> String {
    "default".to_string()
}

fn default_pin_state() -> String {
    "auto".to_string()
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

    #[tokio::test]
    async fn upsert_and_search_records() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf()).await.unwrap();
        store
            .upsert(&[MemoryRecord {
                id: "mem-1".to_string(),
                session_id: "s-1".to_string(),
                source_session_id: "s-1".to_string(),
                scope: "long_term".to_string(),
                namespace: "history".to_string(),
                kind: "history_summary".to_string(),
                title: "Decision log".to_string(),
                content: "User asked for budget policy and the agent summarized older turns."
                    .to_string(),
                tags: vec!["budget".to_string()],
                created_at: Utc::now(),
                metadata: BTreeMap::new(),
                importance: 25,
                last_accessed_at: None,
                pin_state: "auto".to_string(),
                promotion_source: "test".to_string(),
                summary_ref: String::new(),
            }])
            .await
            .unwrap();

        let hits = store.search("budget policy", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "mem-1");
        assert_eq!(hits[0].scope, "long_term");
        assert!(store.get("mem-1").await.unwrap().is_some());
    }
}
