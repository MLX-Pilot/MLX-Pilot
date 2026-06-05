# Hermes Gap Analysis

Date: 2026-04-18
Hermes source analyzed: `external/hermes-agent` @ `cb4addacab4679914878ceaab3be7bd1011ffb7a`

## High-severity gaps

### 1. No Hermes-style runtime boundary

- Current MLX-Pilot has a single `AgentLoop`.
- There is no `runtime_variant` or alternate orchestration mode.
- Consequence: it is impossible to introduce Hermes-inspired session/memory/delegation behavior cleanly without overloading the existing loop.
- Decision: implement now.

### 2. Session persistence is log-oriented, not recall-oriented

- `SessionStore` is JSONL with a lightweight index.
- There is no structured event model, parent/child relation, or search index.
- Consequence: sessions can be listed/read but not reused as active memory.
- Decision: implement now with SQLite as primary storage.

### 3. Memory is too narrow

- `MemoryStore` only stores compact artifacts in a single JSON index.
- There is no distinction between session memory, long-term memory, or memory writes triggered by the agent.
- Consequence: memory is not a first-class runtime primitive.
- Decision: implement now.

### 4. No delegation model

- There is no child session / delegated task abstraction.
- Consequence: MLX-Pilot cannot reproduce one of the most important Hermes behaviors.
- Decision: implement now as bounded synchronous MVP.

## Medium-severity gaps

### 5. Skills are loadable but not yet a richer registry

- `agent-skills` already loads workspace skills and requirements.
- It does not yet expose richer package metadata for routines/assets/templates/references/format.
- Consequence: compatibility with Hermes-inspired skill metadata is limited.
- Decision: implement now in a minimal non-breaking way.

### 6. Tool runtime is not memory-aware

- Runtime tools expose sessions and memory, but they are independent utilities.
- Consequence: the agent loop does not naturally hydrate from or write back to persistent memory.
- Decision: implement now.

### 7. Provider abstraction is usable but not runtime-oriented

- Provider resolution exists, and prior work already improved Ollama.
- The runtime still assumes a single `AgentLoop` path instead of a runtime strategy.
- Decision: implement now.

## Lower-severity / later gaps

### 8. Full gateway/messaging parity

- Hermes gateway is richer than MLX-Pilot’s current runtime integration.
- Decision: later. Not required for this MVP.

### 9. External memory providers

- Hermes can orchestrate local + external memory providers.
- Decision: later. Local-first base comes first.

### 10. Parallel delegation

- Hermes supports batch/parallel child tasks.
- Decision: later. MVP should bound depth and avoid adding concurrency risk too early.

## Chosen integration strategy

Primary strategy: conceptual compatibility (`Option A`) with a small internal compatibility layer for metadata (`Option C`).

Reasons:

- Best long-term fit for Rust/Tauri/daemon architecture.
- Preserves local-first guarantees.
- Avoids runtime coupling to Python or a sidecar Hermes process.
- Lets MLX-Pilot absorb the architecture rather than mimic the implementation language.
