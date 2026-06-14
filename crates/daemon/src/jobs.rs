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
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::Sse;
use axum::Json;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{broadcast, RwLock, Semaphore};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
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
                        phase: if is_cancelled { "cancelled" } else { "error" }.to_string(),
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
        records.sort_by_key(|b| std::cmp::Reverse(b.created_at));
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

// ── Clock abstraction for deterministic scheduling ─────────────────────────

/// Injectable clock for deterministic tests.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Production clock — delegates to [`Utc::now`].
pub struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

// ── Action dispatcher ──────────────────────────────────────────────────────

/// Type-erased, boxed future returned by an [`ActionHandler`].
pub type ActionFuture = Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send>>;

/// A registered action handler: receives a JSON payload and a [`JobCtx`],
/// returns a result.
pub type ActionHandler =
    Arc<dyn Fn(Value, JobCtx) -> ActionFuture + Send + Sync>;

/// Extensible registry of [`ActionHandler`]s keyed by `job_kind`.
///
/// Each `job_kind` string (e.g. `"deep_research"`, `"reindex"`) maps to
/// a handler that receives the task's `payload_json` and a [`JobCtx`] for
/// progress reporting and cancellation.
pub struct ActionDispatcher {
    handlers: RwLock<HashMap<String, ActionHandler>>,
}

impl ActionDispatcher {
    /// Create an empty dispatcher.
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// Register (or replace) a handler for `kind`.
    pub async fn register(&self, kind: &str, handler: ActionHandler) {
        let mut handlers = self.handlers.write().await;
        handlers.insert(kind.to_string(), handler);
    }

    /// Execute the handler registered for `kind` with the given `payload` and `ctx`.
    ///
    /// Returns `Err` if no handler is registered for `kind`.
    pub async fn dispatch(
        &self,
        kind: &str,
        payload: Value,
        ctx: JobCtx,
    ) -> Result<Value, String> {
        let handlers = self.handlers.read().await;
        if let Some(handler) = handlers.get(kind) {
            handler(payload, ctx).await
        } else {
            Err(format!("no handler registered for job_kind: {kind}"))
        }
    }
}

// ── SSE endpoint handlers ───────────────────────────────────────────────────

/// Axum handler that streams job progress as Server-Sent Events.
pub async fn job_stream(
    State(state): State<crate::AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Sse<impl Stream<Item = Result<axum::response::sse::Event, io::Error>>>, StatusCode> {
    let rx = state
        .jobs
        .subscribe(&job_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

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
pub async fn job_list(State(state): State<crate::AppState>) -> Json<Vec<JobRecord>> {
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
    state
        .jobs
        .spawn("test_dummy", |ctx| async move {
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
pub async fn job_test(State(state): State<crate::AppState>) -> Json<JobRecord> {
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

/// Background scheduler that polls SQLite for due tasks, atomically claims
/// them to prevent duplicates, and dispatches each `job_kind` to the
/// appropriate [`ActionHandler`].
pub struct Scheduler {
    db_path: PathBuf,
    registry: Arc<JobRegistry>,
    dispatcher: Arc<ActionDispatcher>,
    clock: Arc<dyn Clock>,
}

impl Scheduler {
    pub fn new(
        db_path: PathBuf,
        registry: Arc<JobRegistry>,
        dispatcher: Arc<ActionDispatcher>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            db_path,
            registry,
            dispatcher,
            clock,
        }
    }

    /// Start the scheduler loop. Runs recovery first, then ticks on a periodic
    /// interval until `shutdown` is triggered. When shutting down, cancels all
    /// active jobs cooperatively.
    pub fn start(self, shutdown: CancellationToken) -> JoinHandle<()> {
        tokio::spawn(async move {
            // Short initial delay so the system stabilises.
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Recovery: mark any task_run rows still "running" as errored.
            if let Err(e) = self.recover_stale_runs().await {
                warn!("scheduler recovery error: {e}");
            }

            let mut interval = tokio::time::interval(Duration::from_secs(30));
            info!("scheduler started (30s tick)");

            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        info!("scheduler shutting down — cancelling active jobs");
                        self.cancel_all_active().await;
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

    /// Mark any `task_runs` still in `"running"` status as errored (daemon
    /// restart).
    async fn recover_stale_runs(&self) -> Result<(), String> {
        let db_path = self.db_path.clone();
        let now = self.clock.now();
        let affected = tokio::task::spawn_blocking(move || -> Result<usize, io::Error> {
            let conn = open_sqlite(&db_path)?;
            let now_str = now.to_rfc3339();
            let n = conn
                .execute(
                    "UPDATE task_runs SET status = 'error', error = 'daemon restarted',
                     finished_at = ?1 WHERE status = 'running'",
                    params![now_str],
                )
                .map_err(sql_error)?;
            Ok(n)
        })
        .await
        .map_err(|e| format!("recover_stale_runs join error: {e}"))?
        .map_err(|e| format!("recover_stale_runs: {e}"))?;
        if affected > 0 {
            info!("scheduler marked {affected} stale task_run(s) as error");
        }
        Ok(())
    }

    /// Cancel every active (queued or running) job in the registry.
    async fn cancel_all_active(&self) {
        let records = self.registry.list().await;
        for r in &records {
            if matches!(r.status, JobStatus::Queued | JobStatus::Running) {
                self.registry.cancel(&r.id).await;
            }
        }
    }

    /// Single scheduler tick: claim due tasks atomically, then spawn a job
    /// for each.
    async fn tick(&self) -> Result<(), String> {
        let now = self.clock.now();

        // Atomically claim due tasks in a transaction.
        let claimed = claim_due_tasks(&self.db_path, now)
            .await
            .map_err(|e| e.to_string())?;

        if claimed.is_empty() {
            return Ok(());
        }

        debug!("scheduler claimed {} due task(s)", claimed.len());
        for task in claimed {
            let registry = self.registry.clone();
            let dispatcher = self.dispatcher.clone();
            let db_path = self.db_path.clone();
            let clock = self.clock.clone();
            let task_id = task.id.clone();
            let job_kind = task.job_kind.clone();
            let is_once = task.schedule_kind == "once";

            // Parse payload from payload_json; default to Null.
            let payload: Value = task
                .payload_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(Value::Null);

            let job_kind_for_spawn = job_kind.clone();
            let task_id_for_log = task_id.clone();
            let job_id = registry
                .spawn(&job_kind_for_spawn, move |ctx| {
                    let db_path = db_path.clone();
                    let task_id = task_id.clone();
                    let dispatcher = dispatcher.clone();
                    let job_kind = job_kind.clone();
                    let payload = payload.clone();
                    let clock = clock.clone();
                    async move {
                        // Record the run start with the actual job_id.
                        let run_id = record_run_start(&db_path, &task_id, &ctx.job_id).await;

                        ctx.progress(10, "running", &format!("Executing {job_kind}"));

                        // Dispatch to the real action handler.
                        let result = dispatcher.dispatch(&job_kind, payload, ctx).await;

                        // Persist completion: status, last_run_at, and maybe disable.
                        let now = clock.now();
                        match &result {
                            Ok(_) => {
                                complete_scheduled_run(
                                    &db_path, &task_id, run_id, "success",
                                    None, now, is_once,
                                )
                                .await;
                            }
                            Err(e) => {
                                complete_scheduled_run(
                                    &db_path, &task_id, run_id, "error",
                                    Some(e), now, is_once,
                                )
                                .await;
                            }
                        }

                        result
                    }
                })
                .await;

            info!("scheduler spawned job {job_id} for task {task_id_for_log}");
        }

        Ok(())
    }
}

// ── SQLite helpers (private to this module) ─────────────────────────────────

fn open_sqlite(db_path: &Path) -> Result<Connection, io::Error> {
    let conn = Connection::open(db_path).map_err(|e| {
        io::Error::other(format!(
            "Failed to open SQLite at {}: {e}",
            db_path.display()
        ))
    })?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        .map_err(|e| io::Error::other(format!("Failed to set pragmas: {e}")))?;
    Ok(conn)
}

fn sql_error(e: rusqlite::Error) -> io::Error {
    io::Error::other(format!("SQLite error: {e}"))
}

// Tables are now created via the versioned MIGRATIONS mechanism in
// crates/agent-core/src/state_store.rs (Migration id 3: "wave2_scheduler").
// The ensure_scheduler_tables function has been removed.

fn row_to_scheduled_task(row: &rusqlite::Row) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get(0)?,
        name: row.get(1)?,
        schedule_kind: row.get(2)?,
        cron_expr: row.get(3)?,
        interval_secs: row.get(4)?,
        run_at: row.get::<_, Option<String>>(5)?.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }),
        job_kind: row.get(6)?,
        payload_json: row.get(7)?,
        enabled: row.get::<_, bool>(8).unwrap_or(true),
        created_at: row
            .get::<_, Option<String>>(9)?
            .and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            })
            .unwrap_or_else(Utc::now),
        last_run_at: row.get::<_, Option<String>>(10)?.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }),
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
        run_at: req.run_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }),
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

/// Atomically claim due tasks.
///
/// Within a single transaction, reads every enabled task, checks if it is due
/// (using the given `now`), and attempts an optimistic-lock UPDATE of
/// `last_run_at`. Only tasks whose UPDATE succeeds (1 row changed) are
/// returned — preventing duplicate execution from overlapping ticks or
/// restarts.
async fn claim_due_tasks(
    db_path: &Path,
    now: DateTime<Utc>,
) -> Result<Vec<ScheduledTask>, io::Error> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;

        // Read all enabled tasks.
        let mut stmt = conn
            .prepare(
                "SELECT id, name, schedule_kind, cron_expr, interval_secs, run_at,
                        job_kind, payload_json, enabled, created_at, last_run_at
                 FROM scheduled_tasks
                 WHERE enabled = 1",
            )
            .map_err(sql_error)?;

        let rows = stmt
            .query_map([], row_to_scheduled_task)
            .map_err(sql_error)?;

        let mut candidates = Vec::new();
        for row in rows {
            let task = row.map_err(sql_error)?;
            if is_task_due(&task, now) {
                candidates.push(task);
            }
        }

        // Atomically claim each candidate.
        let now_str = now.to_rfc3339();
        let mut claimed = Vec::new();
        for task in &candidates {
            let old_last: Option<String> = task.last_run_at.map(|dt| dt.to_rfc3339());
            let changes = if let Some(ref old) = old_last {
                conn.execute(
                    "UPDATE scheduled_tasks SET last_run_at = ?1
                     WHERE id = ?2 AND enabled = 1 AND last_run_at = ?3",
                    params![now_str, task.id, old],
                )
            } else {
                conn.execute(
                    "UPDATE scheduled_tasks SET last_run_at = ?1
                     WHERE id = ?2 AND enabled = 1 AND last_run_at IS NULL",
                    params![now_str, task.id],
                )
            }
            .map_err(sql_error)?;
            if changes > 0 {
                claimed.push(task.clone());
            }
        }

        Ok(claimed)
    })
    .await
    .map_err(|e| io::Error::other(e.to_string()))?
}

async fn record_run_start(db_path: &Path, task_id: &str, job_id: &str) -> i64 {
    let db_path = db_path.to_path_buf();
    let task_id = task_id.to_string();
    let job_id = job_id.to_string();
    match tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO task_runs (task_id, job_id, status, started_at) VALUES (?1, ?2, 'running', ?3)",
            params![task_id, job_id, now],
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

/// Atomically finalise a scheduled run: update the `task_runs` row AND refresh
/// `scheduled_tasks.last_run_at`, and for `once` tasks disable further
/// execution.
async fn complete_scheduled_run(
    db_path: &Path,
    task_id: &str,
    run_id: i64,
    status: &str,
    error: Option<&str>,
    now: DateTime<Utc>,
    is_once: bool,
) {
    let db_path = db_path.to_path_buf();
    let task_id = task_id.to_string();
    let status = status.to_string();
    let error = error.map(|s| s.to_string());
    let now_str = now.to_rfc3339();
    let _ = tokio::task::spawn_blocking(move || {
        let conn = open_sqlite(&db_path)?;

        // Update the task_runs row.
        conn.execute(
            "UPDATE task_runs SET status = ?1, finished_at = ?2, error = ?3 WHERE id = ?4",
            params![status, now_str, error, run_id],
        )
        .map_err(sql_error)?;

        // Refresh last_run_at on the parent task.
        conn.execute(
            "UPDATE scheduled_tasks SET last_run_at = ?1 WHERE id = ?2",
            params![now_str, task_id],
        )
        .map_err(sql_error)?;

        // Once-tasks self-disable.
        if is_once {
            conn.execute(
                "UPDATE scheduled_tasks SET enabled = 0 WHERE id = ?1",
                params![task_id],
            )
            .map_err(sql_error)?;
        }

        Ok::<_, io::Error>(())
    })
    .await;
}

fn is_task_due(task: &ScheduledTask, now: DateTime<Utc>) -> bool {
    match task.schedule_kind.as_str() {
        "once" => {
            // A once task is due exactly when run_at has passed AND it has
            // never been executed (last_run_at is still NULL).  The atomic
            // claim will set last_run_at, so a second tick cannot re-fire it.
            if let Some(run_at) = task.run_at {
                run_at <= now && task.last_run_at.is_none()
            } else {
                false
            }
        }
        "interval" => {
            // First run: always due.  Later runs: due when `last + interval ≤ now`.
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
                    // Reference point: last run, or creation time if never run.
                    let reference = task.last_run_at.unwrap_or(task.created_at);
                    // The cron crate's `after()` returns an iterator of fire
                    // times starting *after* the given DateTime.
                    if let Some(next_fire) = schedule.after(&reference).next() {
                        next_fire <= now
                    } else {
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
            .query_map([], row_to_scheduled_task)
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

pub async fn upsert_scheduled_task(db_path: &Path, task: &ScheduledTask) -> io::Result<()> {
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
        conn.execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])
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
                    status: row
                        .get::<_, String>(3)
                        .unwrap_or_else(|_| "unknown".to_string()),
                    started_at: row
                        .get::<_, String>(4)
                        .ok()
                        .and_then(|s| {
                            DateTime::parse_from_rfc3339(&s)
                                .ok()
                                .map(|dt| dt.with_timezone(&Utc))
                        })
                        .unwrap_or_else(Utc::now),
                    finished_at: row.get::<_, Option<String>>(5)?.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    }),
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::sync::Mutex as StdMutex;

    /// A clock whose time can be externally controlled.
    pub struct FakeClock {
        now: StdMutex<DateTime<Utc>>,
    }

    impl FakeClock {
        pub fn new(start: DateTime<Utc>) -> Self {
            Self {
                now: StdMutex::new(start),
            }
        }

        pub fn set(&self, t: DateTime<Utc>) {
            *self.now.lock().unwrap() = t;
        }

        pub fn advance(&self, d: chrono::Duration) -> DateTime<Utc> {
            let mut now = self.now.lock().unwrap();
            *now = *now + d;
            *now
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> DateTime<Utc> {
            *self.now.lock().unwrap()
        }
    }

    // ── Test helpers ──────────────────────────────────────────────────────

    const MIGRATION_3_SQL: &str = r#"
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
    "#;

    fn setup_db(dir: &tempfile::TempDir) -> PathBuf {
        let db_path = dir.path().join("test_scheduler.sqlite");
        let conn = Connection::open(&db_path).expect("open test db");
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        conn.execute_batch(MIGRATION_3_SQL).unwrap();
        conn.close().ok();
        db_path
    }

    fn insert_task(db_path: &Path, task: &ScheduledTask) {
        let conn = open_sqlite(db_path).unwrap();
        conn.execute(
            "INSERT INTO scheduled_tasks
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
        .unwrap();
    }

    async fn setup_harness(start_time: DateTime<Utc>) -> TestHarness {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = setup_db(&dir);

        let clock = Arc::new(FakeClock::new(start_time));
        let registry = Arc::new(JobRegistry::new(4));
        let dispatcher = Arc::new(ActionDispatcher::new());

        dispatcher
            .register(
                "generic",
                Arc::new(|payload: Value, ctx: JobCtx| {
                    Box::pin(async move {
                        ctx.progress(50, "working", "test work");
                        Ok(payload)
                    })
                }),
            )
            .await;

        let scheduler = Scheduler::new(
            db_path.clone(),
            registry.clone(),
            dispatcher.clone(),
            clock.clone(),
        );

        TestHarness {
            _dir: dir,
            db_path,
            clock,
            registry,
            dispatcher,
            scheduler,
        }
    }

    struct TestHarness {
        _dir: tempfile::TempDir,
        db_path: PathBuf,
        clock: Arc<FakeClock>,
        registry: Arc<JobRegistry>,
        dispatcher: Arc<ActionDispatcher>,
        scheduler: Scheduler,
    }

    fn default_task(id: &str, name: &str) -> ScheduledTask {
        let now = Utc::now();
        ScheduledTask {
            id: id.to_string(),
            name: name.to_string(),
            schedule_kind: "generic".to_string(),
            cron_expr: None,
            interval_secs: None,
            run_at: None,
            job_kind: "generic".to_string(),
            payload_json: Some(r#"{"test":true}"#.to_string()),
            enabled: true,
            created_at: now,
            last_run_at: None,
        }
    }

    fn task_runs_for(db_path: &Path, task_id: &str) -> Vec<TaskRun> {
        let conn = open_sqlite(db_path).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, job_id, status, started_at, finished_at, error
                 FROM task_runs WHERE task_id = ?1 ORDER BY id",
            )
            .unwrap();
        stmt.query_map(params![task_id], |row| {
            Ok(TaskRun {
                id: row.get(0)?,
                task_id: row.get(1)?,
                job_id: row.get(2)?,
                status: row.get::<_, String>(3).unwrap_or_else(|_| "unknown".to_string()),
                started_at: row
                    .get::<_, String>(4)
                    .ok()
                    .and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    })
                    .unwrap_or_else(Utc::now),
                finished_at: row.get::<_, Option<String>>(5)?.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
                error: row.get(6)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    fn get_task(db_path: &Path, id: &str) -> Option<ScheduledTask> {
        let conn = open_sqlite(db_path).unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, schedule_kind, cron_expr, interval_secs, run_at,
                        job_kind, payload_json, enabled, created_at, last_run_at
                 FROM scheduled_tasks WHERE id = ?1",
            )
            .unwrap();
        stmt.query_row(params![id], row_to_scheduled_task).ok()
    }

    /// Helper: run a scheduler tick and wait for spawned jobs to finish.
    async fn tick_and_wait(h: &TestHarness) {
        if let Err(e) = h.scheduler.tick().await {
            if !e.contains("no handler registered") {
                panic!("tick failed: {e}");
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // ── Unit tests: is_task_due ───────────────────────────────────────────

    #[test]
    fn due_once_past_run_at_not_yet_run() {
        let t0 = Utc::now();
        let task = ScheduledTask {
            run_at: Some(t0 - chrono::Duration::minutes(10)),
            last_run_at: None,
            schedule_kind: "once".to_string(),
            ..default_task("t1", "test")
        };
        assert!(is_task_due(&task, t0));
    }

    #[test]
    fn due_once_not_due_before_run_at() {
        let t0 = Utc::now();
        let task = ScheduledTask {
            run_at: Some(t0 + chrono::Duration::minutes(10)),
            last_run_at: None,
            schedule_kind: "once".to_string(),
            ..default_task("t1", "test")
        };
        assert!(!is_task_due(&task, t0));
    }

    #[test]
    fn due_once_not_due_if_already_run() {
        let t0 = Utc::now();
        let task = ScheduledTask {
            run_at: Some(t0 - chrono::Duration::minutes(20)),
            last_run_at: Some(t0 - chrono::Duration::minutes(10)),
            schedule_kind: "once".to_string(),
            ..default_task("t1", "test")
        };
        assert!(!is_task_due(&task, t0));
    }

    #[test]
    fn due_interval_first_run_always_due() {
        let task = ScheduledTask {
            interval_secs: Some(60),
            last_run_at: None,
            schedule_kind: "interval".to_string(),
            ..default_task("t1", "test")
        };
        assert!(is_task_due(&task, Utc::now()));
    }

    #[test]
    fn due_interval_not_due_before_elapsed() {
        let t0 = Utc::now();
        let task = ScheduledTask {
            interval_secs: Some(60),
            last_run_at: Some(t0 - chrono::Duration::seconds(30)),
            schedule_kind: "interval".to_string(),
            ..default_task("t1", "test")
        };
        assert!(!is_task_due(&task, t0));
    }

    #[test]
    fn due_interval_due_after_elapsed() {
        let t0 = Utc::now();
        let task = ScheduledTask {
            interval_secs: Some(60),
            last_run_at: Some(t0 - chrono::Duration::seconds(65)),
            schedule_kind: "interval".to_string(),
            ..default_task("t1", "test")
        };
        assert!(is_task_due(&task, t0));
    }

    #[test]
    fn due_cron_fires_after_last_run() {
        let t0 = Utc.with_ymd_and_hms(2025, 6, 14, 12, 0, 0).unwrap();
        let created = t0 - chrono::Duration::hours(1);
        let task = ScheduledTask {
            cron_expr: Some("0 5 * * * *".to_string()),
            last_run_at: None,
            created_at: created,
            schedule_kind: "cron".to_string(),
            ..default_task("t1", "test")
        };
        // Next fire after created (11:00) is 11:05, which is ≤ 12:00 → due.
        assert!(is_task_due(&task, t0));
    }

    #[test]
    fn due_cron_not_due_before_first_fire() {
        let t0 = Utc.with_ymd_and_hms(2025, 6, 14, 12, 0, 0).unwrap();
        let created = t0 - chrono::Duration::minutes(2); // 11:58
        let task = ScheduledTask {
            cron_expr: Some("0 5 * * * *".to_string()),
            last_run_at: None,
            created_at: created,
            schedule_kind: "cron".to_string(),
            ..default_task("t1", "test")
        };
        // After 11:58, next fire is 12:05, > 12:00 → not due.
        assert!(!is_task_due(&task, t0));
    }

    #[test]
    fn due_cron_due_after_interval() {
        let t0 = Utc.with_ymd_and_hms(2025, 6, 14, 12, 10, 0).unwrap();
        let last_run = t0 - chrono::Duration::minutes(15); // 11:55
        let task = ScheduledTask {
            cron_expr: Some("0 5 * * * *".to_string()),
            last_run_at: Some(last_run),
            schedule_kind: "cron".to_string(),
            ..default_task("t1", "test")
        };
        // After 11:55, next fire is 12:05, ≤ 12:10 → due.
        assert!(is_task_due(&task, t0));
    }

    #[test]
    fn due_cron_not_due_future_fire() {
        let t0 = Utc.with_ymd_and_hms(2025, 6, 14, 12, 3, 0).unwrap();
        let last_run = Utc.with_ymd_and_hms(2025, 6, 14, 12, 0, 0).unwrap();
        let task = ScheduledTask {
            cron_expr: Some("0 5 * * * *".to_string()),
            last_run_at: Some(last_run),
            schedule_kind: "cron".to_string(),
            ..default_task("t1", "test")
        };
        // After 12:00, next fire is 12:05, > 12:03 → not due.
        assert!(!is_task_due(&task, t0));
    }

    // ── Integration tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn once_task_fires_exactly_once() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "once-1".into(),
                name: "test once".into(),
                schedule_kind: "once".into(),
                run_at: Some(t0 - chrono::Duration::seconds(1)),
                job_kind: "generic".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("once-1", "test")
            },
        );

        tick_and_wait(&h).await;
        assert_eq!(task_runs_for(&h.db_path, "once-1").len(), 1);

        // Second tick — must NOT fire again.
        tick_and_wait(&h).await;
        assert_eq!(
            task_runs_for(&h.db_path, "once-1").len(),
            1,
            "once task must not fire twice"
        );
    }

    #[tokio::test]
    async fn once_task_disabled_after_run() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "once-dis".into(),
                name: "test once disable".into(),
                schedule_kind: "once".into(),
                run_at: Some(t0 - chrono::Duration::seconds(1)),
                job_kind: "generic".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("once-dis", "test")
            },
        );

        tick_and_wait(&h).await;
        let t = get_task(&h.db_path, "once-dis").expect("task should exist");
        assert!(!t.enabled, "once task must be disabled after run");
        assert!(t.last_run_at.is_some(), "last_run_at must be set");
    }

    #[tokio::test]
    async fn interval_task_fires_repeatedly() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "int-1".into(),
                name: "test interval".into(),
                schedule_kind: "interval".into(),
                interval_secs: Some(10),
                job_kind: "generic".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("int-1", "test")
            },
        );

        tick_and_wait(&h).await;
        assert_eq!(task_runs_for(&h.db_path, "int-1").len(), 1);

        // Advance 5 s — not due yet.
        h.clock.advance(chrono::Duration::seconds(5));
        tick_and_wait(&h).await;
        assert_eq!(task_runs_for(&h.db_path, "int-1").len(), 1);

        // Advance another 10 s — due (15 s total).
        h.clock.advance(chrono::Duration::seconds(10));
        tick_and_wait(&h).await;
        assert_eq!(task_runs_for(&h.db_path, "int-1").len(), 2);
    }

    #[tokio::test]
    async fn cron_task_fires_on_schedule() {
        let t0 = Utc.with_ymd_and_hms(2025, 6, 14, 12, 0, 0).unwrap();
        let h = setup_harness(t0).await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "cron-1".into(),
                name: "test cron".into(),
                schedule_kind: "cron".into(),
                cron_expr: Some("0 * * * * *".into()), // every minute
                job_kind: "generic".into(),
                enabled: true,
                last_run_at: None,
                created_at: t0 - chrono::Duration::minutes(5),
                ..default_task("cron-1", "test")
            },
        );

        tick_and_wait(&h).await;
        assert_eq!(task_runs_for(&h.db_path, "cron-1").len(), 1);

        // Advance 30 s — not due.
        h.clock.advance(chrono::Duration::seconds(30));
        tick_and_wait(&h).await;
        assert_eq!(task_runs_for(&h.db_path, "cron-1").len(), 1);

        // Advance to 12:01.
        h.clock.set(Utc.with_ymd_and_hms(2025, 6, 14, 12, 1, 0).unwrap());
        tick_and_wait(&h).await;
        assert_eq!(task_runs_for(&h.db_path, "cron-1").len(), 2);
    }

    #[tokio::test]
    async fn concurrent_ticks_do_not_duplicate() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "conc-1".into(),
                name: "test concurrent".into(),
                schedule_kind: "once".into(),
                run_at: Some(t0 - chrono::Duration::seconds(1)),
                job_kind: "generic".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("conc-1", "test")
            },
        );

        let (r1, r2) = tokio::join!(h.scheduler.tick(), h.scheduler.tick());
        assert!(r1.is_ok() || r2.is_ok());
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(
            task_runs_for(&h.db_path, "conc-1").len(),
            1,
            "concurrent ticks must produce exactly one task_run"
        );
    }

    #[tokio::test]
    async fn job_id_recorded_in_task_runs() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "jid-1".into(),
                name: "test job_id".into(),
                schedule_kind: "once".into(),
                run_at: Some(t0 - chrono::Duration::seconds(1)),
                job_kind: "generic".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("jid-1", "test")
            },
        );

        tick_and_wait(&h).await;
        let runs = task_runs_for(&h.db_path, "jid-1");
        assert_eq!(runs.len(), 1);
        let jid = runs[0].job_id.as_ref().expect("job_id must be set");
        assert!(!jid.is_empty());
        assert_eq!(jid.len(), 36, "job_id should be a UUID v4");
    }

    #[tokio::test]
    async fn failure_records_error_in_task_runs() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        // Register a handler that always fails.
        h.dispatcher
            .register(
                "failing",
                Arc::new(|_payload: Value, _ctx: JobCtx| {
                    Box::pin(async move { Err("simulated failure".to_string()) })
                }),
            )
            .await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "fail-1".into(),
                name: "test failure".into(),
                schedule_kind: "once".into(),
                run_at: Some(t0 - chrono::Duration::seconds(1)),
                job_kind: "failing".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("fail-1", "test")
            },
        );

        tick_and_wait(&h).await;
        let runs = task_runs_for(&h.db_path, "fail-1");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "error");
        assert_eq!(runs[0].error.as_deref(), Some("simulated failure"));
    }

    #[tokio::test]
    async fn last_run_at_updated_after_completion() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "lru-1".into(),
                name: "test last_run_at".into(),
                schedule_kind: "interval".into(),
                interval_secs: Some(30),
                job_kind: "generic".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("lru-1", "test")
            },
        );

        tick_and_wait(&h).await;
        let t = get_task(&h.db_path, "lru-1").expect("task should exist");
        assert!(t.last_run_at.is_some(), "last_run_at must be updated");
        let diff = (t.last_run_at.unwrap() - t0).num_seconds().abs();
        assert!(diff < 5, "last_run_at should be within 5s of start");
    }

    #[tokio::test]
    async fn recovery_marks_stale_runs_as_error() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        // Insert a parent task so the FK constraint is satisfied.
        let dummy_task = ScheduledTask {
            id: "dummy-task".into(),
            name: "dummy".into(),
            ..default_task("dummy-task", "dummy")
        };
        insert_task(&h.db_path, &dummy_task);

        // Simulate a "running" task_run left over from a crash.
        {
            let conn = open_sqlite(&h.db_path).unwrap();
            conn.execute(
                "INSERT INTO task_runs (task_id, job_id, status, started_at)
                 VALUES ('dummy-task', 'dummy-jid', 'running', ?1)",
                params![t0.to_rfc3339()],
            )
            .unwrap();
        }

        h.scheduler.recover_stale_runs().await.unwrap();

        let conn = open_sqlite(&h.db_path).unwrap();
        let (status, error): (String, Option<String>) = conn
            .query_row(
                "SELECT status, error FROM task_runs WHERE task_id = 'dummy-task'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "error");
        assert_eq!(error.as_deref(), Some("daemon restarted"));
    }

    #[tokio::test]
    async fn cancel_active_jobs_on_shutdown() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        h.dispatcher
            .register(
                "long_running",
                Arc::new(|_payload: Value, ctx: JobCtx| {
                    Box::pin(async move {
                        ctx.progress(10, "start", "long job started");
                        for _ in 0..50 {
                            if ctx.is_cancelled() {
                                return Err("cancelled".to_string());
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Ok(Value::Null)
                    })
                }),
            )
            .await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "long-1".into(),
                name: "test cancel".into(),
                schedule_kind: "once".into(),
                run_at: Some(t0 - chrono::Duration::seconds(1)),
                job_kind: "long_running".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("long-1", "test")
            },
        );

        tick_and_wait(&h).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Cancel all active jobs (simulating shutdown).
        h.scheduler.cancel_all_active().await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let jobs = h.registry.list().await;
        let long_job = jobs.iter().find(|j| j.kind == "long_running");
        if let Some(j) = long_job {
            assert!(
                matches!(j.status, JobStatus::Cancelled | JobStatus::Error),
                "long job should be cancelled, got {:?}",
                j.status
            );
        }
    }

    #[tokio::test]
    async fn once_task_disabled_even_on_failure() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        h.dispatcher
            .register(
                "fragile",
                Arc::new(|_payload: Value, _ctx: JobCtx| {
                    Box::pin(async move { Err("boom".to_string()) })
                }),
            )
            .await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "frag-1".into(),
                name: "test fragile".into(),
                schedule_kind: "once".into(),
                run_at: Some(t0 - chrono::Duration::seconds(1)),
                job_kind: "fragile".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("frag-1", "test")
            },
        );

        tick_and_wait(&h).await;

        let t = get_task(&h.db_path, "frag-1").expect("task should exist");
        assert!(!t.enabled, "once task must be disabled even after failure");
        assert!(t.last_run_at.is_some(), "last_run_at must be set");

        let runs = task_runs_for(&h.db_path, "frag-1");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "error");
    }

    #[tokio::test]
    async fn no_orphan_jobs_after_completion() {
        let t0 = Utc::now();
        let h = setup_harness(t0).await;

        insert_task(
            &h.db_path,
            &ScheduledTask {
                id: "orphan-1".into(),
                name: "test orphan".into(),
                schedule_kind: "once".into(),
                run_at: Some(t0 - chrono::Duration::seconds(1)),
                job_kind: "generic".into(),
                enabled: true,
                last_run_at: None,
                ..default_task("orphan-1", "test")
            },
        );

        tick_and_wait(&h).await;

        let jobs = h.registry.list().await;
        let done: Vec<_> = jobs
            .iter()
            .filter(|j| j.status == JobStatus::Done)
            .collect();
        assert!(!done.is_empty(), "completed jobs should be visible");

        let record = h.registry.get(&done[0].id).await;
        assert!(record.is_some());
        assert_eq!(record.unwrap().status, JobStatus::Done);
    }
}
