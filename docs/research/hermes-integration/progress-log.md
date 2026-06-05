# Hermes Integration Progress Log

## 2026-04-18

### Hermes files and sources studied

- `external/hermes-agent/run_agent.py`
- `external/hermes-agent/model_tools.py`
- `external/hermes-agent/toolsets.py`
- `external/hermes-agent/agent/memory_manager.py`
- `external/hermes-agent/tools/memory_tool.py`
- `external/hermes-agent/tools/session_search_tool.py`
- `external/hermes-agent/tools/delegate_tool.py`
- `external/hermes-agent/gateway/session.py`
- `external/hermes-agent/hermes_cli/providers.py`
- `external/hermes-agent/hermes_cli/config.py`
- Ollama docs:
  - `https://docs.ollama.com/integrations/hermes`
  - `https://docs.ollama.com/capabilities/tool-calling`

Hermes checkout:

- remote: `https://github.com/NousResearch/hermes-agent`
- commit: `cb4addacab4679914878ceaab3be7bd1011ffb7a`

### MLX-Pilot components mapped

- `crates/agent-core/src/agent_loop.rs`
- `crates/agent-core/src/session.rs`
- `crates/agent-core/src/memory.rs`
- `crates/agent-core/src/runtime.rs`
- `crates/agent-core/src/registry.rs`
- `crates/agent-core/src/tool_catalog.rs`
- `crates/agent-skills/src/types.rs`
- `crates/agent-skills/src/loader.rs`
- `crates/daemon/src/agent_api.rs`
- `crates/daemon/src/agent_runtime_tools.rs`
- `crates/daemon/src/config.rs`
- `crates/providers/ollama/src/lib.rs`
- `apps/desktop-ui/ui/agent-control-plane.js`

### Strategy chosen

- Primary: conceptual compatibility (`Option A`)
- Secondary: internal metadata compatibility layer (`Option C`)

Reason:

- best fit for a local-first Rust/Tauri daemon
- no Python runtime dependency
- preserves maintainability and policy/security integration

### Modules expected to be created or altered in this cycle

- Create:
  - `crates/agent-core/src/agent_runtime.rs`
  - `crates/agent-core/src/state_store.rs`
  - `crates/agent-core/src/memory_manager.rs`
  - `crates/agent-core/src/session_recall.rs`
- Alter:
  - `crates/agent-core/src/lib.rs`
  - `crates/agent-core/src/session.rs`
  - `crates/agent-core/src/memory.rs`
  - `crates/agent-core/src/runtime.rs`
  - `crates/agent-skills/src/types.rs`
  - `crates/agent-skills/src/loader.rs`
  - `crates/daemon/src/agent_api.rs`
  - `crates/daemon/src/agent_runtime_tools.rs`
  - `crates/daemon/src/config.rs`
  - `crates/providers/ollama/src/lib.rs`
  - `apps/desktop-ui/ui/agent-control-plane.js`

### Remaining limitations

- No Hermes gateway parity in this cycle.
- No external memory providers in this cycle.
- No parallel delegated subagents in this cycle.

### Recommended next steps

1. Implement SQLite-backed state store.
2. Introduce runtime variant selection and Hermes-inspired runtime orchestration.
3. Add `memory_write`, `session_search`, and `delegate_session`.
4. Expand skill metadata and provider/runtime config.
5. Validate with Ollama multi-turn tool loop and cross-session recall.

## 2026-04-18 — Implementation checkpoint

### Modules created or altered

- Created:
  - `crates/agent-core/src/agent_runtime.rs`
  - `crates/agent-core/src/state_store.rs`
  - `crates/agent-core/src/memory_manager.rs`
  - `crates/agent-core/src/session_recall.rs`
  - `docs/hermes-inspired-runtime.md`
- Altered:
  - `crates/agent-core/src/agent_loop.rs`
  - `crates/agent-core/src/events.rs`
  - `crates/agent-core/src/lib.rs`
  - `crates/agent-core/src/memory.rs`
  - `crates/agent-core/src/policy.rs`
  - `crates/agent-core/src/session.rs`
  - `crates/agent-core/src/tool_catalog.rs`
  - `crates/agent-skills/src/frontmatter.rs`
  - `crates/agent-skills/src/lib.rs`
  - `crates/agent-skills/src/loader.rs`
  - `crates/agent-skills/src/types.rs`
  - `crates/daemon/src/agent_api.rs`
  - `crates/daemon/src/agent_runtime_tools.rs`
  - `crates/daemon/src/config.rs`
  - `crates/daemon/src/lib.rs`
  - `docs/agent_architecture.md`

### What now works

- `runtime_variant=hermes_inspired` is accepted by config and request.
- Session and memory persistence use local SQLite as primary storage.
- Session recall is available through `session_search`.
- Durable writes are available through `memory_write`.
- Delegated child execution is available through `delegate_session` with bounded depth.
- Skill metadata now carries format/support directories for future compatibility work.

### Still missing for Hermes-level parity

- Full gateway/messaging parity from Hermes.
- Rich session search with FTS5 summarization.
- Parallel delegated subagents.
- External memory providers and richer lifecycle hooks.
