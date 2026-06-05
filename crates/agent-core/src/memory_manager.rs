//! Memory-aware prompt hydration and lifecycle helpers.

use crate::agent_loop::AgentResponse;
use crate::agent_runtime::MemorySnapshotMode;
use crate::memory::{MemoryPromotionDecision, MemoryRecord, MemorySearchHit, MemoryStore};
use crate::session::{SessionSnapshot, SessionStore};
use crate::session_recall::{SessionRecall, SessionRecallHit};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryContextBlock {
    pub text: String,
    #[serde(default)]
    pub memory_hits: Vec<MemorySearchHit>,
    #[serde(default)]
    pub session_hits: Vec<SessionRecallHit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SessionSnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryLifecycleResult {
    pub summary: String,
    pub summary_json: Option<serde_json::Value>,
    pub snapshot_text: String,
    pub snapshot_json: Option<serde_json::Value>,
    pub promotions: Vec<MemoryPromotionDecision>,
    pub artifact_records: Vec<MemoryPromotionDecision>,
}

#[derive(Clone)]
pub struct MemoryManager {
    memory: Arc<MemoryStore>,
    sessions: Arc<SessionStore>,
    session_recall: SessionRecall,
}

impl MemoryManager {
    pub fn new(memory: Arc<MemoryStore>, sessions: Arc<SessionStore>) -> Self {
        Self {
            memory,
            sessions: sessions.clone(),
            session_recall: SessionRecall::new(sessions),
        }
    }

    pub async fn hydrate_context(
        &self,
        query: &str,
        current_session_id: &str,
        profile: &str,
        session_search_enabled: bool,
        snapshot_mode: &MemorySnapshotMode,
    ) -> io::Result<MemoryContextBlock> {
        let (memory_limit, session_limit) = match profile.trim().to_ascii_lowercase().as_str() {
            "minimal" => (2, 1),
            "full" => (6, 4),
            _ => (4, 3),
        };

        let snapshot = if *snapshot_mode == MemorySnapshotMode::Off {
            None
        } else {
            self.sessions.latest_snapshot(current_session_id).await?
        };

        let memory_hits = self.memory.search(query, memory_limit).await?;
        let session_hits = if session_search_enabled {
            self.session_recall
                .search(query, Some(current_session_id), session_limit)
                .await?
        } else {
            Vec::new()
        };

        Ok(MemoryContextBlock {
            text: render_memory_block(snapshot.as_ref(), &memory_hits, &session_hits),
            memory_hits,
            session_hits,
            snapshot,
        })
    }

    pub async fn capture_turn_result(
        &self,
        session_id: &str,
        user_message: &str,
        response: &AgentResponse,
        memory_context: &MemoryContextBlock,
        snapshot_mode: &MemorySnapshotMode,
    ) -> io::Result<MemoryLifecycleResult> {
        let summary = summarize_turn(user_message, &response.content);
        let summary_json = Some(json!({
            "session_id": session_id,
            "user_message_preview": preview(user_message, 140),
            "assistant_preview": preview(&response.content, 220),
            "memory_hits": memory_context.memory_hits.len(),
            "session_hits": memory_context.session_hits.len(),
            "iterations": response.iterations,
            "tool_calls_made": response.tool_calls_made,
        }));

        let snapshot_text = if *snapshot_mode == MemorySnapshotMode::Off {
            String::new()
        } else {
            snapshot_session_context(&summary, memory_context, response)
        };
        let snapshot_json = if snapshot_text.trim().is_empty() {
            None
        } else {
            Some(json!({
                "summary": summary,
                "memory_ids": memory_context.memory_hits.iter().map(|hit| hit.id.clone()).collect::<Vec<_>>(),
                "session_ids": memory_context.session_hits.iter().map(|hit| hit.session_id.clone()).collect::<Vec<_>>(),
            }))
        };

        let promotions = promote_memory(session_id, &summary, response, memory_context);
        let artifact_records = response
            .summary_artifacts
            .iter()
            .map(|artifact| MemoryPromotionDecision {
                reason: "context_budget_artifact".to_string(),
                record: MemoryRecord {
                    id: artifact.id.clone(),
                    session_id: artifact.session_id.clone(),
                    source_session_id: artifact.session_id.clone(),
                    scope: "session".to_string(),
                    namespace: "history".to_string(),
                    kind: artifact
                        .metadata
                        .get("kind")
                        .cloned()
                        .unwrap_or_else(|| "history_summary".to_string()),
                    title: artifact.title.clone(),
                    content: artifact.content.clone(),
                    tags: Vec::new(),
                    metadata: artifact.metadata.clone(),
                    importance: 50,
                    created_at: artifact.created_at,
                    last_accessed_at: Some(Utc::now()),
                    pin_state: "auto".to_string(),
                    promotion_source: "context_budget".to_string(),
                    summary_ref: String::new(),
                },
            })
            .collect();

        Ok(MemoryLifecycleResult {
            summary,
            summary_json,
            snapshot_text,
            snapshot_json,
            promotions,
            artifact_records,
        })
    }
}

fn render_memory_block(
    snapshot: Option<&SessionSnapshot>,
    memory_hits: &[MemorySearchHit],
    session_hits: &[SessionRecallHit],
) -> String {
    if snapshot.is_none() && memory_hits.is_empty() && session_hits.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "## Recalled Local Memory".to_string(),
        "Use this as local context when relevant. It may be incomplete; verify before acting."
            .to_string(),
        String::new(),
    ];

    if let Some(snapshot) = snapshot.filter(|value| !value.text.trim().is_empty()) {
        lines.push("### Latest Session Snapshot".to_string());
        lines.push(snapshot.text.trim().to_string());
        lines.push(String::new());
    }

    if !memory_hits.is_empty() {
        lines.push("### Long-Term Memory".to_string());
        for hit in memory_hits {
            lines.push(format!(
                "- [{}:{}] {}",
                hit.scope,
                hit.namespace,
                hit.preview.trim()
            ));
        }
        lines.push(String::new());
    }

    if !session_hits.is_empty() {
        lines.push("### Related Past Sessions".to_string());
        for hit in session_hits {
            lines.push(format!("- [{}] {}", hit.name, hit.preview.trim()));
        }
        lines.push(String::new());
    }

    lines.join("\n").trim().to_string()
}

fn summarize_turn(user_message: &str, assistant_message: &str) -> String {
    format!(
        "User asked: {}. Assistant outcome: {}",
        preview(user_message, 140),
        preview(assistant_message, 220)
    )
}

fn snapshot_session_context(
    summary: &str,
    memory_context: &MemoryContextBlock,
    response: &AgentResponse,
) -> String {
    let mut lines = vec![
        "## Session Snapshot".to_string(),
        summary.to_string(),
        format!(
            "Turn stats: {} iterations, {} tool calls.",
            response.iterations, response.tool_calls_made
        ),
    ];

    if !memory_context.memory_hits.is_empty() {
        lines.push("Reused memory:".to_string());
        lines.extend(
            memory_context
                .memory_hits
                .iter()
                .take(3)
                .map(|hit| format!("- {} ({})", hit.title, hit.scope)),
        );
    }

    if !memory_context.session_hits.is_empty() {
        lines.push("Related sessions:".to_string());
        lines.extend(
            memory_context
                .session_hits
                .iter()
                .take(2)
                .map(|hit| format!("- {}", hit.name)),
        );
    }

    lines.join("\n")
}

fn promote_memory(
    session_id: &str,
    summary: &str,
    response: &AgentResponse,
    memory_context: &MemoryContextBlock,
) -> Vec<MemoryPromotionDecision> {
    let mut metadata = BTreeMap::new();
    metadata.insert("kind".to_string(), "session_summary".to_string());
    metadata.insert("iterations".to_string(), response.iterations.to_string());
    metadata.insert(
        "tool_calls_made".to_string(),
        response.tool_calls_made.to_string(),
    );
    metadata.insert(
        "memory_hits".to_string(),
        memory_context.memory_hits.len().to_string(),
    );
    metadata.insert(
        "session_hits".to_string(),
        memory_context.session_hits.len().to_string(),
    );

    let importance = if response.tool_calls_made > 0 { 70 } else { 45 };
    let record = MemoryRecord {
        id: format!("mem-{}", uuid::Uuid::new_v4()),
        session_id: session_id.to_string(),
        source_session_id: session_id.to_string(),
        scope: if response.tool_calls_made > 0 {
            "long_term".to_string()
        } else {
            "session".to_string()
        },
        namespace: "history".to_string(),
        kind: "session_summary".to_string(),
        title: format!("Session summary {}", preview(session_id, 12)),
        content: summary.to_string(),
        tags: vec!["summary".to_string(), "hermes_inspired".to_string()],
        created_at: Utc::now(),
        metadata,
        importance,
        last_accessed_at: Some(Utc::now()),
        pin_state: if response.tool_calls_made > 0 {
            "candidate".to_string()
        } else {
            "auto".to_string()
        },
        promotion_source: "turn_summary".to_string(),
        summary_ref: session_id.to_string(),
    };

    vec![MemoryPromotionDecision {
        record,
        reason: "turn_summary".to_string(),
    }]
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
