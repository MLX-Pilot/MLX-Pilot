//! `SessionStore` — JSONL-based session persistence.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::RwLock;

/// A persisted message in a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Metadata for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub name: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
}

/// JSONL-based session persistence.
///
/// Each session is stored as a `.jsonl` file under `sessions_dir/{id}.jsonl`.
/// An `index.json` file is maintained for quick listing.
pub struct SessionStore {
    pub sessions_dir: PathBuf,
    index_cache: Arc<RwLock<BTreeMap<String, SessionMeta>>>,
}

impl SessionStore {
    /// Create a new session store targeting the given directory.
    pub async fn new(sessions_dir: PathBuf) -> std::io::Result<Self> {
        if !sessions_dir.exists() {
            tokio::fs::create_dir_all(&sessions_dir).await?;
        }

        let store = Self {
            sessions_dir,
            index_cache: Arc::new(RwLock::new(BTreeMap::new())),
        };

        store.load_index().await?;
        Ok(store)
    }

    /// Generate a new session ID.
    pub fn new_session_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn index_path(&self) -> PathBuf {
        self.sessions_dir.join("index.json")
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{}.jsonl", session_id))
    }

    async fn load_index(&self) -> std::io::Result<()> {
        let index_path = self.index_path();
        if !index_path.exists() {
            return Ok(());
        }

        let content = tokio::fs::read_to_string(&index_path).await?;
        if content.trim().is_empty() {
            return Ok(());
        }

        let index: BTreeMap<String, SessionMeta> = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        *self.index_cache.write().await = index;
        Ok(())
    }

    async fn save_index(&self) -> std::io::Result<()> {
        let index_path = self.index_path();
        let index = self.index_cache.read().await;
        let content = serde_json::to_string_pretty(&*index)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        tokio::fs::write(&index_path, content).await?;
        Ok(())
    }

    /// Creates a new session in the index if it doesn't exist.
    pub async fn ensure_session(&self, session_id: &str, initial_name: Option<String>) -> std::io::Result<()> {
        let mut index = self.index_cache.write().await;
        if !index.contains_key(session_id) {
            let name = initial_name.unwrap_or_else(|| "Nova conversa".to_string());
            let meta = SessionMeta {
                id: session_id.to_string(),
                name,
                updated_at: chrono::Utc::now(),
                message_count: 0,
            };
            index.insert(session_id.to_string(), meta);
            drop(index);
            self.save_index().await?;
        }
        Ok(())
    }

    /// Append a message to a session.
    pub async fn append(
        &self,
        session_id: &str,
        message: &SessionMessage,
    ) -> Result<(), std::io::Error> {
        self.ensure_session(session_id, None).await?;

        let path = self.session_path(session_id);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        let mut writer = BufWriter::new(file);
        let mut json = serde_json::to_string(message)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        json.push('\n');

        writer.write_all(json.as_bytes()).await?;
        writer.flush().await?;

        // Update index metadata
        let mut index = self.index_cache.write().await;
        if let Some(meta) = index.get_mut(session_id) {
            meta.updated_at = message.timestamp;
            meta.message_count += 1;
            
            // Auto-generate name from first user message if name is default
            if meta.message_count == 1 && meta.name == "Nova conversa" && message.role == "user" {
                // Take first 30 chars
                let snippet: String = message.content.chars().take(30).collect();
                meta.name = if message.content.chars().count() > 30 {
                    format!("{}...", snippet)
                } else {
                    snippet
                };
            }
        }
        drop(index);
        self.save_index().await?;

        Ok(())
    }

    /// Load all messages for a session.
    pub async fn load(&self, session_id: &str) -> Result<Vec<SessionMessage>, std::io::Error> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut messages = Vec::new();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let message: SessionMessage = serde_json::from_str(&line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            messages.push(message);
        }

        Ok(messages)
    }

    /// List all session metadata.
    pub async fn list_sessions(&self) -> Result<Vec<SessionMeta>, std::io::Error> {
        let index = self.index_cache.read().await;
        let mut sessions: Vec<SessionMeta> = index.values().cloned().collect();
        // Sort descending by updated_at
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    /// Rename a session
    pub async fn rename(&self, session_id: &str, new_name: &str) -> Result<(), std::io::Error> {
        let mut index = self.index_cache.write().await;
        if let Some(meta) = index.get_mut(session_id) {
            meta.name = new_name.to_string();
            drop(index);
            self.save_index().await?;
            Ok(())
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Sessão não encontrada"))
        }
    }

    /// Delete a session
    pub async fn delete(&self, session_id: &str) -> std::io::Result<()> {
        let mut index = self.index_cache.write().await;
        if index.remove(session_id).is_some() {
            drop(index);
            self.save_index().await?;
        }

        let path = self.session_path(session_id);
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
        }

        Ok(())
    }
    
    /// Export session to JSON string
    pub async fn export(&self, session_id: &str) -> std::io::Result<String> {
        let messages = self.load(session_id).await?;
        let json = serde_json::to_string_pretty(&messages)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(json)
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
    async fn session_store_operations() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(temp_dir.path().to_path_buf()).await.unwrap();
        
        // 1. Load empty
        let messages = store.load("nonexistent").await.unwrap();
        assert!(messages.is_empty());

        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
        
        // 2. Append message
        let session_id = SessionStore::new_session_id();
        let msg1 = SessionMessage {
            role: "user".to_string(),
            content: "Hello agent".to_string(),
            tool_call_id: None,
            tool_name: None,
            timestamp: chrono::Utc::now(),
        };
        store.append(&session_id, &msg1).await.unwrap();
        
        let loaded = store.load(&session_id).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "Hello agent");
        
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].message_count, 1);
        assert_eq!(sessions[0].name, "Hello agent");
        
        // 3. Rename
        store.rename(&session_id, "Greeting test").await.unwrap();
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions[0].name, "Greeting test");
        
        // 4. Delete
        store.delete(&session_id).await.unwrap();
        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }
}
