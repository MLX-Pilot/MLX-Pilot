# Hermes to MLX-Pilot Integration Plan

Date: 2026-04-18
Hermes source analyzed: `external/hermes-agent` @ `cb4addacab4679914878ceaab3be7bd1011ffb7a`

## Strategy

Adopt Hermes conceptually inside MLX-Pilot:

- `Option A` as the primary architecture: native Rust implementation of session memory, runtime loop, skill registry, provider abstraction, and delegation.
- `Option C` as a limited metadata bridge: skill package format tagging and future import/interoperability hooks.

`Option B` is intentionally rejected as the base architecture because it would duplicate state, session, and policy between Rust and Python.

## Target architecture

```text
agent_api.rs
  -> runtime selector (classic | hermes_inspired)
    -> AgentRuntime
      -> SessionStore (SQLite primary)
      -> MemoryManager
      -> SessionRecall
      -> ToolRegistry / runtime tools
      -> ModelProvider (Ollama/OpenAI/etc.)
      -> SkillRuntime / SkillRegistry metadata
```

## Storage design

Primary store: `settings_dir()/agent/state.sqlite`

Tables:

- `sessions`
  - `id`
  - `name`
  - `provider_id`
  - `model_id`
  - `workspace_root`
  - `origin_kind`
  - `parent_session_id`
  - `status`
  - `created_at`
  - `updated_at`
- `session_events`
  - `id`
  - `session_id`
  - `kind`
  - `role`
  - `tool_name`
  - `tool_call_id`
  - `content`
  - `content_json`
  - `metadata_json`
  - `created_at`
- `memory_records`
  - `id`
  - `session_id`
  - `scope`
  - `namespace`
  - `kind`
  - `title`
  - `content`
  - `tags_json`
  - `metadata_json`
  - `importance`
  - `created_at`
  - `last_accessed_at`

Compatibility:

- Preserve current JSONL/JSON export paths for sessions and compact memory when useful.
- SQLite becomes the source of truth for the new runtime.

## Runtime behavior

The Hermes-inspired runtime should:

1. Load prior structured session events for the current session.
2. Query recent long-term memory and relevant prior sessions.
3. Build the prompt from:
   - system prompt
   - memory context block
   - recalled session summaries
   - selected tools
   - selected skills
   - current conversation
4. Execute iterative turns until:
   - no tool calls remain
   - a stop condition triggers
   - iteration budget is exhausted
5. Persist:
   - user messages
   - assistant messages
   - tool calls
   - tool results
   - summary/system snapshot events
6. Write back durable memory records when requested or when summarization policy decides.

## MVP code changes

### agent-core

- Add `agent_runtime.rs`.
- Add `state_store.rs`.
- Add `memory_manager.rs`.
- Add `session_recall.rs`.
- Expand `session.rs` types for richer metadata and event kinds.
- Expand `memory.rs` types for scoped durable memory.

### daemon

- Add config flags:
  - `runtime_variant`
  - `persist_tool_events`
  - `session_search_enabled`
  - `memory_profile`
- Extend `/agent/run` request shape for:
  - `runtime_variant`
  - `persist_tool_events`
  - `session_context`
  - `delegate_depth`
- Register new runtime tools:
  - `session_search`
  - `memory_write`
  - `delegate_session`
- Route Ollama config cleanly into the runtime/provider call path.

### agent-skills

- Expand `SkillPackage` with format and metadata fields:
  - `format`
  - `manifest_version`
  - `references`
  - `templates`
  - `scripts`
  - `assets`
  - `policy`

### UI

- Surface `runtime_variant`.
- Show local storage/runtime status.
- Keep existing control plane intact.

## Validation target

MVP is complete when:

- a run can opt into `hermes_inspired`
- Ollama works as local provider with tool-calling loop
- session events persist in SQLite
- `memory_write` persists and `session_search` can find prior sessions
- at least one delegated child session can be created and summarized
- skills expose the richer metadata base without breaking current loading
