//! Local memory store for compact context artifacts and durable agent memory.
//!
//! Supports hybrid search: FTS (keyword) + semantic (embedding cosine similarity),
//! degrading gracefully to FTS-only when no embedder is available.

use crate::embeddings::{cosine_similarity, Embedder};
use crate::state_store::StateStore;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, warn};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// Embedding vector (not serialized in API responses by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    /// Dimension of the embedding.
    #[serde(default)]
    pub embedding_dim: usize,
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
    /// Whether this hit included a semantic match component.
    #[serde(default)]
    pub semantic: bool,
}

pub struct MemoryStore {
    root: PathBuf,
    state: StateStore,
    embedder: Option<Arc<dyn Embedder>>,
    /// Weight of cosine similarity in hybrid score (0.0 to 1.0).
    /// FTS weight = 1.0 - semantic_weight.
    semantic_weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryPromotionDecision {
    pub record: MemoryRecord,
    pub reason: String,
}

impl MemoryStore {
    /// Create a new MemoryStore with no embedder (FTS-only mode).
    pub async fn new(root: PathBuf) -> std::io::Result<Self> {
        let db_path = root
            .parent()
            .unwrap_or(root.as_path())
            .join("agent")
            .join("state.sqlite");
        let state = StateStore::new(db_path).await?;
        let store = Self {
            root,
            state,
            embedder: None,
            semantic_weight: 0.6,
        };
        store.import_legacy_if_needed_blocking()?;
        Ok(store)
    }

    /// Create a MemoryStore with an embedder for semantic search.
    pub async fn with_embedder(
        root: PathBuf,
        embedder: Arc<dyn Embedder>,
    ) -> std::io::Result<Self> {
        let db_path = root
            .parent()
            .unwrap_or(root.as_path())
            .join("agent")
            .join("state.sqlite");
        let state = StateStore::new(db_path).await?;
        let store = Self {
            root,
            state,
            embedder: Some(embedder),
            semantic_weight: 0.6,
        };
        store.import_legacy_if_needed_blocking()?;
        Ok(store)
    }

    /// Set the semantic weight for hybrid search (0.0 = FTS only, 1.0 = semantic only).
    pub fn set_semantic_weight(&mut self, weight: f32) {
        self.semantic_weight = weight.clamp(0.0, 1.0);
    }

    /// Whether an embedder is active and ready.
    pub fn has_semantic(&self) -> bool {
        self.embedder
            .as_ref()
            .map(|e| e.is_ready())
            .unwrap_or(false)
    }

    /// Name of the active embedder, or "none".
    pub fn embedder_name(&self) -> &str {
        self.embedder.as_ref().map(|e| e.name()).unwrap_or("none")
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

    /// Hybrid search — combines FTS and semantic scores when embeddings are available.
    pub async fn search(&self, query: &str, limit: usize) -> std::io::Result<Vec<MemorySearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        // Determine if we can do semantic search.
        let can_semantic = self.has_semantic();
        let query_embedding = if can_semantic {
            match self
                .embedder
                .as_ref()
                .unwrap()
                .embed(&[query.to_string()])
                .await
            {
                Ok(mut vecs) if !vecs.is_empty() => Some(vecs.remove(0)),
                Ok(_) => {
                    warn!("embedder returned empty result for query");
                    None
                }
                Err(e) => {
                    warn!("embedder failed for query: {e}");
                    None
                }
            }
        } else {
            None
        };

        // Get all records with embeddings loaded.
        let records = self.state.load_all_memory_records().await?;

        // FTS search
        let fts_hits = self
            .state
            .fts_memory_search(query, limit.max(records.len()))
            .await?;

        if can_semantic && query_embedding.is_some() {
            let q_emb = query_embedding.as_ref().unwrap();
            let sem_weight = self.semantic_weight;
            let fts_weight = 1.0 - sem_weight;

            // Compute max FTS score for normalization
            let max_fts_score = fts_hits.first().map(|(_, _, s)| *s).unwrap_or(1).max(1) as f32;

            // Build a map of record_id -> FTS score
            let mut fts_map: BTreeMap<String, i64> = BTreeMap::new();
            for (record, _, score) in &fts_hits {
                let existing = fts_map.get(&record.id).copied().unwrap_or(0);
                fts_map.insert(record.id.clone(), existing.max(*score));
            }

            // Compute semantic scores and combine with FTS.
            let mut combined: Vec<(MemoryRecord, i64, bool)> = records
                .into_iter()
                .filter_map(|record| {
                    let emb = record.embedding.as_ref()?;
                    if emb.is_empty() || emb.len() != q_emb.len() {
                        return None;
                    }
                    let cos_sim = cosine_similarity(emb, q_emb);
                    let sem_score = (cos_sim.max(0.0) * 1000.0) as i64;
                    let fts_norm = fts_map
                        .get(&record.id)
                        .map(|s| (*s as f32 / max_fts_score * 1000.0) as i64)
                        .unwrap_or(0);

                    let hybrid_score =
                        (sem_weight * sem_score as f32 + fts_weight * fts_norm as f32) as i64;
                    let final_score = hybrid_score + i64::from(record.importance.max(0));

                    if final_score <= 0 {
                        return None;
                    }

                    Some((record, final_score, true))
                })
                .collect();

            combined.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| b.0.created_at.cmp(&a.0.created_at))
            });
            combined.truncate(limit.max(1));

            let hits: Vec<MemorySearchHit> = combined
                .into_iter()
                .map(|(record, score, semantic)| MemorySearchHit {
                    id: record.id,
                    session_id: record.session_id,
                    kind: record.kind,
                    title: record.title,
                    preview: preview(&record.content, 180),
                    score,
                    created_at: record.created_at,
                    scope: record.scope,
                    namespace: record.namespace,
                    semantic,
                })
                .collect();

            return Ok(hits);
        }

        // Fallback: FTS-only search (existing logic).
        if !fts_hits.is_empty() {
            let mut hits: Vec<MemorySearchHit> = fts_hits
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
                    semantic: false,
                })
                .collect();
            hits.sort_by(|left, right| {
                right
                    .score
                    .cmp(&left.score)
                    .then_with(|| right.created_at.cmp(&left.created_at))
            });
            hits.truncate(limit.max(1));
            return Ok(hits);
        }

        // Keyword fallback (no FTS index, no embeddings).
        let records = self.state.load_all_memory_records().await?;
        let query_tokens = tokenize(query);
        let normalized_query = query.to_ascii_lowercase();

        let mut hits: Vec<MemorySearchHit> = records
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
                    semantic: false,
                })
            })
            .collect();

        hits.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.created_at.cmp(&left.created_at))
        });
        hits.truncate(limit.max(1));
        Ok(hits)
    }

    /// List stored records, newest first, with optional scope/kind filters.
    /// `pinned_only` restricts to records whose `pin_state == "pinned"`.
    pub async fn list(
        &self,
        scope: Option<&str>,
        kind: Option<&str>,
        pinned_only: bool,
        limit: usize,
    ) -> std::io::Result<Vec<MemoryRecord>> {
        let mut records = self.state.load_all_memory_records().await?;
        records.retain(|record| {
            scope.map(|s| record.scope == s).unwrap_or(true)
                && kind.map(|k| record.kind == k).unwrap_or(true)
                && (!pinned_only || record.pin_state == "pinned")
        });
        if limit > 0 && records.len() > limit {
            records.truncate(limit);
        }
        Ok(records)
    }

    /// Save (insert or update) a single record, optionally generating embeddings.
    pub async fn save(&self, record: &MemoryRecord) -> std::io::Result<()> {
        let mut enriched = record.clone();
        self.enrich_embedding(&mut enriched).await;
        self.state
            .upsert_memory_records(std::slice::from_ref(&enriched))
            .await
    }

    /// Upsert a batch of records with embedding generation.
    pub async fn upsert_enriched(&self, records: &[MemoryRecord]) -> std::io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let mut enriched = records.to_vec();
        for record in &mut enriched {
            self.enrich_embedding(record).await;
        }
        self.state.upsert_memory_records(&enriched).await
    }

    /// Delete a record by id.
    pub async fn delete(&self, id: &str) -> std::io::Result<()> {
        self.state.delete_memory_record(id).await
    }

    /// Pin (`pinned`) or unpin (`auto`) a record so it survives auto-pruning.
    pub async fn set_pin(&self, id: &str, pinned: bool) -> std::io::Result<bool> {
        let Some(mut record) = self.state.get_memory_record(id).await? else {
            return Ok(false);
        };
        record.pin_state = if pinned {
            "pinned".to_string()
        } else {
            "auto".to_string()
        };
        self.state
            .upsert_memory_records(std::slice::from_ref(&record))
            .await?;
        Ok(true)
    }

    /// Reindex all records: recompute embeddings for records that don't have one.
    /// Returns the number of records that were updated.
    pub async fn reindex(&self) -> std::io::Result<usize> {
        let embedder = match self.embedder.as_ref() {
            Some(e) if e.is_ready() => e,
            _ => {
                return Err(std::io::Error::other(
                    "no embedder configured or embedder not ready",
                ))
            }
        };

        let records = self.state.load_all_memory_records().await?;
        let to_update: Vec<&MemoryRecord> = records
            .iter()
            .filter(|r| r.embedding.is_none() || r.embedding_dim == 0)
            .collect();

        if to_update.is_empty() {
            return Ok(0);
        }

        let total = to_update.len();
        debug!("reindexing {total} memory records");

        // Process in batches of 32 to avoid overwhelming the embedder.
        let batch_size = 32;
        let mut updated = 0_usize;

        for batch in to_update.chunks(batch_size) {
            let texts: Vec<String> = batch
                .iter()
                .map(|r| format!("{}: {} {}", r.kind, r.title, r.content))
                .collect();

            match embedder.embed(&texts).await {
                Ok(embeddings) => {
                    for (record, emb) in batch.iter().zip(embeddings.iter()) {
                        let mut enriched = (*record).clone();
                        enriched.embedding = Some(emb.clone());
                        enriched.embedding_dim = emb.len();
                        self.state
                            .upsert_memory_records(std::slice::from_ref(&enriched))
                            .await?;
                        updated += 1;
                    }
                }
                Err(e) => {
                    warn!("embedder failed during reindex batch: {e}");
                    // Continue with remaining batches.
                }
            }
        }

        Ok(updated)
    }

    /// Generate an embedding for a record if an embedder is available.
    async fn enrich_embedding(&self, record: &mut MemoryRecord) {
        let embedder = match self.embedder.as_ref() {
            Some(e) if e.is_ready() => e,
            _ => return,
        };

        let text = format!("{}: {} {}", record.kind, record.title, record.content);
        match embedder.embed(&[text]).await {
            Ok(mut vecs) if !vecs.is_empty() => {
                let emb = vecs.remove(0);
                record.embedding_dim = emb.len();
                record.embedding = Some(emb);
            }
            Ok(_) => {
                debug!("embedder returned empty result for record {}", record.id);
            }
            Err(e) => {
                warn!("embedder failed for record {}: {e}", record.id);
                // Graceful degradation: save without embedding.
            }
        }
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
                embedding: None,
                embedding_dim: 0,
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
