# Hermes Architecture Overview

Date: 2026-04-18
Hermes source analyzed: `external/hermes-agent` @ `cb4addacab4679914878ceaab3be7bd1011ffb7a`

## Entry points

- `run_agent.py` contains the main `AIAgent` orchestration loop.
- `model_tools.py` is the tool bridge between the model/runtime and the concrete tool implementations.
- `toolsets.py` defines named tool bundles and composition rules.
- `gateway/session.py` owns multi-channel session context and persisted gateway conversation state.
- `hermes_cli/config.py` centralizes config, home directory layout, managed install rules, and setup defaults.
- `hermes_cli/providers.py` is the single provider registry that merges catalog data, Hermes overlays, and user config.

## Core agent loop

The Hermes loop is explicit and stateful, not just a single chat-completion wrapper:

1. Build prompt context from system prompt, memory snapshot, session context, tools, and toolset constraints.
2. Call the selected provider/model.
3. Inspect the response for tool calls.
4. Dispatch tool calls through `model_tools.handle_function_call`.
5. Append tool results back into the conversation.
6. Repeat until no more tool calls, stop conditions trigger, or the iteration budget is exhausted.

Notable properties:

- Iteration budget is first-class (`IterationBudget` in `run_agent.py`).
- Tool execution policy is contextual: there are heuristics for safe subsets, destructive commands, and path-sensitive tools.
- The loop supports multi-step tasks and can surface progress callbacks.

## Session and conversation model

Hermes separates local CLI session behavior from gateway-originated conversations:

- The core loop tracks the current task/session context.
- `gateway/session.py` adds source-aware session context for platforms like Telegram/Discord/etc.
- Session context is injected into the prompt so the agent knows where the conversation originated and where follow-up actions can be routed.
- Delegated child sessions are isolated from parent history; the parent only sees the summary result.

## Memory model

Hermes memory is more than a flat note store:

- `agent/memory_manager.py` orchestrates memory providers and lifecycle hooks.
- There is a built-in durable memory provider plus optional external provider integration.
- `tools/memory_tool.py` persists two durable stores (`MEMORY.md`, `USER.md`).
- Memory is snapshotted into the system prompt as a frozen block to preserve prefix-cache stability.
- Mid-session writes are durable, but they do not mutate the already-injected prompt snapshot during the same turn.
- Hooks exist for `on_turn_start`, `on_session_end`, `on_pre_compress`, `on_memory_write`, and `on_delegation`.

## Session search

`tools/session_search_tool.py` adds cross-session recall using:

- SQLite storage with FTS5 search.
- Current-session exclusion.
- Child/delegation session exclusion.
- Result truncation around the relevant hits.
- Optional summary of top matches.

This is a key Hermes behavior because it turns persisted sessions into reusable context, not just logs.

## Skills and toolsets

Hermes uses named toolsets instead of one global monolithic tool inventory:

- `toolsets.py` composes bundles with `includes`.
- Different scenarios expose different capabilities.
- Some toolsets are intentionally excluded from delegation or special-case flows.

This is conceptually close to a capability-aware skill/tool profile system rather than a flat allowlist.

## Tool exposure and dispatch

`model_tools.py` is the abstraction boundary:

- Discovers built-in tools, plugins, and MCP-backed tools.
- Produces tool definitions for the model.
- Maps a tool call back to the correct implementation.
- Supports async execution and parallel-friendly handling.

The important reuse here is conceptual: tool discovery, definition exposure, and dispatch belong in a dedicated orchestration layer, not buried in provider-specific code.

## Delegation / subagents

`tools/delegate_tool.py` implements explicit subagent delegation:

- Child agents get fresh isolated context.
- Toolsets are restricted for children.
- Dangerous tools are always stripped from delegated runs.
- The parent sees only the summary output, not the child’s intermediate reasoning/tool history.
- Delegation depth and concurrency are bounded.

## Provider organization

`hermes_cli/providers.py` is a unified registry:

- Base provider metadata comes from an external catalog.
- Hermes overlays add transport type, auth mode, and extra environment/base URL rules.
- User config is layered on top.

The structural idea matters more than the exact Python implementation: one canonical provider abstraction, transport-aware, with config overlays.

## Persistence and configuration

`hermes_cli/config.py` centralizes:

- Home directory and config/env paths.
- Managed-mode semantics.
- Setup/update commands.
- Container-aware execution.

Hermes treats configuration as part of the runtime contract, not as scattered ad-hoc globals.

## Reusable concepts for MLX-Pilot

These should be adapted, not copied:

- Explicit iterative agent runtime with stop reasons and tool/result events.
- Memory manager with prompt-snapshot semantics plus durable writes.
- Searchable session persistence.
- Delegation as child sessions with bounded policy.
- Provider abstraction that separates transport shape from user config.
- Toolset/profile composition as the basis for capability-aware exposure.

## Hermes-specific pieces not worth copying directly

- Python runtime/process model and event loop details.
- Hermes gateway platform integrations as-is.
- External provider catalog dependency pattern.
- Markdown-file memory layout (`MEMORY.md`, `USER.md`) as the primary storage format.

## Rust/Tauri reinterpretation for MLX-Pilot

These concepts should be redesigned idiomatically:

- Storage should move to local SQLite for sessions + memory + recall, with JSONL export compatibility where useful.
- Runtime orchestration should live in `agent-core` / `daemon`, not in provider adapters.
- Skills should extend the existing `agent-skills` crate instead of imitating Python package loading.
- Delegation should be modeled as child sessions and bounded tool-policy inheritance.
- UI/settings should expose runtime/provider toggles without making Hermes a runtime dependency.
