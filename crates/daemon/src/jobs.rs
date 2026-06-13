//! Background job registry, SSE progress streaming, and scheduled-task engine.
//!
//! Provides:
//! - `JobRegistry` — spawn async tasks with progress reporting, cancellation, and
//!   a concurrency cap.
//! - SSE helpers — `/jobs/{id}/stream` progress endpoint and `/jobs/{id}/cancel`.
//! - `Scheduler` — background loop that reads `scheduled_tasks` from SQLite,
//!   evaluates cron/once triggers, and spawns jobs via the registry.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::Sse;
use axum::Json;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tokio_stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{broadcast, RwLock, Semaphore};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

// ── Core types ──────────────────────────────────────────────────────────────

pub type JobId = String;

/// Lifecycle status of a background job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Error,
    Cancelled,
}

/// Progress snapshot emitted during a job's lifetime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    pub job_id: JobId,
    pub percent: u8,
    pub phase: String,
    pub message: String,
}

/// Full record of a job (status + progress + timestamps).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: JobId,
    pub kind: String,
    pub status: JobStatus,
    pub progress: Option<JobProgress>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub result: Option<Value>,
}

// ── In-memory job registry ──────────────────────────────────────────────────

/// In-memory handle for a running job.
struct JobHandle {
    token: CancellationToken,
    join_handle: RwLock<Option<JoinHandle<()>>>,
    progress_tx: broadcast::Sender<JobProgress>,
    record: RwLock<JobRecord>,
}

/// Shared job registry — the single source of truth for background work.
pub struct JobRegistry {
    inner: Arc<RwLock<HashMap<JobId, Arc<JobHandle>>>>,
    sem: Arc<Semaphore>,
    _max_concurrent: usize,
}

impl JobRegistry {
    /// Create a new registry.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            sem: Arc::new(Semaphore::new(max_concurrent.max(1))),
            _max_concurrent: max_concurrent.max(1),
        }
    }

    /// Spawn a background job.
    ///
    /// `kind` is an opaque label (e.g. `"deep_research"`, `"reindex"`).
    /// `work` is an async block that receives a `JobCtx` (cancellation token +
    /// progress sender). The registry manages the semaphore, the join handle,
    /// and lifecycle transitions automatically.
    pub async fn spawn<F, Fut>(self: &Arc<Self>, kind: &str, work: F) -> JobId
    where
        F: FnOnce(JobCtx) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<Value, String>> + Send,
    {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        let (progress_tx, _) = broadcast::channel(32);
        let token = CancellationToken::new();

        let record = JobRecord {
            id: id.clone(),
            kind: kind.to_string(),
            status: JobStatus::Queued,
            progress: None,
            created_at: now,
            started_at: None,
            finished_at: None,
            error: None,
            result: None,
        };

        let handle = Arc::new(JobHandle {
            token: token.clone(),
            join_handle: RwLock::new(None),
            progress_tx: progress_tx.clone(),
            record: RwLock::new(record),
        });

        // Register immediately so the caller can observe "queued".
        {
            let mut inner = self.inner.write().await;
            inner.insert(id.clone(), handle.clone());
        }

        let sem = self.sem.clone();
        let job_id = id.clone();
        let handle_clone = handle.clone();
        let this = self.clone();

        let real_join = tokio::spawn(async move {
            // Acquire semaphore permit (respects concurrency cap).
            let _permit = sem.acquire().await;

            // Check if cancelled before really starting.
            if token.is_cancelled() {
                let mut rec = handle_clone.record.write().await;
                rec.status = JobStatus::Cancelled;
                rec.finished_at = Some(Utc::now());
                rec.error = Some("Cancelled before start".to_string());
                return;
            }

            // Mark as running.
            {
                let mut rec = handle_clone.record.write().await;
                rec.status = JobStatus::Running;
                rec.started_at = Some(Utc::now());
            }
            let _ = progress_tx.send(JobProgress {
                job_id: job_id.clone(),
                percent: 0,
                phase: "starting".to_string(),
                message: "Job started".to_string(),
            });

            let ctx = JobCtx {
                token: token.clone(),
                progress_tx: progress_tx.clone(),
                job_id: job_id.clone(),
            };

            match work(ctx).await {
                Ok(result) => {
                    let mut rec = handle_clone.record.write().await;
                    rec.status = JobStatus::Done;
                    rec.finished_at = Some(Utc::now());
                    rec.result = Some(result);
                    let _ = progress_tx.send(JobProgress {
                        job_id: job_id.clone(),
                        percent: 100,
                        phase: "done".to_string(),
                        message: "Job completed".to_string(),
                    });
                }
                Err(error) => {
                    let mut rec = handle_clone.record.write().await;
                    let is_cancelled = token.is_cancelled();
                    rec.status = if is_cancelled {
                        JobStatus::Cancelled
                    } else {
                        JobStatus::Error
                    };
                    rec.finished_at = Some(Utc::now());
                    rec.error = Some(error.clone());
                    let _ = progress_tx.send(JobProgress {
                        job_id: job_id.clone(),
                        percent: 0,
                        phase: if is_cancelled {
                            "cancelled"
                        } else {
                            "error"
                        }
                        .to_string(),
                        message: if is_cancelled {
                            "Job cancelled".to_string()
                        } else {
                            error
                        },
                    });
                }
            }

            // Prune old finished jobs.
            this.prune(Duration::from_secs(3600)).await;
        });

        // Store the real join handle.
        {
            let mut join_handle_guard = handle.join_handle.write().await;
            *join_handle_guard = Some(real_join);
        }

        id
    }

    /// Get the current record for a job.
    pub async fn get(&self, id: &str) -> Option<JobRecord> {
        let inner = self.inner.read().await;
        match inner.get(id) {
            Some(h) => {
                let rec = h.record.read().await;
                Some(rec.clone())
            }
            None => None,
        }
    }

    /// Cancel a job by id. Returns the current record if the job was cancellable.
    pub async fn cancel(&self, id: &str) -> Option<JobRecord> {
        let inner = self.inner.read().await;
        if let Some(handle) = inner.get(id) {
            handle.token.cancel();
            let rec = handle.record.read().await;
            if matches!(rec.status, JobStatus::Queued | JobStatus::Running) {
                Some(rec.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// List all known jobs (active + recently finished).
    pub async fn list(&self) -> Vec<JobRecord> {
        let inner = self.inner.read().await;
        let handles: Vec<_> = inner.values().cloned().collect();
        let mut records = Vec::with_capacity(handles.len());
        for h in handles {
            let rec = h.record.read().await;
            records.push(rec.clone());
        }
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        records
    }

    /// Subscribe to progress events for a job.
    pub async fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<JobProgress>> {
        let inner = self.inner.read().await;
        inner.get(id).map(|h| h.progress_tx.subscribe())
    }

    /// Prune finished jobs older than `max_age`.
    pub async fn prune(&self, max_age: Duration) {
        let now = Utc::now();
        let mut inner = self.inner.write().await;
        let keys: Vec<String> = inner.keys().cloned().collect();
        let mut to_remove = Vec::new();
        for id in &keys {
            if let Some(h) = inner.get(id) {
                let rec = h.record.read().await;
                match rec.status {
                    JobStatus::Done | JobStatus::Error | JobStatus::Cancelled => {
                        if let Some(finished) = rec.finished_at {
                            let age = (now - finished).num_seconds().max(0) as u64;
                            if age >= max_age.as_secs() {
                                to_remove.push(id.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for id in to_remove {
            inner.remove(&id);
        }
    }
}

/// Context handed to every background job.
#[derive(Clone)]
pub struct JobCtx {
    pub token: CancellationToken,
    pub progress_tx: broadcast::Sender<JobProgress>,
    pub job_id: JobId,
}

impl JobCtx {
    /// Report progress (0-100).
    pub fn progress(&self, percent: u8, phase: &str, message: &str) {
        let _ = self.progress_tx.send(JobProgress {
            job_id: self.job_id.clone(),
            percent: percent.min(100),
            phase: phase.to_string(),
            message: message.to_string(),
        });
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

// ── SSE endpoint handlers ───────────────────────────────────────────────────

/// Axum handler that streams job progress as Server-Sent Events.
pub async fn job_stream(
    State(state): State<crate::AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Sse<impl Stream<Item = Result<axum::response::sse::Event, io::Error>>>, StatusCode> {
    let rx = state.jobs.subscribe(&job_id).await.ok_or(StatusCode::NOT_FOUND)?;

    let stream = BroadcastStream::new(rx).map(|result| match result {
        Ok(progress) => {
            let json = serde_json::to_string(&progress).unwrap_or_default();
            Ok(axum::response::sse::Event::default().data(json))
        }
        Err(_) => Ok(axum::response::sse::Event::default().comment("stream ended")),
    });

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

/// Axum handler to request cancellation of a job.
pub async fn job_cancel(
    State(state): State<crate::AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<JobRecord>, StatusCode> {
    state
        .jobs
        .cancel(&job_id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// Axum handler to list all jobs.
pub async fn job_list(
    State(state): State<crate::AppState>,
) -> Json<Vec<JobRecord>> {
    Json(state.jobs.list().await)
}

/// Axum handler to get a single job.
pub async fn job_get(
    State(state): State<crate::AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<JobRecord>, StatusCode> {
    state
        .jobs
        .get(&job_id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

// ── Scheduled task persistent model ─────────────────────────────────────────

/// A scheduled task stored in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    /// `once` | `interval` | `cron`
    pub schedule_kind: String,
    /// Cron expression (for `cron` kind).
    pub cron_expr: Option<String>,
    /// Interval in seconds (for `interval` kind).
    pub interval_secs: Option<i64>,
    /// ISO-8601 datetime for one-shot execution.
    pub run_at: Option<DateTime<Utc>>,
    /// The job kind to spawn (e.g. `"deep_research"`, `"reindex"`).
    pub job_kind: String,
    /// JSON payload passed to the job.
    pub payload_json: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
}

/// A record marking that a scheduled task ran.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRun {
    pub id: i64,
    pub task_id: String,
    pub job_id: Option<String>,
    pub status: String, // "running" | "success" | "error" | "cancelled"
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

// ── Smoke test job ──────────────────────────────────────────────────────────

/// Spawn a test job with 5 steps that reports incremental progress.
/// Used for smoke-testing the SSE and cancel flow.
pub async fn spawn_test_job(state: &crate::AppState) -> JobId {
    state.jobs.spawn("test_dummy", |ctx| async move {
        for i in 0..5 {
            if ctx.is_cancelled() {
                return Err("Cancelled at checkpoint".to_string());
            }
            let percent = ((i + 1) * 20) as u8;
            let phase = format!("step_{}", i + 1);
            let msg = format!("Running step {}/5 — checkpoint {}", i + 1, i + 1);
            ctx.progress(percent, &phase, &msg);
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        Ok(serde_json::json!({"steps_completed": 5, "status": "success"}))
    })
    .await
}

/// Axum handler: POST /jobs/test — creates a dummy 5-step job.
pub async fn job_test(
    State(state): State<crate::AppState>,
) -> Json<JobRecord> {
    let job_id = spawn_test_job(&state).await;
    let record = state.jobs.get(&job_id).await.unwrap();
    Json(record)
}

/// Request body for creating/updating a scheduled task.
#[derive(Debug, Deserialize)]
pub struct ScheduledTaskRequest {
    pub name: String,
    pub schedule_kind: String,
    #[serde(default)]
    pub cron_expr: Option<String>,
    #[serde(default)]
    pub interval_secs: Option<i64>,
    #[serde(default)]
    pub run_at: Option<String>,
    #[serde(default)]
    pub job_kind: Option<String>,
    #[serde(default)]
    pub payload_json: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

// ── Scheduler ───────────────────────────────────────────────────────────────

/// Background scheduler that polls SQLite for due tasks and spawns them via the
/// JobRegistry.
pub struct Scheduler {
    db_path: PathBuf,
    registry: Arc<JobRegistry>,
}

impl Scheduler {
    pub fn new(db_path: PathBuf, registry: Arc<JobRegistry>) -> Self {
        Self { db_path, registry }
    }

    /// Start the scheduler loop. Runs until the CancellationToken is triggered.
    pub fn start(self, shutdown: CancellationToken) -> JoinHandle<()> {
        tokio::spawn(async move {
            // Wait a bit before first tick to let the system stabilize.
            tokio::time::sleep(Duration::from_secs(5)).await;
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            info!("scheduler started (30s tick)");

            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        info!("scheduler shutting down");
                        break;
                    }
                    _ = interval.tick() => {
                        if let Err(e) = self.tick().await {
                            warn!("scheduler tick error: {e}");
                        }
                    }
                }
            }
        })
    }

    async fn tick(&self) -> Result<(), String> {
        let tasks = load_due_tasks(&self.db_path).await.map_err(|e| e.to_string())?;
        if tasks.is_empty() {
            return Ok(());
        }

        debug!("scheduler found {} due task(s)", tasks.len());
        for task in tasks {
            if !task.enabled {
                continue;
            }
            let registry = self.registry.clone();
            let db_path = self.db_path.clone();
            let task_id = task.id.clone();

            let job_id = registry
                .spawn(&task.job_kind, move |ctx| {
                    let db_path = db_path.clone();
                    let task_id = task_id.clone();
                    async move {
                        ctx.progress(10, "running", "Executing scheduled task");
                        let run_id = record_run_start(&db_path, &task_id).await;

                        // The actual work is driven by the job_kind.
                        // Here we just mark it as completed.
                        let result = serde_json::json!({"scheduled": true, "task_id": task_id});

                        record_run_finish(&db_path, run_id, "success", None).await;
                        ctx.progress(100, "done", "Scheduled task completed");
                        Ok(result)
                    }
                })
                .await;

            info!("scheduler spawned job {job_id}");
        }

        Ok(())
    }
}

// ── SQLite helpers (private to this module) ─────────────────────────────────

fn open_sqlite(db_path: &Path) -> Result<Connection, io::Error> {
    let conn = Connection::open(db_path).map_err(|e| {
        io::Error::other(format!("Failed to open SQLite at {}: {e}", db_path.display()))
    })?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        .map_err(|e| io::Error::other(format!("Failed to set pragmas: {e}")))?;
    Ok(conn)
}

fn sql_error(e: rusqlite::Error) -> io::Error {
    io::Error::other(format!("SQLite error: {e}"))
}

/// Ensure scheduler tables exist in the given database.
pub async fn ensure_scheduler_tables(db_path: &Path) -> io::Result<()> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS scheduled_tasks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                schedule_kind TEXT NOT NULL DEFAULT 'once',
                cron_expr TEXT,
                interval_secs INTEGER,
                run_at TEXT,
                job_kind TEXT NOT NULL DEFAULT 'generic',
                payload_json TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                last_run_at TEXT
            );

            CREATE TABLE IF NOT EXISTS task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                job_id TEXT,
                status TEXT NOT NULL DEFAULT 'queued',
                started_at TEXT NOT NULL,
                finished_at TEXT,
                error TEXT,
                FOREIGN KEY(task_id) REFERENCES scheduled_tasks(id)
            );

            CREATE INDEX IF NOT EXISTS idx_task_runs_task_id ON task_runs(task_id);
            "#,
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .await
    .map_err(|e| io::Error::other(e.to_string()))?
}

fn row_to_scheduled_task(row: &rusqlite::Row) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get(0)?,
        name: row.get(1)?,
        schedule_kind: row.get(2)?,
        cron_expr: row.get(3)?,
        interval_secs: row.get(4)?,
        run_at: row
            .get::<_, Option<String>>(5)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
        job_kind: row.get(6)?,
        payload_json: row.get(7)?,
        enabled: row.get::<_, bool>(8).unwrap_or(true),
        created_at: row
            .get::<_, Option<String>>(9)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)))
            .unwrap_or_else(Utc::now),
        last_run_at: row
            .get::<_, Option<String>>(10)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
    })
}

// ── Scheduler CRUD endpoint handlers ───────────────────────────────────────

pub async fn scheduler_list_tasks(
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<ScheduledTask>>, StatusCode> {
    list_scheduled_tasks(&state.state_db_path)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn scheduler_create_task(
    State(state): State<crate::AppState>,
    Json(req): Json<ScheduledTaskRequest>,
) -> Result<Json<ScheduledTask>, StatusCode> {
    let now = Utc::now();
    let task = ScheduledTask {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name,
        schedule_kind: req.schedule_kind,
        cron_expr: req.cron_expr,
        interval_secs: req.interval_secs,
        run_at: req.run_at.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
        job_kind: req.job_kind.unwrap_or_else(|| "generic".to_string()),
        payload_json: req.payload_json,
        enabled: req.enabled,
        created_at: now,
        last_run_at: None,
    };

    upsert_scheduled_task(&state.state_db_path, &task)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(task))
}

pub async fn scheduler_delete_task(
    State(state): State<crate::AppState>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    delete_scheduled_task(&state.state_db_path, &task_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn scheduler_task_runs(
    State(state): State<crate::AppState>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<Vec<TaskRun>>, StatusCode> {
    list_task_runs(&state.state_db_path, &task_id, 50)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn load_due_tasks(db_path: &Path) -> Result<Vec<ScheduledTask>, io::Error> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, schedule_kind, cron_expr, interval_secs, run_at,
                        job_kind, payload_json, enabled, created_at, last_run_at
                 FROM scheduled_tasks
                 WHERE enabled = 1",
            )
            .map_err(sql_error)?;

        let rows = stmt
            .query_map([], |row| row_to_scheduled_task(row))
            .map_err(sql_error)?;

        let mut tasks = Vec::new();
        for row in rows {
            let task = row.map_err(sql_error)?;
            if is_task_due(&task) {
                tasks.push(task);
            }
        }
        Ok(tasks)
    })
    .await
    .map_err(|e| io::Error::other(e.to_string()))?
}

async fn record_run_start(db_path: &Path, task_id: &str) -> i64 {
    let db_path = db_path.to_path_buf();
    let task_id = task_id.to_string();
    match tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO task_runs (task_id, status, started_at) VALUES (?1, 'running', ?2)",
            params![task_id, now],
        )
        .map_err(sql_error)?;
        Ok::<i64, io::Error>(conn.last_insert_rowid())
    })
    .await
    {
        Ok(Ok(id)) => id,
        _ => 0,
    }
}

async fn record_run_finish(db_path: &Path, run_id: i64, status: &str, error: Option<&str>) {
    let db_path = db_path.to_path_buf();
    let status = status.to_string();
    let error = error.map(|s| s.to_string());
    let _ = tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE task_runs SET status = ?1, finished_at = ?2, error = ?3 WHERE id = ?4",
            params![status, now, error, run_id],
        )
        .map_err(sql_error)?;
        Ok::<_, io::Error>(())
    })
    .await;
}

async fn disable_task(db_path: &Path, task_id: &str) {
    let db_path = db_path.to_path_buf();
    let task_id = task_id.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        conn.execute(
            "UPDATE scheduled_tasks SET enabled = 0 WHERE id = ?1",
            params![task_id],
        )
        .map_err(sql_error)?;
        Ok::<_, io::Error>(())
    })
    .await;
}

async fn touch_last_run(db_path: &Path, task_id: &str) {
    let db_path = db_path.to_path_buf();
    let task_id = task_id.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE scheduled_tasks SET last_run_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )
        .map_err(sql_error)?;
        Ok::<_, io::Error>(())
    })
    .await;
}

fn is_task_due(task: &ScheduledTask) -> bool {
    let now = Utc::now();
    match task.schedule_kind.as_str() {
        "once" => {
            if let Some(run_at) = task.run_at {
                run_at <= now && task.last_run_at.is_none()
            } else {
                false
            }
        }
        "interval" => {
            if let Some(interval_secs) = task.interval_secs {
                let interval = chrono::Duration::seconds(interval_secs);
                match task.last_run_at {
                    Some(last) => last + interval <= now,
                    None => true,
                }
            } else {
                false
            }
        }
        "cron" => {
            if let Some(cron_expr) = &task.cron_expr {
                if let Ok(schedule) = cron_expr.parse::<cron::Schedule>() {
                    if let Some(last) = task.last_run_at {
                        for fire_time in schedule.upcoming(Utc).take(5) {
                            if fire_time > last && fire_time <= now {
                                return true;
                            }
                        }
                        false
                    } else {
                        // Never run — check if a fire time is within the last tick window.
                        for fire_time in schedule.upcoming(Utc).take(3) {
                            let diff = (fire_time - now).num_seconds().abs();
                            if diff <= 60 {
                                return true;
                            }
                        }
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

// ── Public persistent CRUD (used by Axum handlers) ─────────────────────────

pub async fn list_scheduled_tasks(db_path: &Path) -> io::Result<Vec<ScheduledTask>> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, schedule_kind, cron_expr, interval_secs, run_at,
                        job_kind, payload_json, enabled, created_at, last_run_at
                 FROM scheduled_tasks
                 ORDER BY created_at DESC",
            )
            .map_err(sql_error)?;

        let rows = stmt
            .query_map([], |row| row_to_scheduled_task(row))
            .map_err(sql_error)?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(sql_error)?);
        }
        Ok(tasks)
    })
    .await
    .map_err(|e| io::Error::other(e.to_string()))?
}

pub async fn upsert_scheduled_task(
    db_path: &Path,
    task: &ScheduledTask,
) -> io::Result<()> {
    let db_path = db_path.to_path_buf();
    let task = task.clone();
    tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        conn.execute(
            "INSERT OR REPLACE INTO scheduled_tasks
             (id, name, schedule_kind, cron_expr, interval_secs, run_at,
              job_kind, payload_json, enabled, created_at, last_run_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                task.id,
                task.name,
                task.schedule_kind,
                task.cron_expr,
                task.interval_secs,
                task.run_at.map(|dt| dt.to_rfc3339()),
                task.job_kind,
                task.payload_json,
                task.enabled as i32,
                task.created_at.to_rfc3339(),
                task.last_run_at.map(|dt| dt.to_rfc3339()),
            ],
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .await
    .map_err(|e| io::Error::other(e.to_string()))?
}

pub async fn delete_scheduled_task(db_path: &Path, id: &str) -> io::Result<()> {
    let db_path = db_path.to_path_buf();
    let id = id.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        conn.execute(
            "DELETE FROM scheduled_tasks WHERE id = ?1",
            params![id],
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .await
    .map_err(|e| io::Error::other(e.to_string()))?
}

pub async fn list_task_runs(
    db_path: &Path,
    task_id: &str,
    limit: usize,
) -> io::Result<Vec<TaskRun>> {
    let db_path = db_path.to_path_buf();
    let task_id = task_id.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, job_id, status, started_at, finished_at, error
                 FROM task_runs
                 WHERE task_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(sql_error)?;

        let rows = stmt
            .query_map(params![task_id, limit as i64], |row| {
                Ok(TaskRun {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    job_id: row.get(2)?,
                    status: row.get::<_, String>(3).unwrap_or_else(|_| "unknown".to_string()),
                    started_at: row
                        .get::<_, String>(4)
                        .ok()
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)))
                        .unwrap_or_else(Utc::now),
                    finished_at: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))),
                    error: row.get(6)?,
                })
            })
            .map_err(sql_error)?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row.map_err(sql_error)?);
        }
        Ok(runs)
    })
    .await
    .map_err(|e| io::Error::other(e.to_string()))?
}
