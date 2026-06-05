//! `EventBus` — broadcast channel for agent events (lifecycle,
//! streaming, tool calls, approvals, skills, audit).

use crate::approval::ApprovalRequest;
use crate::audit::AuditLogEntry;
use serde::Serialize;
use tokio::sync::broadcast;

/// All events the agent can emit.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    // ── Lifecycle ──
    RunStarted {
        session_id: String,
        model: String,
    },
    SessionIngress {
        session_id: String,
        origin_kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_channel: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        correlation_id: Option<String>,
    },
    RunCompleted {
        session_id: String,
        latency_ms: u64,
    },
    SessionSummaryUpdated {
        session_id: String,
        summary: String,
    },
    RunFailed {
        session_id: String,
        error: String,
    },

    // ── Streaming ──
    TextDelta {
        session_id: String,
        delta: String,
    },
    ThinkingDelta {
        session_id: String,
        delta: String,
    },

    // ── Tool execution ──
    ToolCallStarted {
        session_id: String,
        tool: String,
        call_id: String,
        params: serde_json::Value,
    },
    ToolCallCompleted {
        session_id: String,
        tool: String,
        call_id: String,
        result: String,
        result_preview: String,
    },
    ToolCallDenied {
        session_id: String,
        tool: String,
        reason: String,
    },
    SessionHandoff {
        parent_session_id: String,
        child_session_id: String,
        handoff_summary: String,
    },

    // ── Approvals ──
    ApprovalRequired {
        request: ApprovalRequest,
    },
    ApprovalReceived {
        id: String,
        decision: String,
    },

    // ── Skills ──
    SkillLoaded {
        name: String,
        source: String,
    },
    SkillRejected {
        name: String,
        reason: String,
    },

    // ── Audit ──
    AuditEntry {
        entry: AuditLogEntry,
    },
}

/// Broadcast-based event bus for the agent.
pub struct EventBus {
    sender: broadcast::Sender<AgentEvent>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Emit an event to all subscribers.
    pub fn emit(&self, event: AgentEvent) {
        // Ignore send errors (no subscribers).
        let _ = self.sender.send(event);
    }

    /// Subscribe to the event bus.
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_bus_emits_without_subscribers() {
        let bus = EventBus::new(16);
        // Should not panic even without subscribers.
        bus.emit(AgentEvent::RunStarted {
            session_id: "s1".into(),
            model: "test".into(),
        });
    }

    #[tokio::test]
    async fn event_bus_subscriber_receives() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        bus.emit(AgentEvent::RunStarted {
            session_id: "s1".into(),
            model: "test".into(),
        });
        let event = rx.recv().await.unwrap();
        match event {
            AgentEvent::RunStarted { session_id, .. } => assert_eq!(session_id, "s1"),
            _ => panic!("unexpected event"),
        }
    }
}
