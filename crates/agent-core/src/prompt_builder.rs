//! Prompt engineering utilities:
//! model-aware profiles, context budgeting, and tool filtering.

use mlx_agent_tools::ExecutionMode;
use mlx_ollama_core::{ChatMessage, FunctionDef, MessageRole};
use serde_json::{Map, Value};
use std::collections::HashSet;

const CHARS_PER_TOKEN_APPROX: usize = 4;
const MIN_PROMPT_TOKENS: usize = 256;

const IDENTITY_SECTION: &str =
    "You are MLX-Pilot Agent. Be precise, safe, and concise. Use tools when they improve accuracy.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbosityLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPromptProfileKind {
    SmallLocal,
    MidLocal,
    LargeLocal,
    RemoteFull,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelPromptProfile {
    pub kind: ModelPromptProfileKind,
    pub max_tokens_prompt: usize,
    pub max_history_messages: usize,
    pub max_tools_in_prompt: usize,
    pub max_tool_description_chars: usize,
    pub max_skill_summaries: usize,
    pub max_skill_summary_chars: usize,
    pub verbosity_level: VerbosityLevel,
    pub temperature_default: f32,
}

impl ModelPromptProfile {
    pub fn for_kind(kind: ModelPromptProfileKind) -> Self {
        match kind {
            ModelPromptProfileKind::SmallLocal => Self {
                kind,
                max_tokens_prompt: 1200,
                max_history_messages: 8,
                max_tools_in_prompt: 3,
                max_tool_description_chars: 120,
                max_skill_summaries: 6,
                max_skill_summary_chars: 120,
                verbosity_level: VerbosityLevel::Low,
                temperature_default: 0.1,
            },
            ModelPromptProfileKind::MidLocal => Self {
                kind,
                max_tokens_prompt: 2200,
                max_history_messages: 14,
                max_tools_in_prompt: 5,
                max_tool_description_chars: 160,
                max_skill_summaries: 10,
                max_skill_summary_chars: 140,
                verbosity_level: VerbosityLevel::Low,
                temperature_default: 0.1,
            },
            ModelPromptProfileKind::LargeLocal => Self {
                kind,
                max_tokens_prompt: 3800,
                max_history_messages: 22,
                max_tools_in_prompt: 8,
                max_tool_description_chars: 220,
                max_skill_summaries: 14,
                max_skill_summary_chars: 160,
                verbosity_level: VerbosityLevel::Medium,
                temperature_default: 0.15,
            },
            ModelPromptProfileKind::RemoteFull => Self {
                kind,
                max_tokens_prompt: 7000,
                max_history_messages: 40,
                max_tools_in_prompt: 16,
                max_tool_description_chars: 320,
                max_skill_summaries: 24,
                max_skill_summary_chars: 220,
                verbosity_level: VerbosityLevel::High,
                temperature_default: 0.2,
            },
        }
    }

    pub fn apply_overrides(
        mut self,
        max_prompt_tokens: Option<usize>,
        max_history_messages: Option<usize>,
        max_tools_in_prompt: Option<usize>,
    ) -> Self {
        if let Some(v) = max_prompt_tokens {
            self.max_tokens_prompt = v.max(MIN_PROMPT_TOKENS);
        }
        if let Some(v) = max_history_messages {
            self.max_history_messages = v.max(1);
        }
        if let Some(v) = max_tools_in_prompt {
            self.max_tools_in_prompt = v.max(1);
        }
        self
    }
}

pub fn select_model_prompt_profile(provider_id: &str, model_id: &str) -> ModelPromptProfile {
    let provider = provider_id.trim().to_lowercase();
    let model = model_id.trim().to_lowercase();

    let local_provider = matches!(
        provider.as_str(),
        "mlx" | "llamacpp" | "llama.cpp" | "ollama"
    );

    if !local_provider {
        return ModelPromptProfile::for_kind(ModelPromptProfileKind::RemoteFull);
    }

    let size_b = extract_model_size_billion(&model);
    match size_b {
        Some(v) if v <= 8.0 => ModelPromptProfile::for_kind(ModelPromptProfileKind::SmallLocal),
        Some(v) if v <= 13.0 => ModelPromptProfile::for_kind(ModelPromptProfileKind::MidLocal),
        Some(v) if v >= 30.0 => ModelPromptProfile::for_kind(ModelPromptProfileKind::LargeLocal),
        Some(_) => ModelPromptProfile::for_kind(ModelPromptProfileKind::MidLocal),
        None => ModelPromptProfile::for_kind(ModelPromptProfileKind::MidLocal),
    }
}

#[derive(Debug, Clone)]
pub struct PromptBuildInput {
    pub system_prompt_override: Option<String>,
    pub execution_mode: ExecutionMode,
    pub profile: ModelPromptProfile,
    pub conversation: Vec<ChatMessage>,
    pub skill_summaries: Vec<String>,
    pub tools: Vec<FunctionDef>,
    pub aggressive_tool_filtering: bool,
}

#[derive(Debug, Clone)]
pub struct PromptBuildOutput {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<FunctionDef>,
    pub estimated_prompt_tokens: usize,
}

#[derive(Debug, Default)]
pub struct PromptBuilder;

impl PromptBuilder {
    pub fn build(&self, input: PromptBuildInput) -> PromptBuildOutput {
        let last_user_text = input
            .conversation
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or_default()
            .to_string();

        let system_prompt = build_system_prompt(
            input.execution_mode,
            input.profile.verbosity_level,
            input.system_prompt_override.as_deref(),
            &input.skill_summaries,
            input.profile.max_skill_summaries,
            input.profile.max_skill_summary_chars,
        );

        let mut messages = Vec::with_capacity(input.conversation.len() + 1);
        messages.push(ChatMessage::text(MessageRole::System, system_prompt));
        messages.extend(input.conversation);
        messages = apply_sliding_window(messages, input.profile.max_history_messages);

        let aggressive = input.aggressive_tool_filtering
            || input.profile.kind != ModelPromptProfileKind::RemoteFull;
        let mut tools = filter_tools(
            input.tools,
            input.execution_mode,
            &last_user_text,
            input.profile.max_tools_in_prompt,
            input.profile.max_tool_description_chars,
            aggressive,
        );

        enforce_prompt_budget(
            &mut messages,
            &mut tools,
            input.profile.max_tokens_prompt.max(MIN_PROMPT_TOKENS),
        );

        PromptBuildOutput {
            estimated_prompt_tokens: estimate_prompt_tokens(&messages, &tools),
            messages,
            tools,
        }
    }

    pub fn should_force_tool_call(user_message: &str, available_tools: &[FunctionDef]) -> bool {
        if available_tools.is_empty() {
            return false;
        }

        let text = user_message.to_lowercase();
        [
            "file",
            "folder",
            "directory",
            "workspace",
            "read",
            "write",
            "edit",
            "replace",
            "run",
            "exec",
            "command",
            "build",
            "test",
            "cargo",
            "npm",
            "shell",
            "list",
            "show",
        ]
        .iter()
        .any(|k| text.contains(k))
    }

    pub fn tool_call_reprompt(tool_names: &[String]) -> String {
        let names = if tool_names.is_empty() {
            "[]".to_string()
        } else {
            format!("[{}]", tool_names.join(", "))
        };

        format!(
            "If a tool is needed, respond with exactly one tool call (no prose). Allowed tools: {names}."
        )
    }
}

pub fn estimate_prompt_tokens(messages: &[ChatMessage], tools: &[FunctionDef]) -> usize {
    let msg_tokens = messages
        .iter()
        .map(estimate_message_tokens)
        .sum::<usize>()
        .saturating_add(messages.len() * 6);

    let tool_tokens = tools
        .iter()
        .map(|tool| {
            estimate_text_tokens(&tool.name)
                + estimate_text_tokens(&tool.description)
                + estimate_json_tokens(&tool.parameters)
                + 8
        })
        .sum::<usize>();

    msg_tokens.saturating_add(tool_tokens)
}

fn estimate_message_tokens(message: &ChatMessage) -> usize {
    let role_tokens: usize = 3;
    let content_tokens = estimate_text_tokens(&message.content);
    let tool_calls_tokens = message
        .tool_calls
        .iter()
        .map(|tc| estimate_text_tokens(&tc.name) + estimate_text_tokens(&tc.arguments) + 8)
        .sum::<usize>();
    role_tokens
        .saturating_add(content_tokens)
        .saturating_add(tool_calls_tokens)
}

fn estimate_text_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    chars.div_ceil(CHARS_PER_TOKEN_APPROX).max(1)
}

fn estimate_json_tokens(v: &Value) -> usize {
    serde_json::to_string(v)
        .map(|json| estimate_text_tokens(&json))
        .unwrap_or(8)
}

fn build_system_prompt(
    mode: ExecutionMode,
    verbosity: VerbosityLevel,
    system_override: Option<&str>,
    skill_summaries: &[String],
    max_skill_summaries: usize,
    max_skill_summary_chars: usize,
) -> String {
    let mut lines = Vec::new();
    lines.push(IDENTITY_SECTION.to_string());
    lines.push(runtime_rules(mode, verbosity));

    if let Some(text) = system_override {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            lines.push(format!("Runtime override: {trimmed}"));
        }
    }

    if !skill_summaries.is_empty() && max_skill_summaries > 0 {
        lines.push("Skills (compact):".to_string());
        for line in skill_summaries.iter().take(max_skill_summaries) {
            let compact = truncate_chars(line.trim(), max_skill_summary_chars);
            if compact.is_empty() {
                continue;
            }
            lines.push(format!("- {compact}"));
        }
    }

    lines.join("\n")
}

fn runtime_rules(mode: ExecutionMode, verbosity: VerbosityLevel) -> String {
    let mut rules = vec![
        "Rules: Do not invent tool outputs.".to_string(),
        "Call tools with valid JSON arguments only when needed.".to_string(),
        "Prefer shortest correct answer.".to_string(),
    ];

    match mode {
        ExecutionMode::Locked => {
            rules.push("Tool execution is locked; answer without tools.".to_string())
        }
        ExecutionMode::ReadOnly => {
            rules.push("Do not request write or exec operations.".to_string())
        }
        ExecutionMode::DryRun => {
            rules.push("Mutations are simulated; explain dry-run outcomes clearly.".to_string())
        }
        ExecutionMode::Full => {}
    }

    match verbosity {
        VerbosityLevel::Low => {
            rules.push("Keep internal reasoning and outputs compact.".to_string())
        }
        VerbosityLevel::Medium => {}
        VerbosityLevel::High => {
            rules.push("Provide complete output when confidence is high.".to_string())
        }
    }

    rules.join(" ")
}

fn apply_sliding_window(
    messages: Vec<ChatMessage>,
    max_history_messages: usize,
) -> Vec<ChatMessage> {
    if messages.is_empty() {
        return messages;
    }

    let mut out = Vec::new();
    let mut start = 0;

    if matches!(messages.first().map(|m| m.role), Some(MessageRole::System)) {
        out.push(messages[0].clone());
        start = 1;
    }

    let history = &messages[start..];
    if history.len() <= max_history_messages {
        out.extend_from_slice(history);
        return out;
    }

    out.extend_from_slice(&history[history.len().saturating_sub(max_history_messages)..]);
    out
}

fn enforce_prompt_budget(
    messages: &mut Vec<ChatMessage>,
    tools: &mut Vec<FunctionDef>,
    max_tokens: usize,
) {
    if max_tokens == 0 {
        messages.clear();
        tools.clear();
        return;
    }

    while estimate_prompt_tokens(messages, tools) > max_tokens && tools.len() > 1 {
        tools.pop();
    }

    while estimate_prompt_tokens(messages, tools) > max_tokens && drop_oldest_non_system(messages) {
    }

    while estimate_prompt_tokens(messages, tools) > max_tokens && !tools.is_empty() {
        tools.pop();
    }

    if estimate_prompt_tokens(messages, tools) <= max_tokens {
        return;
    }

    truncate_messages_in_place(messages, max_tokens, tools);
}

fn drop_oldest_non_system(messages: &mut Vec<ChatMessage>) -> bool {
    if let Some(idx) = messages
        .iter()
        .position(|m| !matches!(m.role, MessageRole::System))
    {
        messages.remove(idx);
        return true;
    }
    false
}

fn truncate_messages_in_place(
    messages: &mut [ChatMessage],
    max_tokens: usize,
    tools: &[FunctionDef],
) {
    if messages.is_empty() {
        return;
    }

    let mut remaining = max_tokens.saturating_sub(
        tools
            .iter()
            .map(|t| estimate_text_tokens(&t.name) + estimate_text_tokens(&t.description))
            .sum::<usize>(),
    );
    if remaining == 0 {
        remaining = 1;
    }

    let per_message_budget = (remaining / messages.len()).max(12);
    for message in messages.iter_mut() {
        let current = estimate_message_tokens(message);
        if current > per_message_budget {
            message.content =
                truncate_to_token_budget(&message.content, per_message_budget.saturating_sub(6));
            message.tool_calls.clear();
        }
    }
}

fn filter_tools(
    tools: Vec<FunctionDef>,
    mode: ExecutionMode,
    user_text: &str,
    max_tools: usize,
    max_desc_chars: usize,
    aggressive: bool,
) -> Vec<FunctionDef> {
    if matches!(mode, ExecutionMode::Locked) || tools.is_empty() || max_tools == 0 {
        return Vec::new();
    }

    let query = user_text.to_lowercase();
    let selected_names = if aggressive {
        Some(select_relevant_tool_names(&query))
    } else {
        None
    };

    let mut filtered = Vec::new();
    for tool in tools {
        if !is_tool_enabled_for_mode(&tool.name, mode) {
            continue;
        }
        if let Some(selected) = selected_names.as_ref() {
            if !selected.is_empty() && !selected.contains(tool.name.as_str()) {
                continue;
            }
        }

        filtered.push(FunctionDef {
            name: tool.name,
            description: compact_description(&tool.description, max_desc_chars),
            parameters: compact_schema(&tool.parameters),
        });

        if filtered.len() >= max_tools {
            break;
        }
    }

    filtered
}

fn select_relevant_tool_names(query: &str) -> HashSet<&'static str> {
    let mut selected = HashSet::new();

    if query.is_empty() {
        selected.insert("read_file");
        selected.insert("list_dir");
        return selected;
    }

    if contains_any(
        query,
        &["list", "show", "find", "folder", "directory", "tree"],
    ) {
        selected.insert("list_dir");
        selected.insert("read_file");
    }

    if contains_any(query, &["read", "open", "inspect", "view", "cat", "file"]) {
        selected.insert("read_file");
        selected.insert("list_dir");
    }

    if contains_any(query, &["write", "create", "save", "new file", "append"]) {
        selected.insert("write_file");
        selected.insert("read_file");
    }

    if contains_any(
        query,
        &["edit", "replace", "patch", "modify", "refactor", "update"],
    ) {
        selected.insert("edit_file");
        selected.insert("read_file");
    }

    if contains_any(
        query,
        &[
            "run", "exec", "shell", "command", "test", "build", "cargo", "npm", "make",
        ],
    ) {
        selected.insert("exec");
        selected.insert("read_file");
    }

    if selected.is_empty() {
        selected.insert("read_file");
        selected.insert("list_dir");
    }

    selected
}

fn contains_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}

fn compact_description(description: &str, max_chars: usize) -> String {
    let normalized = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return normalized;
    }

    let mut seen = HashSet::new();
    let mut unique_sentences = Vec::new();
    for sentence in normalized.split('.') {
        let part = sentence.trim();
        if part.is_empty() {
            continue;
        }
        let key = part.to_ascii_lowercase();
        if seen.insert(key) {
            unique_sentences.push(part.to_string());
        }
    }

    let mut compact = unique_sentences.join(". ");
    if !compact.ends_with('.') {
        compact.push('.');
    }
    truncate_chars(&compact, max_chars.max(32))
}

fn compact_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(obj) => {
            let mut out = Map::new();
            for key in [
                "type",
                "properties",
                "required",
                "additionalProperties",
                "items",
                "enum",
                "oneOf",
                "anyOf",
                "allOf",
            ] {
                if let Some(value) = obj.get(key) {
                    let mapped = if key == "properties" {
                        if let Some(props) = value.as_object() {
                            let mut compact_props = Map::new();
                            for (name, prop_schema) in props {
                                compact_props.insert(name.clone(), compact_schema(prop_schema));
                            }
                            Value::Object(compact_props)
                        } else {
                            compact_schema(value)
                        }
                    } else if matches!(key, "oneOf" | "anyOf" | "allOf") {
                        if let Some(list) = value.as_array() {
                            Value::Array(list.iter().map(compact_schema).collect())
                        } else {
                            compact_schema(value)
                        }
                    } else {
                        compact_schema(value)
                    };
                    out.insert(key.to_string(), mapped);
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(compact_schema).collect()),
        _ => schema.clone(),
    }
}

fn is_tool_enabled_for_mode(name: &str, mode: ExecutionMode) -> bool {
    match mode {
        ExecutionMode::Locked => false,
        ExecutionMode::ReadOnly => !matches!(name, "write_file" | "edit_file" | "exec"),
        ExecutionMode::DryRun | ExecutionMode::Full => true,
    }
}

fn truncate_to_token_budget(text: &str, token_budget: usize) -> String {
    let max_chars = token_budget.saturating_mul(CHARS_PER_TOKEN_APPROX).max(8);
    truncate_chars(text, max_chars)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let kept = max_chars.saturating_sub(3);
    let mut out = chars.into_iter().take(kept).collect::<String>();
    out.push_str("...");
    out
}

fn extract_model_size_billion(model_id: &str) -> Option<f32> {
    let bytes = model_id.as_bytes();
    for idx in 0..bytes.len() {
        let b = bytes[idx];
        if b != b'b' && b != b'B' {
            continue;
        }

        let mut start = idx;
        while start > 0 {
            let c = bytes[start - 1];
            if c.is_ascii_digit() || c == b'.' {
                start -= 1;
            } else {
                break;
            }
        }

        if start == idx {
            continue;
        }

        let candidate = &model_id[start..idx];
        if let Ok(value) = candidate.parse::<f32>() {
            if (1.0..=200.0).contains(&value) {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mk_msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage::text(role, content.to_string())
    }

    fn mk_tool(name: &str, desc: &str) -> FunctionDef {
        FunctionDef {
            name: name.to_string(),
            description: desc.to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string","description":"long description"},
                    "content":{"type":"string","description":"long description"}
                },
                "required":["path"]
            }),
        }
    }

    #[test]
    fn applies_profile_by_model_size() {
        let small = select_model_prompt_profile("ollama", "qwen2.5-coder:7b");
        assert_eq!(small.kind, ModelPromptProfileKind::SmallLocal);

        let mid = select_model_prompt_profile("mlx", "deepseek-coder-13b");
        assert_eq!(mid.kind, ModelPromptProfileKind::MidLocal);

        let large = select_model_prompt_profile("llamacpp", "qwen2.5-32b-instruct");
        assert_eq!(large.kind, ModelPromptProfileKind::LargeLocal);

        let remote = select_model_prompt_profile("openrouter", "anthropic/claude-3.7-sonnet");
        assert_eq!(remote.kind, ModelPromptProfileKind::RemoteFull);
    }

    #[test]
    fn prompt_never_exceeds_budget_estimate() {
        let profile = ModelPromptProfile::for_kind(ModelPromptProfileKind::SmallLocal)
            .apply_overrides(Some(320), Some(20), Some(5));
        let builder = PromptBuilder;
        let conversation = vec![
            mk_msg(MessageRole::User, &"u ".repeat(900)),
            mk_msg(MessageRole::Assistant, &"a ".repeat(900)),
            mk_msg(MessageRole::User, &"u2 ".repeat(900)),
        ];

        let output = builder.build(PromptBuildInput {
            system_prompt_override: Some("system override".to_string()),
            execution_mode: ExecutionMode::Full,
            profile: profile.clone(),
            conversation,
            skill_summaries: vec![
                "skill-a: very long summary".to_string(),
                "skill-b: very long summary".to_string(),
            ],
            tools: vec![
                mk_tool("read_file", "Read file. Read file. Read file."),
                mk_tool("write_file", "Write file. Write file. Write file."),
                mk_tool("edit_file", "Edit file. Edit file. Edit file."),
            ],
            aggressive_tool_filtering: true,
        });

        assert!(
            output.estimated_prompt_tokens <= profile.max_tokens_prompt,
            "estimate={}, max={}",
            output.estimated_prompt_tokens,
            profile.max_tokens_prompt
        );
    }

    #[test]
    fn sliding_window_preserves_recent_and_system() {
        let profile = ModelPromptProfile::for_kind(ModelPromptProfileKind::SmallLocal)
            .apply_overrides(Some(1200), Some(3), Some(5));
        let builder = PromptBuilder;
        let conversation = vec![
            mk_msg(MessageRole::User, "u1"),
            mk_msg(MessageRole::Assistant, "a1"),
            mk_msg(MessageRole::User, "u2"),
            mk_msg(MessageRole::Assistant, "a2"),
            mk_msg(MessageRole::User, "u3"),
            mk_msg(MessageRole::Assistant, "a3"),
        ];

        let output = builder.build(PromptBuildInput {
            system_prompt_override: None,
            execution_mode: ExecutionMode::Full,
            profile,
            conversation,
            skill_summaries: Vec::new(),
            tools: vec![mk_tool("read_file", "Read files.")],
            aggressive_tool_filtering: false,
        });

        assert!(matches!(
            output.messages.first().map(|m| m.role),
            Some(MessageRole::System)
        ));
        assert_eq!(output.messages.len(), 4);
        assert_eq!(output.messages[1].content, "a2");
        assert_eq!(output.messages[2].content, "u3");
        assert_eq!(output.messages[3].content, "a3");
    }

    #[test]
    fn filters_tools_aggressively_and_by_mode() {
        let profile = ModelPromptProfile::for_kind(ModelPromptProfileKind::SmallLocal)
            .apply_overrides(Some(1500), Some(10), Some(2));
        let builder = PromptBuilder;
        let output = builder.build(PromptBuildInput {
            system_prompt_override: None,
            execution_mode: ExecutionMode::ReadOnly,
            profile,
            conversation: vec![mk_msg(
                MessageRole::User,
                "read the file and list the directory",
            )],
            skill_summaries: Vec::new(),
            tools: vec![
                mk_tool("read_file", "Read file."),
                mk_tool("list_dir", "List folder."),
                mk_tool("write_file", "Write file."),
                mk_tool("exec", "Execute shell command."),
            ],
            aggressive_tool_filtering: true,
        });

        let names = output
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["read_file", "list_dir"]);
        for tool in &output.tools {
            assert!(tool.description.len() <= 120);
            assert!(tool.parameters["properties"]["path"]["description"].is_null());
        }
    }
}
