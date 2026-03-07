//! Local memory store for compact context artifacts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRecord {
    pub id: String,
    pub session_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
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
}

pub struct MemoryStore {
    root: PathBuf,
    lock: tokio::sync::Mutex<()>,
}

impl MemoryStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn upsert(&self, records: &[MemoryRecord]) -> std::io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let _guard = self.lock.lock().await;
        let mut index = self.load_all_locked().await?;
        for record in records {
            index.insert(record.id.clone(), record.clone());
        }
        self.save_all_locked(&index).await
    }

    pub async fn get(&self, id: &str) -> std::io::Result<Option<MemoryRecord>> {
        let _guard = self.lock.lock().await;
        let index = self.load_all_locked().await?;
        Ok(index.get(id).cloned())
    }

    pub async fn search(&self, query: &str, limit: usize) -> std::io::Result<Vec<MemorySearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let _guard = self.lock.lock().await;
        let index = self.load_all_locked().await?;
        let query_tokens = tokenize(query);
        let normalized_query = query.to_ascii_lowercase();

        let mut hits = index
            .values()
            .filter_map(|record| {
                let haystack = format!(
                    "{} {} {}",
                    record.title.to_ascii_lowercase(),
                    record.kind.to_ascii_lowercase(),
                    record.content.to_ascii_lowercase()
                );
                let score = score_match(&haystack, &normalized_query, &query_tokens);
                if score <= 0 {
                    return None;
                }
                Some(MemorySearchHit {
                    id: record.id.clone(),
                    session_id: record.session_id.clone(),
                    kind: record.kind.clone(),
                    title: record.title.clone(),
                    preview: preview(&record.content, 180),
                    score,
                    created_at: record.created_at,
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

    async fn load_all_locked(&self) -> std::io::Result<BTreeMap<String, MemoryRecord>> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(BTreeMap::new());
        }

        let raw = tokio::fs::read_to_string(path).await?;
        if raw.trim().is_empty() {
            return Ok(BTreeMap::new());
        }

        serde_json::from_str::<BTreeMap<String, MemoryRecord>>(&raw)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    async fn save_all_locked(&self, index: &BTreeMap<String, MemoryRecord>) -> std::io::Result<()> {
        if let Some(parent) = self.index_path().parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let raw = serde_json::to_string_pretty(index)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        tokio::fs::write(self.index_path(), raw).await
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
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

    #[tokio::test]
    async fn upsert_and_search_records() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::new(dir.path().to_path_buf());
        store
            .upsert(&[MemoryRecord {
                id: "mem-1".to_string(),
                session_id: "s-1".to_string(),
                kind: "history_summary".to_string(),
                title: "Decision log".to_string(),
                content: "User asked for budget policy and the agent summarized older turns."
                    .to_string(),
                created_at: Utc::now(),
                metadata: BTreeMap::new(),
            }])
            .await
            .unwrap();

        let hits = store.search("budget policy", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "mem-1");
        assert!(store.get("mem-1").await.unwrap().is_some());
    }
}
