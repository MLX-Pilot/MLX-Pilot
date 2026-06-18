//! Context budget management for long-running local sessions.

use crate::prompt_builder::{
    build_system_prompt, estimate_prompt_tokens, filter_tools, latest_user_text,
    ModelPromptProfile, ModelPromptProfileKind, VerbosityLevel,
};
use crate::tool_catalog::ToolProfileName;
use chrono::{DateTime, Utc};
use mlx_agent_tools::ExecutionMode;
use mlx_ollama_core::{ChatMessage, FunctionDef, MessageRole};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStyle {
    Normal,
    Short,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextSummaryArtifact {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub source_message_count: usize,
    #[serde(default)]
    pub metadata: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextBudgetTelemetry {
    pub session_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub model_profile: String,
    pub tool_profile: String,
    pub max_prompt_tokens: usize,
    pub prompt_tokens_estimate: usize,
    pub prompt_tokens_before_compression: usize,
    pub history_messages_total: usize,
    pub history_messages_used: usize,
    pub summarized_messages: usize,
    pub summary_entries: usize,
    pub tools_considered: usize,
    pub tools_in_prompt: usize,
    pub critical: bool,
    pub response_style: ResponseStyle,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ContextBudgetInput<'a> {
    pub session_id: &'a str,
    pub provider_id: &'a str,
    pub model_id: &'a str,
    pub tool_profile: ToolProfileName,
    pub execution_mode: ExecutionMode,
    pub profile: &'a ModelPromptProfile,
    pub system_prompt_override: Option<&'a str>,
    pub conversation: &'a [ChatMessage],
    pub skill_summaries: &'a [String],
    pub tools: &'a [FunctionDef],
    pub aggressive_tool_filtering: bool,
}

#[derive(Debug, Clone)]
pub struct ContextBudgetOutput {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<FunctionDef>,
    pub estimated_prompt_tokens: usize,
    pub telemetry: ContextBudgetTelemetry,
    pub summary_artifacts: Vec<ContextSummaryArtifact>,
    pub response_style: ResponseStyle,
}

#[derive(Debug, Default)]
pub struct ContextBudgetManager;

impl ContextBudgetManager {
    pub fn build(&self, input: ContextBudgetInput<'_>) -> ContextBudgetOutput {
        let mut working_history = input.conversation.to_vec();
        let max_tokens = input.profile.max_tokens_prompt.max(256);
        let keep_recent = keep_recent_messages(input.profile.kind);
        let chunk_size = compression_chunk_size(input.profile.kind);
        let mut max_tools = input.profile.max_tools_in_prompt.max(1);
        let mut summary_artifacts = Vec::new();
        let now = Utc::now();

        let initial =
            build_prompt_candidate(&working_history, &input, max_tools, ResponseStyle::Normal);
        let prompt_tokens_before_compression = initial.estimated_prompt_tokens;

        let mut final_candidate = initial;
        let mut response_style = ResponseStyle::Normal;

        for _ in 0..64 {
            let headroom = max_tokens.saturating_sub(final_candidate.estimated_prompt_tokens);
            let critical = headroom <= critical_headroom(input.profile.kind);
            if final_candidate.estimated_prompt_tokens <= max_tokens {
                if critical && max_tools > 2 {
                    max_tools = 2;
                    response_style = ResponseStyle::Short;
                    final_candidate =
                        build_prompt_candidate(&working_history, &input, max_tools, response_style);
                    continue;
                }
                if critical {
                    response_style = ResponseStyle::Short;
                    final_candidate =
                        build_prompt_candidate(&working_history, &input, max_tools, response_style);
                }
                break;
            }

            if let Some(artifact) = compress_oldest_history_chunk(
                &mut working_history,
                input.session_id,
                keep_recent,
                chunk_size,
                now,
            ) {
                summary_artifacts.push(artifact);
                response_style = ResponseStyle::Short;
                final_candidate =
                    build_prompt_candidate(&working_history, &input, max_tools, response_style);
                continue;
            }

            if max_tools > 1 {
                max_tools -= 1;
                response_style = ResponseStyle::Short;
                final_candidate =
                    build_prompt_candidate(&working_history, &input, max_tools, response_style);
                continue;
            }

            if drop_oldest_message(&mut working_history, keep_recent) {
                response_style = ResponseStyle::Short;
                final_candidate =
                    build_prompt_candidate(&working_history, &input, max_tools, response_style);
                continue;
            }

            break;
        }

        force_fit_candidate(&mut final_candidate, max_tokens);

        // Trimming above (compression, dropping the oldest message, force-fitting)
        // can leave a `tool` message without the assistant `tool_calls` it answers,
        // or an assistant `tool_calls` without its reply. OpenAI-compatible remote
        // providers (e.g. DeepSeek) reject such requests with HTTP 400
        // ("Messages with role 'tool' must be a response to a preceding message with
        // 'tool_calls'"). Repair the sequence before it leaves the budget.
        sanitize_tool_call_pairs(&mut final_candidate.messages);
        final_candidate.estimated_prompt_tokens =
            estimate_prompt_tokens(&final_candidate.messages, &final_candidate.tools);

        let critical = final_candidate.estimated_prompt_tokens
            >= max_tokens.saturating_sub(critical_headroom(input.profile.kind));

        let telemetry = ContextBudgetTelemetry {
            session_id: input.session_id.to_string(),
            provider_id: input.provider_id.to_string(),
            model_id: input.model_id.to_string(),
            model_profile: model_profile_name(input.profile.kind).to_string(),
            tool_profile: input.tool_profile.as_str().to_string(),
            max_prompt_tokens: max_tokens,
            prompt_tokens_estimate: final_candidate.estimated_prompt_tokens,
            prompt_tokens_before_compression,
            history_messages_total: input.conversation.len(),
            history_messages_used: working_history.len(),
            summarized_messages: input
                .conversation
                .len()
                .saturating_sub(working_history.len())
                + summary_artifacts.len(),
            summary_entries: summary_artifacts.len(),
            tools_considered: input.tools.len(),
            tools_in_prompt: final_candidate.tools.len(),
            critical,
            response_style,
            last_updated: now,
        };

        ContextBudgetOutput {
            messages: final_candidate.messages,
            tools: final_candidate.tools,
            estimated_prompt_tokens: final_candidate.estimated_prompt_tokens,
            telemetry,
            summary_artifacts,
            response_style,
        }
    }
}

struct PromptCandidate {
    messages: Vec<ChatMessage>,
    tools: Vec<FunctionDef>,
    estimated_prompt_tokens: usize,
}

fn build_prompt_candidate(
    conversation: &[ChatMessage],
    input: &ContextBudgetInput<'_>,
    max_tools: usize,
    response_style: ResponseStyle,
) -> PromptCandidate {
    let system_prompt = build_budgeted_system_prompt(
        input.execution_mode,
        input.profile.verbosity_level,
        input.system_prompt_override,
        input.skill_summaries,
        input.profile.max_skill_summaries,
        input.profile.max_skill_summary_chars,
        response_style,
    );

    let mut messages = Vec::with_capacity(conversation.len() + 1);
    messages.push(ChatMessage::text(MessageRole::System, system_prompt));
    messages.extend_from_slice(conversation);

    let tools = filter_tools(
        input.tools,
        input.execution_mode,
        latest_user_text(conversation),
        max_tools,
        input.profile.max_tool_description_chars,
        input.aggressive_tool_filtering || input.profile.kind != ModelPromptProfileKind::RemoteFull,
    );

    let estimated_prompt_tokens = estimate_prompt_tokens(&messages, &tools);

    PromptCandidate {
        messages,
        tools,
        estimated_prompt_tokens,
    }
}

fn force_fit_candidate(candidate: &mut PromptCandidate, max_tokens: usize) {
    while candidate.estimated_prompt_tokens > max_tokens && candidate.tools.len() > 1 {
        candidate.tools.pop();
        candidate.estimated_prompt_tokens =
            estimate_prompt_tokens(&candidate.messages, &candidate.tools);
    }

    while candidate.estimated_prompt_tokens > max_tokens && candidate.messages.len() > 2 {
        candidate.messages.remove(1);
        candidate.estimated_prompt_tokens =
            estimate_prompt_tokens(&candidate.messages, &candidate.tools);
    }

    while candidate.estimated_prompt_tokens > max_tokens && !candidate.tools.is_empty() {
        candidate.tools.pop();
        candidate.estimated_prompt_tokens =
            estimate_prompt_tokens(&candidate.messages, &candidate.tools);
    }

    if candidate.estimated_prompt_tokens <= max_tokens {
        return;
    }

    for message in candidate.messages.iter_mut().skip(1) {
        message.content = compact_preview(&message.content, 96);
        message.tool_calls.clear();
    }
    candidate.estimated_prompt_tokens =
        estimate_prompt_tokens(&candidate.messages, &candidate.tools);
}

/// Repair the message sequence so it satisfies OpenAI-compatible tool-call
/// pairing rules after budget trimming may have dropped or reordered messages:
/// every `tool` message must follow the assistant message that requested it,
/// and every assistant `tool_calls` entry must have a matching `tool` reply.
fn sanitize_tool_call_pairs(messages: &mut Vec<ChatMessage>) {
    use std::collections::HashSet;

    // Pass 1: drop orphaned tool replies whose requesting assistant message was
    // trimmed away. `valid_ids` holds the tool-call ids requested by the most
    // recent assistant message (reset whenever a non-tool message intervenes).
    let mut valid_ids: HashSet<String> = HashSet::new();
    let mut kept: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    for message in messages.drain(..) {
        match message.role {
            MessageRole::Assistant => {
                valid_ids = message.tool_calls.iter().map(|c| c.id.clone()).collect();
                kept.push(message);
            }
            MessageRole::Tool => {
                let matched = message
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| valid_ids.contains(id));
                if matched {
                    kept.push(message);
                }
                // Otherwise the orphaned tool reply is dropped.
            }
            _ => {
                valid_ids.clear();
                kept.push(message);
            }
        }
    }
    *messages = kept;

    // Pass 2: drop assistant tool-call requests that never received a reply (their
    // `tool` message was trimmed), since those also trigger a 400 on strict APIs.
    let len = messages.len();
    for idx in 0..len {
        if messages[idx].role != MessageRole::Assistant || messages[idx].tool_calls.is_empty() {
            continue;
        }
        let mut answered: HashSet<String> = HashSet::new();
        let mut next = idx + 1;
        while next < len && messages[next].role == MessageRole::Tool {
            if let Some(id) = &messages[next].tool_call_id {
                answered.insert(id.clone());
            }
            next += 1;
        }
        messages[idx].tool_calls.retain(|c| answered.contains(&c.id));
    }

    // Pass 3: drop assistant messages left with neither content nor tool calls,
    // which are themselves invalid for OpenAI-compatible providers.
    messages.retain(|m| {
        m.role != MessageRole::Assistant
            || !m.tool_calls.is_empty()
            || !m.content.trim().is_empty()
    });
}

fn compress_oldest_history_chunk(
    history: &mut Vec<ChatMessage>,
    session_id: &str,
    keep_recent: usize,
    chunk_size: usize,
    created_at: DateTime<Utc>,
) -> Option<ContextSummaryArtifact> {
    if history.len() <= keep_recent.saturating_add(1) {
        return None;
    }

    let available = history.len().saturating_sub(keep_recent);
    let end = available.min(chunk_size.max(2));
    if end == 0 {
        return None;
    }

    let chunk = history.drain(0..end).collect::<Vec<_>>();
    let summary = summarize_messages(&chunk);
    let title = format!("Session {} summary ({end} msgs)", session_id);
    let id = summary_id(session_id, &summary);

    history.insert(
        0,
        ChatMessage::text(
            MessageRole::Assistant,
            format!(
                "[history-summary:{id}] {summary}\nUse audit/session history for exact details."
            ),
        ),
    );

    Some(ContextSummaryArtifact {
        id,
        session_id: session_id.to_string(),
        title,
        content: summary,
        created_at,
        source_message_count: end,
        metadata: std::collections::BTreeMap::from([(
            "kind".to_string(),
            "history_summary".to_string(),
        )]),
    })
}

fn summarize_messages(messages: &[ChatMessage]) -> String {
    let mut parts = Vec::new();
    parts.push(format!(
        "Earlier context compressed from {} messages.",
        messages.len()
    ));

    for message in messages.iter().take(8) {
        let role = match message.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        let detail = if !message.tool_calls.is_empty() {
            let names = message
                .tool_calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("requested tools: {names}")
        } else {
            compact_preview(&message.content, 120)
        };
        if !detail.is_empty() {
            parts.push(format!("{role}: {detail}"));
        }
    }

    parts.join(" ")
}

fn drop_oldest_message(history: &mut Vec<ChatMessage>, keep_recent: usize) -> bool {
    if history.len() <= keep_recent.saturating_add(1) {
        return false;
    }
    history.remove(0);
    true
}

fn build_budgeted_system_prompt(
    mode: ExecutionMode,
    verbosity: VerbosityLevel,
    system_override: Option<&str>,
    skill_summaries: &[String],
    max_skill_summaries: usize,
    max_skill_summary_chars: usize,
    response_style: ResponseStyle,
) -> String {
    let mut prompt = build_system_prompt(
        mode,
        verbosity,
        system_override,
        skill_summaries,
        max_skill_summaries,
        max_skill_summary_chars,
    );

    if matches!(response_style, ResponseStyle::Short) {
        prompt.push_str(
            "\nBudget directive: prefer short answers, avoid repetition, and only request tools when necessary.",
        );
    }

    prompt
}

fn compact_preview(value: &str, max_chars: usize) -> String {
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

fn summary_id(session_id: &str, content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update(b":");
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::from("mem-");
    for byte in digest.into_iter().take(8) {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn keep_recent_messages(kind: ModelPromptProfileKind) -> usize {
    match kind {
        ModelPromptProfileKind::SmallLocal => 4,
        ModelPromptProfileKind::MidLocal => 6,
        ModelPromptProfileKind::LargeLocal => 8,
        ModelPromptProfileKind::RemoteFull => 12,
    }
}

fn compression_chunk_size(kind: ModelPromptProfileKind) -> usize {
    match kind {
        ModelPromptProfileKind::SmallLocal => 6,
        ModelPromptProfileKind::MidLocal => 8,
        ModelPromptProfileKind::LargeLocal => 10,
        ModelPromptProfileKind::RemoteFull => 12,
    }
}

fn critical_headroom(kind: ModelPromptProfileKind) -> usize {
    match kind {
        ModelPromptProfileKind::SmallLocal => 96,
        ModelPromptProfileKind::MidLocal => 128,
        ModelPromptProfileKind::LargeLocal => 192,
        ModelPromptProfileKind::RemoteFull => 256,
    }
}

fn model_profile_name(kind: ModelPromptProfileKind) -> &'static str {
    match kind {
        ModelPromptProfileKind::SmallLocal => "small_local",
        ModelPromptProfileKind::MidLocal => "mid_local",
        ModelPromptProfileKind::LargeLocal => "large_local",
        ModelPromptProfileKind::RemoteFull => "remote_full",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt_builder::{ModelPromptProfile, ModelPromptProfileKind};

    fn mk_message(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage::text(role, content.to_string())
    }

    fn mk_tool_call(id: &str, name: &str) -> mlx_ollama_core::ToolCallRequest {
        mlx_ollama_core::ToolCallRequest {
            id: id.to_string(),
            name: name.to_string(),
            arguments: "{}".to_string(),
        }
    }

    #[test]
    fn sanitize_drops_orphaned_tool_reply() {
        // Simulates force-fit having removed the assistant message that requested
        // the first tool call, leaving an orphaned tool reply at the front.
        let mut messages = vec![
            mk_message(MessageRole::System, "sys"),
            ChatMessage::tool_result("call-1", "stale tool output"),
            ChatMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                tool_calls: vec![mk_tool_call("call-2", "list_dir")],
                tool_call_id: None,
            },
            ChatMessage::tool_result("call-2", "fresh tool output"),
        ];

        sanitize_tool_call_pairs(&mut messages);

        // The orphan is gone; the valid pair survives in order.
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(messages[1].role, MessageRole::Assistant);
        assert_eq!(messages[1].tool_calls.len(), 1);
        assert_eq!(messages[2].role, MessageRole::Tool);
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call-2"));
    }

    #[test]
    fn sanitize_drops_unanswered_tool_call() {
        // Assistant requested a tool whose reply was trimmed away.
        let mut messages = vec![
            mk_message(MessageRole::System, "sys"),
            mk_message(MessageRole::User, "hi"),
            ChatMessage {
                role: MessageRole::Assistant,
                content: "working on it".to_string(),
                tool_calls: vec![mk_tool_call("call-9", "read_file")],
                tool_call_id: None,
            },
        ];

        sanitize_tool_call_pairs(&mut messages);

        // The dangling tool_call is stripped; the assistant text is preserved.
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2].role, MessageRole::Assistant);
        assert!(messages[2].tool_calls.is_empty());
        assert_eq!(messages[2].content, "working on it");
    }

    fn mk_tool(name: &str) -> FunctionDef {
        FunctionDef {
            name: name.to_string(),
            description: format!("tool {name}"),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
        }
    }

    #[test]
    fn compresses_long_history_without_exceeding_budget() {
        let profile = ModelPromptProfile::for_kind(ModelPromptProfileKind::SmallLocal)
            .apply_overrides(Some(320), Some(100), Some(4));
        let mut conversation = Vec::new();
        for idx in 0..120 {
            let role = if idx % 2 == 0 {
                MessageRole::User
            } else {
                MessageRole::Assistant
            };
            conversation.push(mk_message(
                role,
                "This is a deliberately long message that should be summarized when the context budget gets tight.",
            ));
        }

        let manager = ContextBudgetManager;
        let output = manager.build(ContextBudgetInput {
            session_id: "sess-1",
            provider_id: "ollama",
            model_id: "qwen2.5:7b",
            tool_profile: ToolProfileName::Coding,
            execution_mode: ExecutionMode::Full,
            profile: &profile,
            system_prompt_override: None,
            conversation: &conversation,
            skill_summaries: &[],
            tools: &[mk_tool("read_file"), mk_tool("exec"), mk_tool("write_file")],
            aggressive_tool_filtering: true,
        });

        assert!(output.estimated_prompt_tokens <= profile.max_tokens_prompt);
        assert!(!output.summary_artifacts.is_empty());
        assert!(matches!(output.response_style, ResponseStyle::Short));
    }
}
