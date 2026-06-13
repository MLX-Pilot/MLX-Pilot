//! SQLite-backed persistent state for sessions and memory.

use crate::compare::{Comparison, ComparisonEntry};
use crate::memory::MemoryRecord;
use crate::presets::Preset;
use crate::session::{SessionMessage, SessionMeta, SessionSnapshot};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct StateStore {
    db_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionSearchCandidate {
    pub meta: SessionMeta,
    pub transcript: String,
    pub preview: String,
    pub raw_score: i64,
}

impl StateStore {
    pub async fn new(db_path: PathBuf) -> io::Result<Self> {
        let store = Self { db_path };
        store.initialize().await?;
        Ok(store)
    }

    async fn initialize(&self) -> io::Result<()> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let conn = open_connection(&db_path)?;
            conn.execute_batch(
                r#"
                PRAGMA journal_mode = WAL;
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    provider_id TEXT NOT NULL DEFAULT '',
                    model_id TEXT NOT NULL DEFAULT '',
                    workspace_root TEXT NOT NULL DEFAULT '',
                    origin_kind TEXT NOT NULL DEFAULT 'local',
                    parent_session_id TEXT,
                    status TEXT NOT NULL DEFAULT 'active',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    last_activity_at TEXT NOT NULL DEFAULT '',
                    summary TEXT NOT NULL DEFAULT '',
                    source_channel TEXT NOT NULL DEFAULT '',
                    thread_id TEXT NOT NULL DEFAULT '',
                    correlation_id TEXT NOT NULL DEFAULT '',
                    message_count INTEGER NOT NULL DEFAULT 0,
                    folder TEXT NOT NULL DEFAULT '',
                    archived INTEGER NOT NULL DEFAULT 0,
                    pinned INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS session_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    role TEXT NOT NULL DEFAULT '',
                    tool_name TEXT,
                    tool_call_id TEXT,
                    content TEXT NOT NULL DEFAULT '',
                    content_json TEXT,
                    metadata_json TEXT,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_session_events_session_created
                ON session_events(session_id, created_at);

                CREATE TABLE IF NOT EXISTS memory_records (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL DEFAULT '',
                    source_session_id TEXT NOT NULL DEFAULT '',
                    scope TEXT NOT NULL DEFAULT 'session',
                    namespace TEXT NOT NULL DEFAULT 'default',
                    kind TEXT NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    tags_json TEXT,
                    metadata_json TEXT,
                    importance INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    last_accessed_at TEXT,
                    pin_state TEXT NOT NULL DEFAULT 'auto',
                    promotion_source TEXT NOT NULL DEFAULT '',
                    summary_ref TEXT NOT NULL DEFAULT '',
                    embedding BLOB,
                    embedding_dim INTEGER NOT NULL DEFAULT 0
                );

                CREATE INDEX IF NOT EXISTS idx_memory_scope_namespace_created
                ON memory_records(scope, namespace, created_at DESC);

                CREATE TABLE IF NOT EXISTS session_summaries (
                    session_id TEXT PRIMARY KEY,
                    summary TEXT NOT NULL DEFAULT '',
                    summary_json TEXT,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS session_context_snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    snapshot_text TEXT NOT NULL DEFAULT '',
                    snapshot_json TEXT,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_session_snapshots_session_created
                ON session_context_snapshots(session_id, created_at DESC);
                "#,
            )
            .map_err(sql_error)?;
            ensure_column(
                &conn,
                "sessions",
                "last_activity_at",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            ensure_column(&conn, "sessions", "summary", "TEXT NOT NULL DEFAULT ''")?;
            ensure_column(
                &conn,
                "sessions",
                "source_channel",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            ensure_column(&conn, "sessions", "thread_id", "TEXT NOT NULL DEFAULT ''")?;
            ensure_column(
                &conn,
                "sessions",
                "correlation_id",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            ensure_column(
                &conn,
                "memory_records",
                "pin_state",
                "TEXT NOT NULL DEFAULT 'auto'",
            )?;
            ensure_column(
                &conn,
                "memory_records",
                "promotion_source",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            ensure_column(
                &conn,
                "memory_records",
                "summary_ref",
                "TEXT NOT NULL DEFAULT ''",
            )?;
            // Wave-1 additive columns (legacy DBs created before these existed).
            ensure_column(&conn, "sessions", "folder", "TEXT NOT NULL DEFAULT ''")?;
            ensure_column(&conn, "sessions", "archived", "INTEGER NOT NULL DEFAULT 0")?;
            ensure_column(&conn, "sessions", "pinned", "INTEGER NOT NULL DEFAULT 0")?;
            ensure_column(&conn, "memory_records", "embedding", "BLOB")?;
            ensure_column(
                &conn,
                "memory_records",
                "embedding_dim",
                "INTEGER NOT NULL DEFAULT 0",
            )?;

            let _ = conn.execute_batch(
                r#"
                CREATE VIRTUAL TABLE IF NOT EXISTS session_events_fts
                USING fts5(event_id UNINDEXED, session_id UNINDEXED, content);

                CREATE VIRTUAL TABLE IF NOT EXISTS memory_records_fts
                USING fts5(record_id UNINDEXED, title, content);
                "#,
            );
            run_migrations(&conn)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn upsert_session_meta(&self, meta: &SessionMeta) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let meta = meta.clone();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute(
                r#"
                INSERT INTO sessions (
                    id, name, provider_id, model_id, workspace_root, origin_kind,
                    parent_session_id, status, created_at, updated_at, last_activity_at,
                    summary, source_channel, thread_id, correlation_id, message_count,
                    folder, archived, pinned
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    provider_id = excluded.provider_id,
                    model_id = excluded.model_id,
                    workspace_root = excluded.workspace_root,
                    origin_kind = excluded.origin_kind,
                    parent_session_id = excluded.parent_session_id,
                    status = excluded.status,
                    updated_at = excluded.updated_at,
                    last_activity_at = excluded.last_activity_at,
                    summary = excluded.summary,
                    source_channel = excluded.source_channel,
                    thread_id = excluded.thread_id,
                    correlation_id = excluded.correlation_id,
                    folder = excluded.folder,
                    archived = excluded.archived,
                    pinned = excluded.pinned
                "#,
                params![
                    meta.id,
                    meta.name,
                    meta.provider_id,
                    meta.model_id,
                    meta.workspace_root,
                    meta.origin_kind,
                    meta.parent_session_id,
                    meta.status,
                    meta.created_at.to_rfc3339(),
                    meta.updated_at.to_rfc3339(),
                    meta.last_activity_at.to_rfc3339(),
                    meta.summary,
                    meta.source_channel,
                    meta.thread_id,
                    meta.correlation_id,
                    meta.message_count as i64,
                    meta.folder,
                    meta.archived as i64,
                    meta.pinned as i64,
                ],
            )
            .map_err(sql_error)?;
            if !meta.summary.trim().is_empty() {
                conn.execute(
                    r#"
                    INSERT INTO session_summaries (session_id, summary, summary_json, updated_at)
                    VALUES (?1, ?2, NULL, ?3)
                    ON CONFLICT(session_id) DO UPDATE SET
                        summary = excluded.summary,
                        updated_at = excluded.updated_at
                    "#,
                    params![meta.id, meta.summary, meta.updated_at.to_rfc3339()],
                )
                .map_err(sql_error)?;
            }
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn append_session_event(
        &self,
        session_id: &str,
        message: &SessionMessage,
    ) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        let message = message.clone();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute(
                r#"
                INSERT INTO session_events (
                    session_id, kind, role, tool_name, tool_call_id, content,
                    content_json, metadata_json, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
                params![
                    session_id,
                    message.kind,
                    message.role,
                    message.tool_name,
                    message.tool_call_id,
                    message.content,
                    message.content_json.map(|value| value.to_string()),
                    message.metadata_json.map(|value| value.to_string()),
                    message.timestamp.to_rfc3339(),
                ],
            )
            .map_err(sql_error)?;
            let event_id = conn.last_insert_rowid();
            if table_exists(&conn, "session_events_fts") {
                let _ = conn.execute(
                    "INSERT INTO session_events_fts(event_id, session_id, content) VALUES (?1, ?2, ?3)",
                    params![event_id, session_id, message.content],
                );
            }
            conn.execute(
                "UPDATE sessions SET updated_at = ?2, last_activity_at = ?2, message_count = message_count + 1 WHERE id = ?1",
                params![session_id, message.timestamp.to_rfc3339()],
            )
            .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn load_session_events(&self, session_id: &str) -> io::Result<Vec<SessionMessage>> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<Vec<SessionMessage>> {
            let conn = open_connection(&db_path)?;
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT role, content, tool_call_id, tool_name, created_at, kind,
                           content_json, metadata_json
                    FROM session_events
                    WHERE session_id = ?1
                    ORDER BY id ASC
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt
                .query_map([session_id], |row| {
                    Ok(SessionMessage {
                        role: row.get(0)?,
                        content: row.get(1)?,
                        tool_call_id: row.get(2)?,
                        tool_name: row.get(3)?,
                        timestamp: parse_datetime(&row.get::<_, String>(4)?),
                        kind: row.get(5)?,
                        content_json: parse_json_opt(row.get(6)?),
                        metadata_json: parse_json_opt(row.get(7)?),
                    })
                })
                .map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn get_session_meta(&self, session_id: &str) -> io::Result<Option<SessionMeta>> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<Option<SessionMeta>> {
            let conn = open_connection(&db_path)?;
            conn.query_row(
                r#"
                    SELECT id, name, updated_at, last_activity_at, message_count, provider_id, model_id,
                       workspace_root, origin_kind, parent_session_id, status, created_at, summary,
                       source_channel, thread_id, correlation_id, folder, archived, pinned
                FROM sessions
                WHERE id = ?1
                "#,
                [session_id],
                row_to_session_meta,
            )
            .optional()
            .map_err(sql_error)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn list_sessions(&self) -> io::Result<Vec<SessionMeta>> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> io::Result<Vec<SessionMeta>> {
            let conn = open_connection(&db_path)?;
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT id, name, updated_at, last_activity_at, message_count, provider_id, model_id,
                           workspace_root, origin_kind, parent_session_id, status, created_at, summary,
                           source_channel, thread_id, correlation_id, folder, archived, pinned
                    FROM sessions
                    ORDER BY updated_at DESC
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt.query_map([], row_to_session_meta).map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn rename_session(&self, session_id: &str, new_name: &str) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        let new_name = new_name.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            let updated = conn
                .execute(
                    "UPDATE sessions SET name = ?2, updated_at = ?3 WHERE id = ?1",
                    params![session_id, new_name, Utc::now().to_rfc3339()],
                )
                .map_err(sql_error)?;
            if updated == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Sessao nao encontrada",
                ));
            }
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn delete_session(&self, session_id: &str) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])
                .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn session_search_candidates(&self) -> io::Result<Vec<SessionSearchCandidate>> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> io::Result<Vec<SessionSearchCandidate>> {
            let conn = open_connection(&db_path)?;
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT s.id, s.name, s.updated_at, s.last_activity_at, s.message_count,
                           s.provider_id, s.model_id, s.workspace_root, s.origin_kind,
                           s.parent_session_id, s.status, s.created_at, s.summary,
                           s.source_channel, s.thread_id, s.correlation_id,
                           s.folder, s.archived, s.pinned,
                           COALESCE(GROUP_CONCAT(e.content, ' '), '')
                    FROM sessions s
                    LEFT JOIN session_events e ON e.session_id = s.id
                    GROUP BY s.id, s.name, s.updated_at, s.last_activity_at, s.message_count,
                             s.provider_id, s.model_id, s.workspace_root, s.origin_kind,
                             s.parent_session_id, s.status, s.created_at, s.summary,
                             s.source_channel, s.thread_id, s.correlation_id,
                             s.folder, s.archived, s.pinned
                    ORDER BY s.updated_at DESC
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(SessionSearchCandidate {
                        meta: row_to_session_meta(row)?,
                        transcript: row.get::<_, String>(19)?,
                        preview: String::new(),
                        raw_score: 0,
                    })
                })
                .map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn fts_session_search_candidates(
        &self,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<SessionSearchCandidate>> {
        let db_path = self.db_path.clone();
        let query = fts_query_string(query);
        tokio::task::spawn_blocking(move || -> io::Result<Vec<SessionSearchCandidate>> {
            let conn = open_connection(&db_path)?;
            if !table_exists(&conn, "session_events_fts") {
                return Ok(Vec::new());
            }
            if query.trim().is_empty() {
                return Ok(Vec::new());
            }

            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT s.id, s.name, s.updated_at, s.last_activity_at, s.message_count,
                           s.provider_id, s.model_id, s.workspace_root, s.origin_kind,
                           s.parent_session_id, s.status, s.created_at, s.summary,
                           s.source_channel, s.thread_id, s.correlation_id,
                           s.folder, s.archived, s.pinned,
                           COALESCE(se.content, ''),
                           snippet(session_events_fts, 2, '[', ']', '...', 18) AS preview,
                           CAST((-bm25(session_events_fts)) * 1000 AS INTEGER) AS raw_score
                    FROM session_events_fts
                    JOIN session_events se ON se.id = CAST(session_events_fts.event_id AS INTEGER)
                    JOIN sessions s ON s.id = se.session_id
                    WHERE session_events_fts MATCH ?1
                    ORDER BY bm25(session_events_fts)
                    LIMIT ?2
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt
                .query_map(params![query, limit.max(1) as i64], |row| {
                    Ok(SessionSearchCandidate {
                        meta: row_to_session_meta(row)?,
                        transcript: row.get::<_, String>(19)?,
                        preview: row.get::<_, String>(20).unwrap_or_default(),
                        raw_score: row.get::<_, i64>(21).unwrap_or_default(),
                    })
                })
                .map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn upsert_memory_records(&self, records: &[MemoryRecord]) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let records = records.to_vec();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let mut conn = open_connection(&db_path)?;
            let tx = conn.transaction().map_err(sql_error)?;
            for record in records {
                let embedding_blob: Option<Vec<u8>> = record
                    .embedding
                    .as_ref()
                    .map(|emb| crate::embeddings::serialize_embedding(emb));
                tx.execute(
                    r#"
                    INSERT INTO memory_records (
                        id, session_id, source_session_id, scope, namespace, kind, title, content,
                        tags_json, metadata_json, importance, created_at, last_accessed_at,
                        pin_state, promotion_source, summary_ref, embedding, embedding_dim
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                    ON CONFLICT(id) DO UPDATE SET
                        session_id = excluded.session_id,
                        source_session_id = excluded.source_session_id,
                        scope = excluded.scope,
                        namespace = excluded.namespace,
                        kind = excluded.kind,
                        title = excluded.title,
                        content = excluded.content,
                        tags_json = excluded.tags_json,
                        metadata_json = excluded.metadata_json,
                        importance = excluded.importance,
                        created_at = excluded.created_at,
                        last_accessed_at = excluded.last_accessed_at,
                        pin_state = excluded.pin_state,
                        promotion_source = excluded.promotion_source,
                        summary_ref = excluded.summary_ref,
                        embedding = excluded.embedding,
                        embedding_dim = excluded.embedding_dim
                    "#,
                    params![
                        record.id,
                        record.session_id,
                        record.source_session_id,
                        record.scope,
                        record.namespace,
                        record.kind,
                        record.title,
                        record.content,
                        serde_json::to_string(&record.tags).unwrap_or_else(|_| "[]".to_string()),
                        serde_json::to_string(&record.metadata)
                            .unwrap_or_else(|_| "{}".to_string()),
                        record.importance,
                        record.created_at.to_rfc3339(),
                        record.last_accessed_at.map(|value| value.to_rfc3339()),
                        record.pin_state,
                        record.promotion_source,
                        record.summary_ref,
                        embedding_blob,
                        record.embedding_dim as i64,
                    ],
                )
                .map_err(sql_error)?;
                if table_exists(&tx, "memory_records_fts") {
                    let _ = tx.execute(
                        "DELETE FROM memory_records_fts WHERE record_id = ?1",
                        params![record.id],
                    );
                    let _ = tx.execute(
                        "INSERT INTO memory_records_fts(record_id, title, content) VALUES (?1, ?2, ?3)",
                        params![record.id, record.title, record.content],
                    );
                }
            }
            tx.commit().map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn get_memory_record(&self, id: &str) -> io::Result<Option<MemoryRecord>> {
        let db_path = self.db_path.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<Option<MemoryRecord>> {
            let conn = open_connection(&db_path)?;
            let record = conn
                .query_row(
                    r#"
                    SELECT id, session_id, source_session_id, scope, namespace, kind, title, content,
                           tags_json, metadata_json, importance, created_at, last_accessed_at,
                           pin_state, promotion_source, summary_ref, embedding, embedding_dim
                    FROM memory_records
                    WHERE id = ?1
                    "#,
                    [id],
                    row_to_memory_record,
                )
                .optional()
                .map_err(sql_error)?;
            if let Some(existing) = record.as_ref() {
                let _ = conn.execute(
                    "UPDATE memory_records SET last_accessed_at = ?2 WHERE id = ?1",
                    params![existing.id, Utc::now().to_rfc3339()],
                );
            }
            Ok(record)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn load_all_memory_records(&self) -> io::Result<Vec<MemoryRecord>> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> io::Result<Vec<MemoryRecord>> {
            let conn = open_connection(&db_path)?;
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT id, session_id, source_session_id, scope, namespace, kind, title, content,
                           tags_json, metadata_json, importance, created_at, last_accessed_at,
                           pin_state, promotion_source, summary_ref, embedding, embedding_dim
                    FROM memory_records
                    ORDER BY created_at DESC
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt.query_map([], row_to_memory_record).map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn fts_memory_search(
        &self,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<(MemoryRecord, String, i64)>> {
        let db_path = self.db_path.clone();
        let query = fts_query_string(query);
        tokio::task::spawn_blocking(move || -> io::Result<Vec<(MemoryRecord, String, i64)>> {
            let conn = open_connection(&db_path)?;
            if !table_exists(&conn, "memory_records_fts") {
                return Ok(Vec::new());
            }
            if query.trim().is_empty() {
                return Ok(Vec::new());
            }

            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT m.id, m.session_id, m.source_session_id, m.scope, m.namespace, m.kind,
                           m.title, m.content, m.tags_json, m.metadata_json, m.importance,
                           m.created_at, m.last_accessed_at, m.pin_state, m.promotion_source,
                           m.summary_ref, m.embedding, m.embedding_dim,
                           snippet(memory_records_fts, 2, '[', ']', '...', 18) AS preview,
                           CAST((-bm25(memory_records_fts)) * 1000 AS INTEGER) AS raw_score
                    FROM memory_records_fts
                    JOIN memory_records m ON m.id = memory_records_fts.record_id
                    WHERE memory_records_fts MATCH ?1
                    ORDER BY bm25(memory_records_fts)
                    LIMIT ?2
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt
                .query_map(params![query, limit.max(1) as i64], |row| {
                    Ok((
                        row_to_memory_record(row)?,
                        row.get::<_, String>(18).unwrap_or_default(),
                        row.get::<_, i64>(19).unwrap_or_default(),
                    ))
                })
                .map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn upsert_session_summary(
        &self,
        session_id: &str,
        summary: &str,
        summary_json: Option<serde_json::Value>,
    ) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        let summary = summary.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            let now = Utc::now().to_rfc3339();
            conn.execute(
                r#"
                INSERT INTO session_summaries (session_id, summary, summary_json, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(session_id) DO UPDATE SET
                    summary = excluded.summary,
                    summary_json = excluded.summary_json,
                    updated_at = excluded.updated_at
                "#,
                params![
                    session_id,
                    summary,
                    summary_json.map(|value| value.to_string()),
                    now,
                ],
            )
            .map_err(sql_error)?;
            conn.execute(
                "UPDATE sessions SET summary = ?2, updated_at = ?3, last_activity_at = ?3 WHERE id = ?1",
                params![session_id, summary, now],
            )
            .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn load_session_summary(&self, session_id: &str) -> io::Result<Option<String>> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<Option<String>> {
            let conn = open_connection(&db_path)?;
            conn.query_row(
                "SELECT summary FROM session_summaries WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_error)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn append_session_snapshot(
        &self,
        session_id: &str,
        snapshot_text: &str,
        snapshot_json: Option<serde_json::Value>,
    ) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        let snapshot_text = snapshot_text.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute(
                r#"
                INSERT INTO session_context_snapshots (session_id, snapshot_text, snapshot_json, created_at)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![
                    session_id,
                    snapshot_text,
                    snapshot_json.map(|value| value.to_string()),
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn latest_session_snapshot(
        &self,
        session_id: &str,
    ) -> io::Result<Option<SessionSnapshot>> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<Option<SessionSnapshot>> {
            let conn = open_connection(&db_path)?;
            conn.query_row(
                r#"
                SELECT session_id, snapshot_text, snapshot_json, created_at
                FROM session_context_snapshots
                WHERE session_id = ?1
                ORDER BY created_at DESC, id DESC
                LIMIT 1
                "#,
                [session_id],
                |row| {
                    Ok(SessionSnapshot {
                        session_id: row.get(0)?,
                        text: row.get(1)?,
                        snapshot_json: parse_json_opt(row.get(2)?),
                        created_at: parse_datetime(&row.get::<_, String>(3)?),
                    })
                },
            )
            .optional()
            .map_err(sql_error)
        })
        .await
        .map_err(join_error)?
    }

    // ────────────────────────────────────────────────────────────────────
    // Wave-1 additions: memory deletion, session organization/editing,
    // presets, and comparisons.
    // ────────────────────────────────────────────────────────────────────

    pub async fn delete_memory_record(&self, id: &str) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute("DELETE FROM memory_records WHERE id = ?1", [id.as_str()])
                .map_err(sql_error)?;
            if table_exists(&conn, "memory_records_fts") {
                let _ = conn.execute(
                    "DELETE FROM memory_records_fts WHERE record_id = ?1",
                    [id.as_str()],
                );
            }
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn set_session_flags(
        &self,
        session_id: &str,
        folder: Option<String>,
        archived: Option<bool>,
        pinned: Option<bool>,
    ) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            let now = Utc::now().to_rfc3339();
            if let Some(folder) = folder {
                conn.execute(
                    "UPDATE sessions SET folder = ?2, updated_at = ?3 WHERE id = ?1",
                    params![session_id, folder, now],
                )
                .map_err(sql_error)?;
            }
            if let Some(archived) = archived {
                conn.execute(
                    "UPDATE sessions SET archived = ?2 WHERE id = ?1",
                    params![session_id, archived as i64],
                )
                .map_err(sql_error)?;
            }
            if let Some(pinned) = pinned {
                conn.execute(
                    "UPDATE sessions SET pinned = ?2 WHERE id = ?1",
                    params![session_id, pinned as i64],
                )
                .map_err(sql_error)?;
            }
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn load_session_events_with_ids(
        &self,
        session_id: &str,
    ) -> io::Result<Vec<(i64, SessionMessage)>> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<Vec<(i64, SessionMessage)>> {
            let conn = open_connection(&db_path)?;
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT id, role, content, tool_call_id, tool_name, created_at, kind,
                           content_json, metadata_json
                    FROM session_events
                    WHERE session_id = ?1
                    ORDER BY id ASC
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt
                .query_map([session_id], |row| {
                    let event_id: i64 = row.get(0)?;
                    Ok((
                        event_id,
                        SessionMessage {
                            role: row.get(1)?,
                            content: row.get(2)?,
                            tool_call_id: row.get(3)?,
                            tool_name: row.get(4)?,
                            timestamp: parse_datetime(&row.get::<_, String>(5)?),
                            kind: row.get(6)?,
                            content_json: parse_json_opt(row.get(7)?),
                            metadata_json: parse_json_opt(row.get(8)?),
                        },
                    ))
                })
                .map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn update_session_event(&self, event_id: i64, content: &str) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let content = content.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            let updated = conn
                .execute(
                    "UPDATE session_events SET content = ?2 WHERE id = ?1",
                    params![event_id, content],
                )
                .map_err(sql_error)?;
            if updated == 0 {
                return Err(io::Error::new(io::ErrorKind::NotFound, "Evento nao encontrado"));
            }
            if table_exists(&conn, "session_events_fts") {
                let _ = conn.execute(
                    "UPDATE session_events_fts SET content = ?2 WHERE event_id = ?1",
                    params![event_id, content],
                );
            }
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn delete_session_event(&self, session_id: &str, event_id: i64) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute("DELETE FROM session_events WHERE id = ?1", [event_id])
                .map_err(sql_error)?;
            if table_exists(&conn, "session_events_fts") {
                let _ = conn.execute(
                    "DELETE FROM session_events_fts WHERE event_id = ?1",
                    [event_id],
                );
            }
            recompute_message_count(&conn, &session_id)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn truncate_session_after(
        &self,
        session_id: &str,
        event_id: i64,
    ) -> io::Result<usize> {
        let db_path = self.db_path.clone();
        let session_id = session_id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<usize> {
            let conn = open_connection(&db_path)?;
            let removed = conn
                .execute(
                    "DELETE FROM session_events WHERE session_id = ?1 AND id > ?2",
                    params![session_id, event_id],
                )
                .map_err(sql_error)?;
            if table_exists(&conn, "session_events_fts") {
                let _ = conn.execute(
                    "DELETE FROM session_events_fts WHERE session_id = ?1 AND CAST(event_id AS INTEGER) > ?2",
                    params![session_id, event_id],
                );
            }
            recompute_message_count(&conn, &session_id)?;
            Ok(removed)
        })
        .await
        .map_err(join_error)?
    }

    // ── Presets ──

    pub async fn upsert_preset(&self, preset: &Preset) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let preset = preset.clone();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute(
                r#"
                INSERT INTO presets (
                    id, name, description, provider_id, model_id, system_prompt,
                    temperature, max_tokens, top_p, prefix, suffix, tags_json,
                    favorite, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    description = excluded.description,
                    provider_id = excluded.provider_id,
                    model_id = excluded.model_id,
                    system_prompt = excluded.system_prompt,
                    temperature = excluded.temperature,
                    max_tokens = excluded.max_tokens,
                    top_p = excluded.top_p,
                    prefix = excluded.prefix,
                    suffix = excluded.suffix,
                    tags_json = excluded.tags_json,
                    favorite = excluded.favorite,
                    updated_at = excluded.updated_at
                "#,
                params![
                    preset.id,
                    preset.name,
                    preset.description,
                    preset.provider_id,
                    preset.model_id,
                    preset.system_prompt,
                    preset.temperature.map(|value| value as f64),
                    preset.max_tokens.map(|value| value as i64),
                    preset.top_p.map(|value| value as f64),
                    preset.prefix,
                    preset.suffix,
                    serde_json::to_string(&preset.tags).unwrap_or_else(|_| "[]".to_string()),
                    preset.favorite as i64,
                    preset.created_at.to_rfc3339(),
                    preset.updated_at.to_rfc3339(),
                ],
            )
            .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn list_presets(&self) -> io::Result<Vec<Preset>> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> io::Result<Vec<Preset>> {
            let conn = open_connection(&db_path)?;
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT id, name, description, provider_id, model_id, system_prompt,
                           temperature, max_tokens, top_p, prefix, suffix, tags_json,
                           favorite, created_at, updated_at
                    FROM presets
                    ORDER BY favorite DESC, updated_at DESC
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt.query_map([], row_to_preset).map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn get_preset(&self, id: &str) -> io::Result<Option<Preset>> {
        let db_path = self.db_path.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<Option<Preset>> {
            let conn = open_connection(&db_path)?;
            conn.query_row(
                r#"
                SELECT id, name, description, provider_id, model_id, system_prompt,
                       temperature, max_tokens, top_p, prefix, suffix, tags_json,
                       favorite, created_at, updated_at
                FROM presets
                WHERE id = ?1
                "#,
                [id],
                row_to_preset,
            )
            .optional()
            .map_err(sql_error)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn delete_preset(&self, id: &str) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute("DELETE FROM presets WHERE id = ?1", [id])
                .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    // ── Comparisons ──

    pub async fn upsert_comparison(&self, comparison: &Comparison) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let comparison = comparison.clone();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute(
                r#"
                INSERT INTO comparisons (
                    id, prompt, system_prompt, blind, entries_json, synthesis,
                    winner_label, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(id) DO UPDATE SET
                    prompt = excluded.prompt,
                    system_prompt = excluded.system_prompt,
                    blind = excluded.blind,
                    entries_json = excluded.entries_json,
                    synthesis = excluded.synthesis,
                    winner_label = excluded.winner_label
                "#,
                params![
                    comparison.id,
                    comparison.prompt,
                    comparison.system_prompt,
                    comparison.blind as i64,
                    serde_json::to_string(&comparison.entries).unwrap_or_else(|_| "[]".to_string()),
                    comparison.synthesis,
                    comparison.winner_label,
                    comparison.created_at.to_rfc3339(),
                ],
            )
            .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn list_comparisons(&self, limit: usize) -> io::Result<Vec<Comparison>> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> io::Result<Vec<Comparison>> {
            let conn = open_connection(&db_path)?;
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT id, prompt, system_prompt, blind, entries_json, synthesis,
                           winner_label, created_at
                    FROM comparisons
                    ORDER BY created_at DESC
                    LIMIT ?1
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt
                .query_map([limit.max(1) as i64], row_to_comparison)
                .map_err(sql_error)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(sql_error)?);
            }
            Ok(out)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn get_comparison(&self, id: &str) -> io::Result<Option<Comparison>> {
        let db_path = self.db_path.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<Option<Comparison>> {
            let conn = open_connection(&db_path)?;
            conn.query_row(
                r#"
                SELECT id, prompt, system_prompt, blind, entries_json, synthesis,
                       winner_label, created_at
                FROM comparisons
                WHERE id = ?1
                "#,
                [id],
                row_to_comparison,
            )
            .optional()
            .map_err(sql_error)
        })
        .await
        .map_err(join_error)?
    }

    pub async fn set_comparison_vote(&self, id: &str, winner_label: &str) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let id = id.to_string();
        let winner_label = winner_label.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute(
                "UPDATE comparisons SET winner_label = ?2 WHERE id = ?1",
                params![id, winner_label],
            )
            .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }

    pub async fn delete_comparison(&self, id: &str) -> io::Result<()> {
        let db_path = self.db_path.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> io::Result<()> {
            let conn = open_connection(&db_path)?;
            conn.execute("DELETE FROM comparisons WHERE id = ?1", [id])
                .map_err(sql_error)?;
            Ok(())
        })
        .await
        .map_err(join_error)?
    }
}

fn recompute_message_count(conn: &Connection, session_id: &str) -> io::Result<()> {
    conn.execute(
        r#"
        UPDATE sessions
        SET message_count = (
            SELECT COUNT(*) FROM session_events WHERE session_id = ?1
        ),
        updated_at = ?2
        WHERE id = ?1
        "#,
        params![session_id, Utc::now().to_rfc3339()],
    )
    .map_err(sql_error)?;
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> io::Result<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&pragma).map_err(sql_error)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sql_error)?;
    for row in rows {
        if row.map_err(sql_error)?.eq_ignore_ascii_case(column) {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )
    .map_err(sql_error)?;
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE name = ?1 LIMIT 1",
        [table],
        |_| Ok(()),
    )
    .is_ok()
}

fn fts_query_string(query: &str) -> String {
    let tokens = query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|token| token.trim())
        .filter(|token| token.len() >= 2)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        String::new()
    } else {
        tokens.join(" OR ")
    }
}

fn open_connection(path: &std::path::Path) -> io::Result<Connection> {
    Connection::open(path).map_err(sql_error)
}

fn sql_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(error.to_string())
}

fn join_error(error: tokio::task::JoinError) -> io::Error {
    io::Error::other(error.to_string())
}

fn parse_json_opt(raw: Option<String>) -> Option<serde_json::Value> {
    raw.and_then(|value| serde_json::from_str(&value).ok())
}

fn parse_tags(raw: Option<String>) -> Vec<String> {
    raw.and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

fn parse_metadata(raw: Option<String>) -> std::collections::BTreeMap<String, String> {
    raw.and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

fn parse_datetime(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_datetime_opt(raw: Option<String>) -> Option<DateTime<Utc>> {
    raw.as_deref().map(parse_datetime)
}

fn row_to_session_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionMeta> {
    Ok(SessionMeta {
        id: row.get(0)?,
        name: row.get(1)?,
        updated_at: parse_datetime(&row.get::<_, String>(2)?),
        last_activity_at: parse_datetime(&row.get::<_, String>(3)?),
        message_count: row.get::<_, i64>(4)?.max(0) as usize,
        provider_id: row.get(5)?,
        model_id: row.get(6)?,
        workspace_root: row.get(7)?,
        origin_kind: row.get(8)?,
        parent_session_id: row.get(9)?,
        status: row.get(10)?,
        created_at: parse_datetime(&row.get::<_, String>(11)?),
        summary: row.get(12).unwrap_or_default(),
        source_channel: row.get(13).unwrap_or_default(),
        thread_id: row.get(14).unwrap_or_default(),
        correlation_id: row.get(15).unwrap_or_default(),
        folder: row.get(16).unwrap_or_default(),
        archived: row.get::<_, i64>(17).unwrap_or(0) != 0,
        pinned: row.get::<_, i64>(18).unwrap_or(0) != 0,
    })
}

fn row_to_memory_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let embedding_blob: Option<Vec<u8>> = row.get(16).unwrap_or(None);
    let embedding_dim: i64 = row.get(17).unwrap_or(0);
    let embedding: Option<Vec<f32>> = embedding_blob
        .filter(|b| !b.is_empty())
        .map(|b| crate::embeddings::deserialize_embedding(&b));
    Ok(MemoryRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        source_session_id: row.get(2)?,
        scope: row.get(3)?,
        namespace: row.get(4)?,
        kind: row.get(5)?,
        title: row.get(6)?,
        content: row.get(7)?,
        tags: parse_tags(row.get(8)?),
        metadata: parse_metadata(row.get(9)?),
        importance: row.get::<_, i64>(10)? as i32,
        created_at: parse_datetime(&row.get::<_, String>(11)?),
        last_accessed_at: parse_datetime_opt(row.get(12)?),
        pin_state: row.get(13).unwrap_or_else(|_| "auto".to_string()),
        promotion_source: row.get(14).unwrap_or_default(),
        summary_ref: row.get(15).unwrap_or_default(),
        embedding,
        embedding_dim: embedding_dim as usize,
    })
}

fn row_to_preset(row: &rusqlite::Row<'_>) -> rusqlite::Result<Preset> {
    Ok(Preset {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2).unwrap_or_default(),
        provider_id: row.get(3).unwrap_or_default(),
        model_id: row.get(4).unwrap_or_default(),
        system_prompt: row.get(5).unwrap_or_default(),
        temperature: row.get::<_, Option<f64>>(6)?.map(|value| value as f32),
        max_tokens: row.get::<_, Option<i64>>(7)?.map(|value| value as u32),
        top_p: row.get::<_, Option<f64>>(8)?.map(|value| value as f32),
        prefix: row.get(9).unwrap_or_default(),
        suffix: row.get(10).unwrap_or_default(),
        tags: parse_tags(row.get(11)?),
        favorite: row.get::<_, i64>(12).unwrap_or(0) != 0,
        created_at: parse_datetime(&row.get::<_, String>(13)?),
        updated_at: parse_datetime(&row.get::<_, String>(14)?),
    })
}

fn row_to_comparison(row: &rusqlite::Row<'_>) -> rusqlite::Result<Comparison> {
    let entries_json: Option<String> = row.get(4)?;
    let entries: Vec<ComparisonEntry> = entries_json
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    Ok(Comparison {
        id: row.get(0)?,
        prompt: row.get(1)?,
        system_prompt: row.get(2).unwrap_or_default(),
        blind: row.get::<_, i64>(3).unwrap_or(1) != 0,
        entries,
        synthesis: row.get(5).unwrap_or_default(),
        winner_label: row.get(6).unwrap_or_default(),
        created_at: parse_datetime(&row.get::<_, String>(7)?),
    })
}

/// A single ordered, idempotent schema migration applied exactly once.
struct Migration {
    id: i64,
    name: &'static str,
    sql: &'static str,
}

/// Ordered list of versioned migrations recorded in `schema_migrations`.
/// Each `sql` block must be idempotent (CREATE ... IF NOT EXISTS) so a partially
/// migrated database can re-run safely.
const MIGRATIONS: &[Migration] = &[
    Migration {
        id: 1,
        name: "wave1_presets",
        sql: r#"
            CREATE TABLE IF NOT EXISTS presets (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                provider_id TEXT NOT NULL DEFAULT '',
                model_id TEXT NOT NULL DEFAULT '',
                system_prompt TEXT NOT NULL DEFAULT '',
                temperature REAL,
                max_tokens INTEGER,
                top_p REAL,
                prefix TEXT NOT NULL DEFAULT '',
                suffix TEXT NOT NULL DEFAULT '',
                tags_json TEXT NOT NULL DEFAULT '[]',
                favorite INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        "#,
    },
    Migration {
        id: 2,
        name: "wave1_comparisons",
        sql: r#"
            CREATE TABLE IF NOT EXISTS comparisons (
                id TEXT PRIMARY KEY,
                prompt TEXT NOT NULL,
                system_prompt TEXT NOT NULL DEFAULT '',
                blind INTEGER NOT NULL DEFAULT 1,
                entries_json TEXT NOT NULL DEFAULT '[]',
                synthesis TEXT NOT NULL DEFAULT '',
                winner_label TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_comparisons_created
            ON comparisons(created_at DESC);
        "#,
    },
];

/// Apply any not-yet-recorded migrations from [`MIGRATIONS`] in order, tracking
/// applied ids in `schema_migrations`. This is the forward-looking mechanism for
/// non-additive schema changes (additive columns still use [`ensure_column`]).
fn run_migrations(conn: &Connection) -> io::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )
    .map_err(sql_error)?;
    for migration in MIGRATIONS {
        let already_applied = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1",
                [migration.id],
                |_| Ok(()),
            )
            .optional()
            .map_err(sql_error)?
            .is_some();
        if already_applied {
            continue;
        }
        conn.execute_batch(migration.sql).map_err(sql_error)?;
        conn.execute(
            "INSERT INTO schema_migrations (id, name, applied_at) VALUES (?1, ?2, ?3)",
            params![migration.id, migration.name, Utc::now().to_rfc3339()],
        )
        .map_err(sql_error)?;
    }
    Ok(())
}
