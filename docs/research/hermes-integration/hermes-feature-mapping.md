# Hermes Feature Mapping

Date: 2026-04-18
Hermes source analyzed: `external/hermes-agent` @ `cb4addacab4679914878ceaab3be7bd1011ffb7a`

| Hermes capability | Hermes source | Current MLX-Pilot module | Current state | Integration target |
| --- | --- | --- | --- | --- |
| Explicit multi-turn agent loop | `run_agent.py` | `crates/agent-core/src/agent_loop.rs` | Present, but single runtime path and no explicit runtime variants | Add `agent_runtime.rs` and keep `agent_loop.rs` as classic facade |
| Iteration budget / stop reasons | `run_agent.py` | `agent_loop.rs` | Partial max-iteration guard only | Add explicit `StopReason` and runtime turn events |
| Tool registry / tool dispatch bridge | `model_tools.py` | `registry.rs`, `daemon/agent_runtime_tools.rs` | Present, but not memory/delegation aware | Extend registry + runtime tools around session/memory/delegation |
| Toolset composition | `toolsets.py` | `tool_catalog.rs`, tool policy in daemon config | Present as tool profiles/policy, but not skill/runtime-oriented | Reuse policy layer and add Hermes-inspired runtime capability profiles |
| Memory manager lifecycle hooks | `agent/memory_manager.py` | `memory.rs` | Missing | Add `memory_manager.rs` and runtime integration |
| Durable memory writes | `tools/memory_tool.py` | `memory.rs` | Partial compact artifact store only | Add `memory_write` and long-term memory records with metadata |
| Search past sessions | `tools/session_search_tool.py` | `session.rs`, `memory.rs` | Missing | Add `session_recall.rs` + `session_search` tool backed by SQLite |
| Session/source context | `gateway/session.py` | `session.rs`, `/agent/sessions*` endpoints | Basic local sessions only | Expand session metadata for origin/provider/workspace/parent |
| Delegated subagents | `tools/delegate_tool.py` | none | Missing | Add `delegate_session` tool with child sessions |
| Provider registry abstraction | `hermes_cli/providers.py` | `resolve_provider()` in `daemon/agent_api.rs`, provider crates | Partial, more provider-specific wiring than canonical registry | Keep current providers, add runtime-aligned provider metadata and request config |
| Config/home/runtime defaults | `hermes_cli/config.py` | `daemon/config.rs` | Present, but no runtime variant or storage mode | Add opt-in `hermes_inspired` runtime config fields |
| Ollama OpenAI-compatible onboarding | Ollama docs + Hermes docs | `providers/ollama`, `agent_api.rs` | Partial; already supports base URL and provider runtime | Tighten config/UI alignment and ensure tool loop compatibility |
| Skills/capability metadata | Hermes skills/toolsets | `crates/agent-skills` | Present but still minimal | Expand manifest metadata and capability discovery |

## Priority classification

### Must have in this cycle

- Runtime split between classic and Hermes-inspired orchestration.
- SQLite-backed persistent sessions + memory foundation.
- `session_search`, `memory_write`, and `delegate_session`.
- Expanded session/memory metadata.
- Ollama as first-class provider in the new runtime path.

### Nice to have in this cycle

- Skill manifest format tagging (`native`, `claude`, `hermes`, `codex`, `hermes_compatible`).
- UI exposure for runtime variant and storage status.
- JSONL export compatibility.

### Later

- Full gateway/messaging parity with Hermes.
- Automatic skill authoring or self-improving workflows.
- Parallel delegated subagents.
- External memory providers beyond local storage.

