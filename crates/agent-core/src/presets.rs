//! Presets — saved model/parameter/prompt profiles, persisted in the shared SQLite store.

use crate::state_store::StateStore;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A reusable generation profile: a system prompt plus default parameters and an
/// optional preferred provider/model. Applied server-side to chat/agent requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Text injected immediately before the user's message.
    #[serde(default)]
    pub prefix: String,
    /// Text injected immediately after the user's message.
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

impl Preset {
    pub fn new(name: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description: String::new(),
            provider_id: String::new(),
            model_id: String::new(),
            system_prompt: String::new(),
            temperature: None,
            max_tokens: None,
            top_p: None,
            prefix: String::new(),
            suffix: String::new(),
            tags: Vec::new(),
            favorite: false,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Persistent store for [`Preset`] records, sharing the agent `state.sqlite` database.
pub struct PresetStore {
    state: StateStore,
}

impl PresetStore {
    pub async fn new(root: PathBuf) -> std::io::Result<Self> {
        let db_path = root
            .parent()
            .unwrap_or(root.as_path())
            .join("agent")
            .join("state.sqlite");
        let state = StateStore::new(db_path).await?;
        Ok(Self { state })
    }

    pub async fn list(&self) -> std::io::Result<Vec<Preset>> {
        self.state.list_presets().await
    }

    pub async fn get(&self, id: &str) -> std::io::Result<Option<Preset>> {
        self.state.get_preset(id).await
    }

    pub async fn save(&self, preset: &Preset) -> std::io::Result<()> {
        self.state.upsert_preset(preset).await
    }

    pub async fn delete(&self, id: &str) -> std::io::Result<()> {
        self.state.delete_preset(id).await
    }
}
