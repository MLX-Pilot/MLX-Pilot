//! Orchestration monitor — live aggregation of agent telemetry.
//!
//! The [`OrchestrationRegistry`] subscribes to the agent [`EventBus`] and keeps
//! the *living* state of every run (phases, sub-agents, reasoning feed, metrics)
//! outside the agent's critical path. It is a **read-mostly observability** layer:
//! it never feeds back into the runtime, and if no UI is attached it simply keeps
//! a bounded in-memory model plus a persisted history in SQLite.
//!
//! Data sources (aggregated, never recreated):
//! - [`AgentEvent`] from the `EventBus` (lifecycle, thinking, tool calls, handoffs).
//! - `ContextBudgetTelemetry` from `AppState.agent_state.budget_tracker` (tokens).
//! - Tool counts derived from the tool-call events themselves.
//!
//! Streaming convention: each reasoning event carries a monotonic `seq`. The SSE
//! endpoint replays buffered events after a client-supplied cursor (`Last-Event-ID`
//! header or `?after=`), so a reconnect resynchronises without duplicating events.
//! Polling (`GET /agent/orchestration/{run_id}`) is the documented fallback.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::Json;
use chrono::{DateTime, Utc};
use mlx_agent_core::{AgentEvent, ContextBudgetTelemetry, EventBus};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tracing::{debug, warn};

/// Max reasoning events kept in memory (and persisted) per run.
const MAX_EVENTS_PER_RUN: usize = 1_000;
/// Max runs kept fully in memory; older ones live only in SQLite history.
const MAX_INMEM_RUNS: usize = 60;
/// Flush the thinking accumulator once it reaches this many characters.
const THINKING_FLUSH_CHARS: usize = 220;
/// Capacity of each run's SSE fan-out channel.
const RUN_BROADCAST_CAPACITY: usize = 512;

// ── Serializable telemetry model ─────────────────────────────────────────────

/// Lifecycle status of an orchestration run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "completed" => RunStatus::Completed,
            "failed" => RunStatus::Failed,
            "cancelled" => RunStatus::Cancelled,
            _ => RunStatus::Running,
        }
    }

    fn is_terminal(self) -> bool {
        !matches!(self, RunStatus::Running)
    }
}

/// A single sub-agent / delegation activity inside a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivity {
    /// Session id of the agent (root or delegated child).
    pub id: String,
    /// Display label (e.g. `main:qwen2.5:7b` or `sub:qwen2.5:7b`).
    pub label: String,
    /// `main` for the root agent, `sub` for delegations.
    pub role: String,
    /// `running` | `completed` | `failed`.
    pub status: String,
    pub model: String,
    pub tokens_total: usize,
    pub tool_calls: usize,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    pub elapsed_ms: u64,
}

/// A phase groups agents by stage of the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    pub name: String,
    pub status: String,
    pub progress: f64,
    pub agents: Vec<AgentActivity>,
}

/// One entry in the reasoning / action feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningEvent {
    pub seq: u64,
    pub ts: DateTime<Utc>,
    pub run_id: String,
    pub session_id: String,
    /// `thinking` | `answer` | `action` | `tool_call` | `tool_result` | `phase` | `error`.
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub meta: Value,
}

/// Aggregated metrics for a single run.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunMetrics {
    pub tokens_total: usize,
    pub tool_calls: usize,
    pub agents: usize,
    pub phases: usize,
    pub elapsed_ms: u64,
}

/// Aggregated metrics across all runs (footer status bar).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalMetrics {
    pub active_runs: usize,
    pub total_runs: usize,
    pub total_tokens: usize,
    pub total_tool_calls: usize,
    /// Wall-clock since the earliest still-active run started.
    pub active_elapsed_ms: u64,
}

/// Full snapshot of a run (used by polling + history replay).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationRun {
    pub run_id: String,
    pub root_session_id: String,
    pub label: String,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    pub phases: Vec<Phase>,
    pub metrics: RunMetrics,
    /// Latest reasoning events (bounded buffer).
    pub events: Vec<ReasoningEvent>,
    /// Highest event seq present (cursor for SSE replay).
    pub last_seq: u64,
    /// Events with `seq <= dropped_before_seq` were pruned from the buffer.
    pub dropped_before_seq: u64,
    /// Whether this run is still tracked live in memory (vs. history only).
    pub live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

/// Compact run header for list views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub root_session_id: String,
    pub label: String,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    pub metrics: RunMetrics,
    pub live: bool,
}

/// Response of `GET /agent/orchestration`.
#[derive(Debug, Clone, Serialize)]
pub struct OrchestrationListResponse {
    pub runs: Vec<RunSummary>,
    pub metrics: GlobalMetrics,
}

// ── Internal live state ──────────────────────────────────────────────────────

struct RunState {
    run_id: String,
    root_session_id: String,
    label: String,
    model: String,
    status: RunStatus,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    /// Activities in insertion order (root first, then delegations).
    activities: Vec<AgentActivity>,
    events: VecDeque<ReasoningEvent>,
    last_seq: u64,
    dropped_before_seq: u64,
    job_id: Option<String>,
    /// Per-session in-flight thinking/answer accumulators (kind -> text).
    thinking: HashMap<String, String>,
    answer: HashMap<String, String>,
    tx: broadcast::Sender<ReasoningEvent>,
    persisted: bool,
}

impl RunState {
    fn new(run_id: String, model: String, started_at: DateTime<Utc>) -> Self {
        let (tx, _) = broadcast::channel(RUN_BROADCAST_CAPACITY);
        let label = run_label(&model);
        RunState {
            root_session_id: run_id.clone(),
            run_id,
            label,
            model,
            status: RunStatus::Running,
            started_at,
            ended_at: None,
            activities: Vec::new(),
            events: VecDeque::new(),
            last_seq: 0,
            dropped_before_seq: 0,
            job_id: None,
            thinking: HashMap::new(),
            answer: HashMap::new(),
            tx,
            persisted: false,
        }
    }

    fn activity_mut(&mut self, session_id: &str) -> Option<&mut AgentActivity> {
        self.activities.iter_mut().find(|a| a.id == session_id)
    }

    fn ensure_activity(&mut self, session_id: &str, model: &str, now: DateTime<Utc>) {
        if self.activities.iter().any(|a| a.id == session_id) {
            return;
        }
        let is_root = session_id == self.root_session_id;
        let role = if is_root { "main" } else { "sub" };
        let model = if model.is_empty() {
            self.model.clone()
        } else {
            model.to_string()
        };
        self.activities.push(AgentActivity {
            id: session_id.to_string(),
            label: format!("{role}:{}", short_label(&model)),
            role: role.to_string(),
            status: "running".to_string(),
            model,
            tokens_total: 0,
            tool_calls: 0,
            started_at: now,
            ended_at: None,
            elapsed_ms: 0,
        });
    }

    fn push_event(&mut self, ev: ReasoningEvent) {
        self.last_seq = ev.seq;
        let _ = self.tx.send(ev.clone());
        self.events.push_back(ev);
        while self.events.len() > MAX_EVENTS_PER_RUN {
            if let Some(dropped) = self.events.pop_front() {
                self.dropped_before_seq = dropped.seq;
            }
        }
    }

    fn recompute_elapsed(&mut self, now: DateTime<Utc>) {
        for a in &mut self.activities {
            let end = a.ended_at.unwrap_or(now);
            a.elapsed_ms = (end - a.started_at).num_milliseconds().max(0) as u64;
        }
    }

    fn build_phases(&self) -> Vec<Phase> {
        let mut phases: Vec<Phase> = Vec::new();
        for stage in ["main", "sub"] {
            let agents: Vec<AgentActivity> = self
                .activities
                .iter()
                .filter(|a| a.role == stage)
                .cloned()
                .collect();
            if agents.is_empty() {
                continue;
            }
            let done = agents
                .iter()
                .filter(|a| a.status == "completed" || a.status == "failed")
                .count();
            let progress = if agents.is_empty() {
                0.0
            } else {
                done as f64 / agents.len() as f64
            };
            let status = if done == agents.len() {
                "completed"
            } else {
                "running"
            };
            phases.push(Phase {
                name: if stage == "main" {
                    "Execução".to_string()
                } else {
                    "Delegações".to_string()
                },
                status: status.to_string(),
                progress,
                agents,
            });
        }
        phases
    }

    fn metrics(&self) -> RunMetrics {
        let tokens_total = self.activities.iter().map(|a| a.tokens_total).sum();
        let tool_calls = self.activities.iter().map(|a| a.tool_calls).sum();
        let elapsed_ms = self
            .activities
            .iter()
            .map(|a| a.elapsed_ms)
            .max()
            .unwrap_or(0);
        let phases = self.build_phases().len();
        RunMetrics {
            tokens_total,
            tool_calls,
            agents: self.activities.len(),
            phases,
            elapsed_ms,
        }
    }

    fn snapshot(&self) -> OrchestrationRun {
        OrchestrationRun {
            run_id: self.run_id.clone(),
            root_session_id: self.root_session_id.clone(),
            label: self.label.clone(),
            status: self.status,
            started_at: self.started_at,
            ended_at: self.ended_at,
            phases: self.build_phases(),
            metrics: self.metrics(),
            events: self.events.iter().cloned().collect(),
            last_seq: self.last_seq,
            dropped_before_seq: self.dropped_before_seq,
            live: true,
            job_id: self.job_id.clone(),
        }
    }

    fn summary(&self) -> RunSummary {
        RunSummary {
            run_id: self.run_id.clone(),
            root_session_id: self.root_session_id.clone(),
            label: self.label.clone(),
            status: self.status,
            started_at: self.started_at,
            ended_at: self.ended_at,
            metrics: self.metrics(),
            live: true,
        }
    }
}

#[derive(Default)]
struct Inner {
    runs: BTreeMap<String, RunState>,
    /// session_id -> owning run_id (root or merged delegation).
    session_index: HashMap<String, String>,
    /// run_ids in recency order for in-memory pruning.
    order: VecDeque<String>,
}

/// Registry aggregating agent telemetry into orchestration runs.
pub struct OrchestrationRegistry {
    inner: RwLock<Inner>,
    seq: AtomicU64,
    budget_tracker: Arc<RwLock<BTreeMap<String, ContextBudgetTelemetry>>>,
    db_path: PathBuf,
}

impl OrchestrationRegistry {
    pub fn new(
        budget_tracker: Arc<RwLock<BTreeMap<String, ContextBudgetTelemetry>>>,
        db_path: PathBuf,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(Inner::default()),
            seq: AtomicU64::new(0),
            budget_tracker,
            db_path,
        })
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Spawn the background consumer that subscribes to the `EventBus`.
    ///
    /// Uses a `broadcast` receiver, so it never blocks the agent: if the consumer
    /// lags it drops the oldest unread events (observability degrades gracefully).
    pub fn start_consumer(self: &Arc<Self>, event_bus: &EventBus) {
        let mut rx = event_bus.subscribe();
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => this.handle_event(event).await,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("orchestration consumer lagged, dropped {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    async fn handle_event(self: &Arc<Self>, event: AgentEvent) {
        let now = Utc::now();
        match event {
            AgentEvent::RunStarted { session_id, model } => {
                let mut inner = self.inner.write().await;
                let run_id = inner
                    .session_index
                    .get(&session_id)
                    .cloned()
                    .unwrap_or_else(|| session_id.clone());
                let created = !inner.runs.contains_key(&run_id);
                if created {
                    inner
                        .runs
                        .insert(run_id.clone(), RunState::new(run_id.clone(), model.clone(), now));
                    inner.session_index.insert(run_id.clone(), run_id.clone());
                    touch_order(&mut inner.order, &run_id);
                }
                let seq = self.next_seq();
                if let Some(run) = inner.runs.get_mut(&run_id) {
                    run.status = RunStatus::Running;
                    run.ended_at = None;
                    run.persisted = false;
                    if !model.is_empty() && run.root_session_id == session_id {
                        run.model = model.clone();
                        run.label = run_label(&model);
                    }
                    run.ensure_activity(&session_id, &model, now);
                    if let Some(a) = run.activity_mut(&session_id) {
                        a.status = "running".to_string();
                    }
                    run.push_event(ReasoningEvent {
                        seq,
                        ts: now,
                        run_id: run_id.clone(),
                        session_id: session_id.clone(),
                        kind: "phase".to_string(),
                        text: "Iniciou um fluxo de trabalho".to_string(),
                        tool: None,
                        meta: json!({ "model": model }),
                    });
                }
                drop(inner);
                self.prune_memory().await;
            }
            AgentEvent::SessionIngress {
                session_id,
                origin_kind,
                ..
            } => {
                self.emit_for_session(
                    &session_id,
                    now,
                    "phase",
                    format!("Sessão iniciada ({origin_kind})"),
                    None,
                    Value::Null,
                )
                .await;
            }
            AgentEvent::ThinkingDelta { session_id, delta } => {
                self.accumulate(&session_id, "thinking", &delta, now).await;
            }
            AgentEvent::TextDelta { session_id, delta } => {
                self.accumulate(&session_id, "answer", &delta, now).await;
            }
            AgentEvent::ToolCallStarted {
                session_id, tool, ..
            } => {
                self.flush_accumulators(&session_id, now).await;
                let label = humanize_tool(&tool);
                let mut inner = self.inner.write().await;
                if let Some(run_id) = inner.session_index.get(&session_id).cloned() {
                    let seq = self.next_seq();
                    if let Some(run) = inner.runs.get_mut(&run_id) {
                        if let Some(a) = run.activity_mut(&session_id) {
                            a.tool_calls += 1;
                        }
                        run.push_event(ReasoningEvent {
                            seq,
                            ts: now,
                            run_id,
                            session_id: session_id.clone(),
                            kind: "tool_call".to_string(),
                            text: label,
                            tool: Some(tool.clone()),
                            meta: json!({ "tool": tool }),
                        });
                    }
                }
            }
            AgentEvent::ToolCallCompleted {
                session_id,
                tool,
                result_preview,
                ..
            } => {
                let preview = sanitize(&truncate(&result_preview, 280));
                self.emit_for_session(
                    &session_id,
                    now,
                    "tool_result",
                    format!("Ferramenta {tool} concluída"),
                    Some(tool.clone()),
                    json!({ "tool": tool, "preview": preview }),
                )
                .await;
            }
            AgentEvent::ToolCallDenied {
                session_id,
                tool,
                reason,
            } => {
                self.emit_for_session(
                    &session_id,
                    now,
                    "action",
                    format!("Ferramenta {tool} negada: {}", sanitize(&reason)),
                    Some(tool.clone()),
                    json!({ "tool": tool }),
                )
                .await;
            }
            AgentEvent::SessionHandoff {
                parent_session_id,
                child_session_id,
                handoff_summary,
            } => {
                self.merge_delegation(
                    &parent_session_id,
                    &child_session_id,
                    &handoff_summary,
                    now,
                )
                .await;
            }
            AgentEvent::RunCompleted {
                session_id,
                latency_ms,
            } => {
                self.flush_accumulators(&session_id, now).await;
                self.complete_session(&session_id, RunStatus::Completed, latency_ms, None, now)
                    .await;
            }
            AgentEvent::RunFailed { session_id, error } => {
                self.flush_accumulators(&session_id, now).await;
                self.complete_session(
                    &session_id,
                    RunStatus::Failed,
                    0,
                    Some(sanitize(&error)),
                    now,
                )
                .await;
            }
            AgentEvent::SessionSummaryUpdated {
                session_id,
                summary,
            } => {
                self.emit_for_session(
                    &session_id,
                    now,
                    "phase",
                    format!("Resumo: {}", sanitize(&truncate(&summary, 200))),
                    None,
                    Value::Null,
                )
                .await;
            }
            AgentEvent::ApprovalRequired { request } => {
                // Approval requests are not session-scoped; attach to the latest run.
                self.emit_for_latest(
                    now,
                    "action",
                    format!("Aprovação necessária: {}", request.tool_name),
                )
                .await;
            }
            AgentEvent::SkillLoaded { name, .. } => {
                // Skills aren't session-scoped; attach to the most recent run.
                self.emit_for_latest(now, "action", format!("Carregou skill {name}"))
                    .await;
            }
            // Lower-signal events for the monitor.
            AgentEvent::ApprovalReceived { .. } | AgentEvent::SkillRejected { .. } => {}
            AgentEvent::AuditEntry { .. } => {}
        }
    }

    async fn accumulate(&self, session_id: &str, kind: &str, delta: &str, now: DateTime<Utc>) {
        let mut inner = self.inner.write().await;
        let Some(run_id) = inner.session_index.get(session_id).cloned() else {
            return;
        };
        let flush = {
            let Some(run) = inner.runs.get_mut(&run_id) else {
                return;
            };
            let bucket = if kind == "answer" {
                &mut run.answer
            } else {
                &mut run.thinking
            };
            let acc = bucket.entry(session_id.to_string()).or_default();
            acc.push_str(delta);
            acc.chars().count() >= THINKING_FLUSH_CHARS
        };
        if flush {
            let seq = self.next_seq();
            if let Some(run) = inner.runs.get_mut(&run_id) {
                flush_bucket(run, session_id, kind, seq, now);
            }
        }
    }

    async fn flush_accumulators(&self, session_id: &str, now: DateTime<Utc>) {
        let mut inner = self.inner.write().await;
        let Some(run_id) = inner.session_index.get(session_id).cloned() else {
            return;
        };
        for kind in ["thinking", "answer"] {
            let has = inner
                .runs
                .get(&run_id)
                .map(|r| {
                    let bucket = if kind == "answer" { &r.answer } else { &r.thinking };
                    bucket.get(session_id).map(|s| !s.is_empty()).unwrap_or(false)
                })
                .unwrap_or(false);
            if has {
                let seq = self.next_seq();
                if let Some(run) = inner.runs.get_mut(&run_id) {
                    flush_bucket(run, session_id, kind, seq, now);
                }
            }
        }
    }

    async fn emit_for_session(
        &self,
        session_id: &str,
        now: DateTime<Utc>,
        kind: &str,
        text: String,
        tool: Option<String>,
        meta: Value,
    ) {
        let mut inner = self.inner.write().await;
        let Some(run_id) = inner.session_index.get(session_id).cloned() else {
            return;
        };
        let seq = self.next_seq();
        if let Some(run) = inner.runs.get_mut(&run_id) {
            run.push_event(ReasoningEvent {
                seq,
                ts: now,
                run_id,
                session_id: session_id.to_string(),
                kind: kind.to_string(),
                text,
                tool,
                meta,
            });
        }
    }

    async fn emit_for_latest(&self, now: DateTime<Utc>, kind: &str, text: String) {
        let mut inner = self.inner.write().await;
        let Some(run_id) = inner.order.back().cloned() else {
            return;
        };
        let seq = self.next_seq();
        if let Some(run) = inner.runs.get_mut(&run_id) {
            let session_id = run.root_session_id.clone();
            run.push_event(ReasoningEvent {
                seq,
                ts: now,
                run_id,
                session_id,
                kind: kind.to_string(),
                text,
                tool: None,
                meta: Value::Null,
            });
        }
    }

    async fn merge_delegation(
        &self,
        parent_session_id: &str,
        child_session_id: &str,
        summary: &str,
        now: DateTime<Utc>,
    ) {
        let mut inner = self.inner.write().await;
        // The parent run id (create lazily if the parent run vanished).
        let parent_run_id = inner
            .session_index
            .get(parent_session_id)
            .cloned()
            .unwrap_or_else(|| parent_session_id.to_string());
        if !inner.runs.contains_key(&parent_run_id) {
            inner.runs.insert(
                parent_run_id.clone(),
                RunState::new(parent_run_id.clone(), String::new(), now),
            );
            inner
                .session_index
                .insert(parent_run_id.clone(), parent_run_id.clone());
            touch_order(&mut inner.order, &parent_run_id);
        }

        // Pull the child's activity out of its standalone run (if any).
        let child_run_id = inner
            .session_index
            .get(child_session_id)
            .cloned()
            .unwrap_or_else(|| child_session_id.to_string());
        let mut child_activity: Option<AgentActivity> = None;
        if child_run_id != parent_run_id {
            if let Some(child_run) = inner.runs.get_mut(&child_run_id) {
                if let Some(pos) = child_run
                    .activities
                    .iter()
                    .position(|a| a.id == child_session_id)
                {
                    child_activity = Some(child_run.activities.remove(pos));
                }
            }
            // A standalone child run only ever held the child agent; drop it.
            if inner
                .runs
                .get(&child_run_id)
                .map(|r| r.root_session_id == child_session_id)
                .unwrap_or(false)
            {
                inner.runs.remove(&child_run_id);
                inner.order.retain(|id| id != &child_run_id);
            }
        }
        inner
            .session_index
            .insert(child_session_id.to_string(), parent_run_id.clone());

        let seq = self.next_seq();
        if let Some(parent) = inner.runs.get_mut(&parent_run_id) {
            match child_activity {
                Some(mut act) => {
                    act.role = "sub".to_string();
                    act.label = format!("sub:{}", short_label(&act.model));
                    if act.ended_at.is_none() {
                        act.ended_at = Some(now);
                    }
                    if act.status == "running" {
                        act.status = "completed".to_string();
                    }
                    if !parent.activities.iter().any(|a| a.id == act.id) {
                        parent.activities.push(act);
                    }
                }
                None => parent.ensure_activity(child_session_id, "", now),
            }
            parent.push_event(ReasoningEvent {
                seq,
                ts: now,
                run_id: parent_run_id.clone(),
                session_id: parent_session_id.to_string(),
                kind: "phase".to_string(),
                text: format!(
                    "Delegação concluída: {}",
                    sanitize(&truncate(summary, 200))
                ),
                tool: Some("delegate_session".to_string()),
                meta: json!({ "child_session_id": child_session_id }),
            });
        }
    }

    async fn complete_session(
        self: &Arc<Self>,
        session_id: &str,
        status: RunStatus,
        latency_ms: u64,
        error: Option<String>,
        now: DateTime<Utc>,
    ) {
        // Refresh tokens from the budget tracker before snapshotting.
        let tokens = self.tokens_for(session_id).await;
        let mut to_persist: Option<OrchestrationRun> = None;
        {
            let mut inner = self.inner.write().await;
            let Some(run_id) = inner.session_index.get(session_id).cloned() else {
                return;
            };
            let is_root = inner
                .runs
                .get(&run_id)
                .map(|r| r.root_session_id == session_id)
                .unwrap_or(false);
            let seq = self.next_seq();
            if let Some(run) = inner.runs.get_mut(&run_id) {
                if let Some(a) = run.activity_mut(session_id) {
                    a.status = if status == RunStatus::Failed {
                        "failed".to_string()
                    } else {
                        "completed".to_string()
                    };
                    a.ended_at = Some(now);
                    if let Some(t) = tokens {
                        a.tokens_total = t;
                    }
                }
                run.recompute_elapsed(now);
                if is_root {
                    run.status = status;
                    run.ended_at = Some(now);
                    let (kind, text) = match status {
                        RunStatus::Failed => (
                            "error",
                            error.clone().unwrap_or_else(|| "Falha no fluxo".to_string()),
                        ),
                        _ => ("phase", "Fluxo concluído".to_string()),
                    };
                    run.push_event(ReasoningEvent {
                        seq,
                        ts: now,
                        run_id: run_id.clone(),
                        session_id: session_id.to_string(),
                        kind: kind.to_string(),
                        text,
                        tool: None,
                        meta: json!({ "latency_ms": latency_ms }),
                    });
                    if !run.persisted {
                        run.persisted = true;
                        to_persist = Some(run.snapshot());
                    }
                }
            }
        }
        if let Some(snapshot) = to_persist {
            self.persist(snapshot).await;
        }
    }

    /// Read the latest token estimate for a session from the budget tracker.
    async fn tokens_for(&self, session_id: &str) -> Option<usize> {
        let tracker = self.budget_tracker.read().await;
        tracker
            .get(session_id)
            .map(|t| t.prompt_tokens_estimate)
    }

    /// Refresh live activity token counts from the budget tracker.
    async fn refresh_tokens(&self) {
        let tracker = self.budget_tracker.read().await;
        if tracker.is_empty() {
            return;
        }
        let mut inner = self.inner.write().await;
        for run in inner.runs.values_mut() {
            for a in &mut run.activities {
                if let Some(t) = tracker.get(&a.id) {
                    a.tokens_total = t.prompt_tokens_estimate;
                }
            }
            run.recompute_elapsed(Utc::now());
        }
    }

    async fn prune_memory(&self) {
        let mut inner = self.inner.write().await;
        while inner.order.len() > MAX_INMEM_RUNS {
            // Evict the oldest run that is no longer running.
            let candidate = inner
                .order
                .iter()
                .find(|id| {
                    inner
                        .runs
                        .get(*id)
                        .map(|r| r.status.is_terminal())
                        .unwrap_or(true)
                })
                .cloned();
            let Some(victim) = candidate else { break };
            inner.order.retain(|id| id != &victim);
            if let Some(run) = inner.runs.remove(&victim) {
                inner.session_index.retain(|_, v| v != &victim);
                // Best-effort: ensure terminal runs are persisted before eviction.
                if run.status.is_terminal() && !run.persisted {
                    let snapshot = run.snapshot();
                    drop(run);
                    drop(inner);
                    self.persist(snapshot).await;
                    return;
                }
            }
        }
    }

    // ── Snapshot / list accessors ──────────────────────────────────────────

    pub async fn global_metrics(&self) -> GlobalMetrics {
        self.refresh_tokens().await;
        let inner = self.inner.read().await;
        let now = Utc::now();
        let mut metrics = GlobalMetrics {
            total_runs: inner.runs.len(),
            ..Default::default()
        };
        let mut earliest_active: Option<DateTime<Utc>> = None;
        for run in inner.runs.values() {
            let m = run.metrics();
            metrics.total_tokens += m.tokens_total;
            metrics.total_tool_calls += m.tool_calls;
            if run.status == RunStatus::Running {
                metrics.active_runs += 1;
                earliest_active = Some(match earliest_active {
                    Some(e) => e.min(run.started_at),
                    None => run.started_at,
                });
            }
        }
        if let Some(start) = earliest_active {
            metrics.active_elapsed_ms = (now - start).num_milliseconds().max(0) as u64;
        }
        metrics
    }

    pub async fn list(&self, limit: usize) -> OrchestrationListResponse {
        let metrics = self.global_metrics().await;
        let inner = self.inner.read().await;
        let mut runs: Vec<RunSummary> = inner.runs.values().map(|r| r.summary()).collect();
        let live_ids: std::collections::HashSet<String> =
            runs.iter().map(|r| r.run_id.clone()).collect();
        drop(inner);

        // Merge persisted history not currently live.
        if let Ok(history) = self.load_history(limit).await {
            for h in history {
                if !live_ids.contains(&h.run_id) {
                    runs.push(h);
                }
            }
        }
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs.truncate(limit);
        OrchestrationListResponse { runs, metrics }
    }

    pub async fn snapshot(&self, run_id: &str) -> Option<OrchestrationRun> {
        self.refresh_tokens().await;
        {
            let inner = self.inner.read().await;
            if let Some(run) = inner.runs.get(run_id) {
                return Some(run.snapshot());
            }
        }
        self.load_run(run_id).await.ok().flatten()
    }

    /// Subscribe to a live run's SSE feed, replaying events after `after_seq`.
    ///
    /// Returns `(replay, receiver, resync)` where `resync` is true when the
    /// requested cursor is older than the retained buffer (client must reload
    /// the snapshot to avoid a gap).
    async fn subscribe(
        &self,
        run_id: &str,
        after_seq: u64,
    ) -> Option<(Vec<ReasoningEvent>, broadcast::Receiver<ReasoningEvent>, bool)> {
        let inner = self.inner.read().await;
        let run = inner.runs.get(run_id)?;
        let rx = run.tx.subscribe();
        let resync = after_seq > 0 && after_seq < run.dropped_before_seq;
        let replay: Vec<ReasoningEvent> = run
            .events
            .iter()
            .filter(|e| e.seq > after_seq)
            .cloned()
            .collect();
        Some((replay, rx, resync))
    }

    /// Attach a job id to a run so it can be cancelled via the Jobs infra.
    ///
    /// Extensibility seam: runs spawned through the Jobs infra (spec 01) can
    /// register their `JobId` here so the cancel endpoint can drive the
    /// `CancellationToken`. Direct agent runs have no backing job.
    #[allow(dead_code)]
    pub async fn attach_job(&self, run_id: &str, job_id: &str) {
        let mut inner = self.inner.write().await;
        if let Some(run) = inner.runs.get_mut(run_id) {
            run.job_id = Some(job_id.to_string());
        }
    }

    pub async fn job_id_for(&self, run_id: &str) -> Option<String> {
        let inner = self.inner.read().await;
        inner.runs.get(run_id).and_then(|r| r.job_id.clone())
    }

    /// Mark a run as cancelled (after the underlying job was cancelled).
    pub async fn mark_cancelled(&self, run_id: &str) {
        let now = Utc::now();
        let seq = self.next_seq();
        let mut to_persist = None;
        {
            let mut inner = self.inner.write().await;
            if let Some(run) = inner.runs.get_mut(run_id) {
                run.status = RunStatus::Cancelled;
                run.ended_at = Some(now);
                for a in &mut run.activities {
                    if a.status == "running" {
                        a.status = "failed".to_string();
                        a.ended_at = Some(now);
                    }
                }
                run.recompute_elapsed(now);
                let root = run.root_session_id.clone();
                run.push_event(ReasoningEvent {
                    seq,
                    ts: now,
                    run_id: run_id.to_string(),
                    session_id: root,
                    kind: "phase".to_string(),
                    text: "Fluxo cancelado".to_string(),
                    tool: None,
                    meta: Value::Null,
                });
                if !run.persisted {
                    run.persisted = true;
                    to_persist = Some(run.snapshot());
                }
            }
        }
        if let Some(snapshot) = to_persist {
            self.persist(snapshot).await;
        }
    }

    // ── SQLite persistence ──────────────────────────────────────────────────

    async fn persist(&self, snapshot: OrchestrationRun) {
        let db_path = self.db_path.clone();
        let result = tokio::task::spawn_blocking(move || persist_run(&db_path, &snapshot)).await;
        if let Ok(Err(e)) = result {
            warn!("failed to persist orchestration run: {e}");
        }
    }

    async fn load_history(&self, limit: usize) -> io::Result<Vec<RunSummary>> {
        let db_path = self.db_path.clone();
        tokio::task::spawn_blocking(move || load_history(&db_path, limit))
            .await
            .map_err(|e| io::Error::other(e.to_string()))?
    }

    async fn load_run(&self, run_id: &str) -> io::Result<Option<OrchestrationRun>> {
        let db_path = self.db_path.clone();
        let run_id = run_id.to_string();
        tokio::task::spawn_blocking(move || load_run(&db_path, &run_id))
            .await
            .map_err(|e| io::Error::other(e.to_string()))?
    }
}

// ── Free helpers ─────────────────────────────────────────────────────────────

fn touch_order(order: &mut VecDeque<String>, run_id: &str) {
    order.retain(|id| id != run_id);
    order.push_back(run_id.to_string());
}

fn flush_bucket(run: &mut RunState, session_id: &str, kind: &str, seq: u64, now: DateTime<Utc>) {
    let bucket = if kind == "answer" {
        &mut run.answer
    } else {
        &mut run.thinking
    };
    let text = match bucket.remove(session_id) {
        Some(t) if !t.trim().is_empty() => t,
        _ => return,
    };
    let run_id = run.run_id.clone();
    run.push_event(ReasoningEvent {
        seq,
        ts: now,
        run_id,
        session_id: session_id.to_string(),
        kind: kind.to_string(),
        text: sanitize(&text),
        tool: None,
        meta: Value::Null,
    });
}

fn run_label(model: &str) -> String {
    if model.trim().is_empty() {
        "Agente".to_string()
    } else {
        short_label(model)
    }
}

fn short_label(model: &str) -> String {
    let trimmed = model.trim();
    let without_prefix = trimmed.split("::").last().unwrap_or(trimmed);
    if without_prefix.is_empty() {
        "agente".to_string()
    } else {
        without_prefix.to_string()
    }
}

fn humanize_tool(tool: &str) -> String {
    match tool {
        "read_file" => "Leu um arquivo".to_string(),
        "write_file" => "Escreveu um arquivo".to_string(),
        "edit_file" => "Editou um arquivo".to_string(),
        "list_dir" | "list_directory" => "Listou um diretório".to_string(),
        "exec" | "run_command" => "Executou um comando".to_string(),
        "delegate_session" | "sessions_spawn" => {
            "Iniciou um fluxo de trabalho (delegação)".to_string()
        }
        "web_search" | "search" => "Pesquisou na web".to_string(),
        "memory_search" | "memory" => "Consultou a memória".to_string(),
        other => format!("Chamou ferramenta {other}"),
    }
}

fn truncate(text: &str, max: usize) -> String {
    let compact: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max {
        return compact;
    }
    let mut out: String = compact.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Redact common secret shapes so they never reach the feed.
fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in text.split_inclusive(|c: char| c.is_whitespace()) {
        let (word, trailing) = split_trailing_ws(token);
        if looks_secret(word) {
            out.push_str("«redigido»");
        } else {
            out.push_str(word);
        }
        out.push_str(trailing);
    }
    out
}

fn split_trailing_ws(token: &str) -> (&str, &str) {
    match token.char_indices().rev().find(|(_, c)| !c.is_whitespace()) {
        Some((idx, c)) => {
            let split = idx + c.len_utf8();
            (&token[..split], &token[split..])
        }
        None => ("", token),
    }
}

fn looks_secret(word: &str) -> bool {
    let w = word.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | ')' | '('));
    if w.len() < 16 {
        // still catch obvious prefixes
        return w.starts_with("sk-") && w.len() >= 8;
    }
    let lower = w.to_ascii_lowercase();
    if lower.starts_with("sk-")
        || lower.starts_with("ghp_")
        || lower.starts_with("xoxb-")
        || lower.starts_with("bearer")
        || lower.starts_with("eyj")
    // JWT
    {
        return true;
    }
    // Long high-entropy alnum blobs (token-like).
    let alnum = w.chars().filter(|c| c.is_ascii_alphanumeric()).count();
    w.len() >= 32 && alnum >= w.len() * 9 / 10 && w.chars().any(|c| c.is_ascii_digit())
}

// ── SQLite row helpers (blocking; called via spawn_blocking) ─────────────────

fn open_db(db_path: &Path) -> Result<Connection, io::Error> {
    let conn = Connection::open(db_path)
        .map_err(|e| io::Error::other(format!("open orchestration db: {e}")))?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        .map_err(|e| io::Error::other(format!("set pragmas: {e}")))?;
    Ok(conn)
}

fn persist_run(db_path: &Path, run: &OrchestrationRun) -> io::Result<()> {
    let conn = open_db(db_path)?;
    let snapshot_json = serde_json::to_string(run).map_err(io::Error::other)?;
    conn.execute(
        "INSERT OR REPLACE INTO orchestration_runs
         (run_id, root_session_id, label, status, started_at, ended_at,
          total_tokens, tool_calls, agents, snapshot_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            run.run_id,
            run.root_session_id,
            run.label,
            run.status.as_str(),
            run.started_at.to_rfc3339(),
            run.ended_at.map(|d| d.to_rfc3339()),
            run.metrics.tokens_total as i64,
            run.metrics.tool_calls as i64,
            run.metrics.agents as i64,
            snapshot_json,
            Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|e| io::Error::other(format!("persist run: {e}")))?;
    Ok(())
}

fn load_history(db_path: &Path, limit: usize) -> io::Result<Vec<RunSummary>> {
    let conn = open_db(db_path)?;
    let mut stmt = conn
        .prepare(
            "SELECT run_id, root_session_id, label, status, started_at, ended_at,
                    total_tokens, tool_calls, agents
             FROM orchestration_runs
             ORDER BY started_at DESC
             LIMIT ?1",
        )
        .map_err(|e| io::Error::other(format!("prepare history: {e}")))?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            let started: String = row.get(4)?;
            let ended: Option<String> = row.get(5)?;
            Ok(RunSummary {
                run_id: row.get(0)?,
                root_session_id: row.get(1)?,
                label: row.get(2)?,
                status: RunStatus::from_str(&row.get::<_, String>(3)?),
                started_at: parse_dt(&started),
                ended_at: ended.map(|s| parse_dt(&s)),
                metrics: RunMetrics {
                    tokens_total: row.get::<_, i64>(6)? as usize,
                    tool_calls: row.get::<_, i64>(7)? as usize,
                    agents: row.get::<_, i64>(8)? as usize,
                    phases: 0,
                    elapsed_ms: 0,
                },
                live: false,
            })
        })
        .map_err(|e| io::Error::other(format!("query history: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| io::Error::other(format!("row history: {e}")))?);
    }
    Ok(out)
}

fn load_run(db_path: &Path, run_id: &str) -> io::Result<Option<OrchestrationRun>> {
    let conn = open_db(db_path)?;
    let snapshot_json: Option<String> = conn
        .query_row(
            "SELECT snapshot_json FROM orchestration_runs WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .ok();
    let Some(json) = snapshot_json else {
        return Ok(None);
    };
    let mut run: OrchestrationRun = serde_json::from_str(&json).map_err(io::Error::other)?;
    run.live = false;
    Ok(Some(run))
}

fn parse_dt(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

// ── Axum handlers ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct StreamQuery {
    pub after: Option<u64>,
}

/// `GET /agent/orchestration` — active + recent runs and global metrics.
pub async fn orchestration_list(
    State(state): State<crate::AppState>,
    Query(query): Query<ListQuery>,
) -> Json<OrchestrationListResponse> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    Json(state.orchestration.list(limit).await)
}

/// `GET /agent/orchestration/metrics` — footer status bar aggregates.
pub async fn orchestration_metrics(State(state): State<crate::AppState>) -> Json<GlobalMetrics> {
    Json(state.orchestration.global_metrics().await)
}

/// `GET /agent/orchestration/{run_id}` — full snapshot (polling fallback).
pub async fn orchestration_snapshot(
    State(state): State<crate::AppState>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<OrchestrationRun>, StatusCode> {
    state
        .orchestration
        .snapshot(&run_id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `GET /agent/orchestration/{run_id}/stream` — SSE feed with replay.
pub async fn orchestration_stream(
    State(state): State<crate::AppState>,
    AxumPath(run_id): AxumPath<String>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<SseEvent, io::Error>>>, StatusCode> {
    // Cursor precedence: Last-Event-ID header (auto-reconnect) > ?after=.
    let after_seq = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .or(query.after)
        .unwrap_or(0);

    let (replay, rx, resync) = state
        .orchestration
        .subscribe(&run_id, after_seq)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    // Highest seq already delivered via replay — used to de-dupe live events.
    let mut max_sent = replay.iter().map(|e| e.seq).max().unwrap_or(after_seq);

    let mut head: Vec<Result<SseEvent, io::Error>> = Vec::new();
    if resync {
        head.push(Ok(SseEvent::default()
            .event("resync")
            .data("{\"resync\":true}")));
    }
    for ev in replay {
        head.push(Ok(reasoning_to_sse(&ev)));
    }

    let live = BroadcastStream::new(rx).filter_map(move |item| match item {
        Ok(ev) => {
            if ev.seq <= max_sent {
                None
            } else {
                max_sent = ev.seq;
                Some(Ok(reasoning_to_sse(&ev)))
            }
        }
        Err(_) => None,
    });

    let stream = tokio_stream::iter(head).chain(live);

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

fn reasoning_to_sse(ev: &ReasoningEvent) -> SseEvent {
    let data = serde_json::to_string(ev).unwrap_or_default();
    SseEvent::default()
        .id(ev.seq.to_string())
        .event("reasoning")
        .data(data)
}

#[derive(Debug, Serialize)]
pub struct CancelResponse {
    pub run_id: String,
    pub cancelled: bool,
    pub message: String,
}

/// `POST /agent/orchestration/{run_id}/cancel` — cancel via Jobs infra when
/// the run is backed by a background job; otherwise report it is not cancellable.
pub async fn orchestration_cancel(
    State(state): State<crate::AppState>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<CancelResponse>, StatusCode> {
    // The run must exist (live).
    if state.orchestration.snapshot(&run_id).await.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    match state.orchestration.job_id_for(&run_id).await {
        Some(job_id) => {
            let cancelled = state.jobs.cancel(&job_id).await.is_some();
            if cancelled {
                state.orchestration.mark_cancelled(&run_id).await;
            }
            Ok(Json(CancelResponse {
                run_id,
                cancelled,
                message: if cancelled {
                    "Job cancelado".to_string()
                } else {
                    "Job já finalizado".to_string()
                },
            }))
        }
        None => Ok(Json(CancelResponse {
            run_id,
            cancelled: false,
            message: "Run sem job associado — cancelamento indisponível".to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Arc<OrchestrationRegistry> {
        let tracker = Arc::new(RwLock::new(BTreeMap::new()));
        OrchestrationRegistry::new(tracker, PathBuf::from(":memory:"))
    }

    #[tokio::test]
    async fn run_started_creates_active_run() {
        let reg = registry();
        reg.handle_event(AgentEvent::RunStarted {
            session_id: "s1".into(),
            model: "qwen2.5:7b".into(),
        })
        .await;
        let snap = reg.snapshot("s1").await.expect("run exists");
        assert_eq!(snap.status, RunStatus::Running);
        assert_eq!(snap.phases.len(), 1);
        assert_eq!(snap.phases[0].agents.len(), 1);
        assert_eq!(snap.phases[0].agents[0].role, "main");
        assert!(snap.events.iter().any(|e| e.kind == "phase"));
    }

    #[tokio::test]
    async fn tool_calls_increment_and_complete() {
        let reg = registry();
        reg.handle_event(AgentEvent::RunStarted {
            session_id: "s1".into(),
            model: "m".into(),
        })
        .await;
        reg.handle_event(AgentEvent::ToolCallStarted {
            session_id: "s1".into(),
            tool: "read_file".into(),
            call_id: "c1".into(),
            params: json!({}),
        })
        .await;
        reg.handle_event(AgentEvent::RunCompleted {
            session_id: "s1".into(),
            latency_ms: 12,
        })
        .await;
        let snap = reg.snapshot("s1").await.unwrap();
        assert_eq!(snap.status, RunStatus::Completed);
        assert_eq!(snap.metrics.tool_calls, 1);
        assert!(snap
            .events
            .iter()
            .any(|e| e.kind == "tool_call" && e.text.contains("arquivo")));
    }

    #[tokio::test]
    async fn delegation_merges_child_into_parent() {
        let reg = registry();
        // Child runs first (as in the real delegation flow), then handoff fires.
        reg.handle_event(AgentEvent::RunStarted {
            session_id: "parent".into(),
            model: "m".into(),
        })
        .await;
        reg.handle_event(AgentEvent::RunStarted {
            session_id: "child".into(),
            model: "m".into(),
        })
        .await;
        reg.handle_event(AgentEvent::RunCompleted {
            session_id: "child".into(),
            latency_ms: 5,
        })
        .await;
        reg.handle_event(AgentEvent::SessionHandoff {
            parent_session_id: "parent".into(),
            child_session_id: "child".into(),
            handoff_summary: "resumo".into(),
        })
        .await;
        // The standalone child run is gone; parent now has a delegations phase.
        assert!(reg.snapshot("child").await.is_none() || !reg.snapshot("child").await.unwrap().live);
        let parent = reg.snapshot("parent").await.unwrap();
        assert_eq!(parent.metrics.agents, 2);
        assert!(parent.phases.iter().any(|p| p.name == "Delegações"));
    }

    #[tokio::test]
    async fn sse_replay_filters_by_cursor() {
        let reg = registry();
        reg.handle_event(AgentEvent::RunStarted {
            session_id: "s1".into(),
            model: "m".into(),
        })
        .await;
        reg.handle_event(AgentEvent::ToolCallStarted {
            session_id: "s1".into(),
            tool: "exec".into(),
            call_id: "c".into(),
            params: json!({}),
        })
        .await;
        let snap = reg.snapshot("s1").await.unwrap();
        let last = snap.last_seq;
        let (replay, _rx, resync) = reg.subscribe("s1", last).await.unwrap();
        assert!(!resync);
        assert!(replay.is_empty(), "no events after the latest cursor");
        let (replay_all, _rx2, _) = reg.subscribe("s1", 0).await.unwrap();
        assert_eq!(replay_all.len() as u64, last);
    }

    #[test]
    fn sanitize_redacts_secrets() {
        let out = sanitize("token sk-abcdef0123456789ABCDEF e fim");
        assert!(out.contains("«redigido»"), "got: {out}");
        assert!(out.contains("token") && out.contains("fim"));
        let safe = sanitize("apenas um texto normal de log");
        assert_eq!(safe, "apenas um texto normal de log");
    }
}
