//! SQLite-backed persistent state for sessions and memory.

use crate::memory::MemoryRecord;
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
                    message_count INTEGER NOT NULL DEFAULT 0
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
                    summary_ref TEXT NOT NULL DEFAULT ''
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

            let _ = conn.execute_batch(
                r#"
                CREATE VIRTUAL TABLE IF NOT EXISTS session_events_fts
                USING fts5(event_id UNINDEXED, session_id UNINDEXED, content);

                CREATE VIRTUAL TABLE IF NOT EXISTS memory_records_fts
                USING fts5(record_id UNINDEXED, title, content);
                "#,
            );
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
                    summary, source_channel, thread_id, correlation_id, message_count
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
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
                    message_count = excluded.message_count
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
                       source_channel, thread_id, correlation_id
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
                           source_channel, thread_id, correlation_id
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
                           COALESCE(GROUP_CONCAT(e.content, ' '), '')
                    FROM sessions s
                    LEFT JOIN session_events e ON e.session_id = s.id
                    GROUP BY s.id, s.name, s.updated_at, s.last_activity_at, s.message_count,
                             s.provider_id, s.model_id, s.workspace_root, s.origin_kind,
                             s.parent_session_id, s.status, s.created_at, s.summary,
                             s.source_channel, s.thread_id, s.correlation_id
                    ORDER BY s.updated_at DESC
                    "#,
                )
                .map_err(sql_error)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(SessionSearchCandidate {
                        meta: row_to_session_meta(row)?,
                        transcript: row.get::<_, String>(16)?,
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
                        transcript: row.get::<_, String>(16)?,
                        preview: row.get::<_, String>(17).unwrap_or_default(),
                        raw_score: row.get::<_, i64>(18).unwrap_or_default(),
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
                tx.execute(
                    r#"
                    INSERT INTO memory_records (
                        id, session_id, source_session_id, scope, namespace, kind, title, content,
                        tags_json, metadata_json, importance, created_at, last_accessed_at,
                        pin_state, promotion_source, summary_ref
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
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
                        summary_ref = excluded.summary_ref
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
                           pin_state, promotion_source, summary_ref
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
                           pin_state, promotion_source, summary_ref
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
                           m.summary_ref,
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
                        row.get::<_, String>(16).unwrap_or_default(),
                        row.get::<_, i64>(17).unwrap_or_default(),
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
    })
}

fn row_to_memory_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
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
    })
}
