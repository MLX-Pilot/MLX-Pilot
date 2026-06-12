//! Compare — persisted side-by-side model comparisons (optionally blind) with voting.

use crate::state_store::StateStore;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One model's response within a comparison run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComparisonEntry {
    /// Blind label shown to the user while voting (e.g. "A", "B", "C").
    pub label: String,
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default)]
    pub error: String,
}

/// A full comparison run: one prompt fanned out to N models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Comparison {
    pub id: String,
    pub prompt: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default = "default_true")]
    pub blind: bool,
    #[serde(default)]
    pub entries: Vec<ComparisonEntry>,
    #[serde(default)]
    pub synthesis: String,
    /// Label of the entry the user voted as the winner ("" until voted).
    #[serde(default)]
    pub winner_label: String,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
}

fn default_true() -> bool {
    true
}

impl Comparison {
    pub fn new(prompt: String, system_prompt: String, blind: bool) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt,
            system_prompt,
            blind,
            entries: Vec::new(),
            synthesis: String::new(),
            winner_label: String::new(),
            created_at: Utc::now(),
        }
    }
}

/// Persistent store for [`Comparison`] runs, sharing the agent `state.sqlite` database.
pub struct CompareStore {
    state: StateStore,
}

impl CompareStore {
    pub async fn new(root: PathBuf) -> std::io::Result<Self> {
        let db_path = root
            .parent()
            .unwrap_or(root.as_path())
            .join("agent")
            .join("state.sqlite");
        let state = StateStore::new(db_path).await?;
        Ok(Self { state })
    }

    pub async fn list(&self, limit: usize) -> std::io::Result<Vec<Comparison>> {
        self.state.list_comparisons(limit).await
    }

    pub async fn get(&self, id: &str) -> std::io::Result<Option<Comparison>> {
        self.state.get_comparison(id).await
    }

    pub async fn save(&self, comparison: &Comparison) -> std::io::Result<()> {
        self.state.upsert_comparison(comparison).await
    }

    pub async fn set_vote(&self, id: &str, winner_label: &str) -> std::io::Result<()> {
        self.state.set_comparison_vote(id, winner_label).await
    }

    pub async fn delete(&self, id: &str) -> std::io::Result<()> {
        self.state.delete_comparison(id).await
    }
}
