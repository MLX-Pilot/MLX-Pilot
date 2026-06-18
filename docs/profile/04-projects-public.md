# 04 — Public & Open-Source Projects

## MLX Pilot

**GitHub:** `github.com/MLX-Pilot/MLX-Pilot`
**Role:** Technical Lead & Primary Integrator
**Timeline:** 2026-03 through 2026-06 (5 sprints, ongoing)

### What it is

A Rust/Tauri desktop application for running, managing, and orchestrating LLMs
entirely on local hardware — no cloud dependency required. Think "LM Studio
meets an agent runtime, with enterprise security controls."

### Tech stack

- **Backend:** Rust (tokio, axum 0.8, rusqlite, reqwest, cron, tower-http)
- **Desktop shell:** Tauri (Rust + native webview)
- **Frontend:** Vanilla JavaScript ES modules (28 feature modules), CSS custom
  properties, no bundler/build step
- **Providers:** MLX (Apple Silicon), llama.cpp embedded, Ollama, plus cloud
  providers (DeepSeek, OpenAI, Anthropic, Groq, OpenRouter) via HTTP
- **Agent runtime:** Full tool-calling loop with policy engine, approval
  service, skills loader (SKILL.md compatible), secrets vault, context budget
  management, memory store with embeddings + FTS5 hybrid search
- **Database:** SQLite with versioned migrations, WAL mode, FTS5 indexes

### Engineering complexity

- **15+ Rust crates** in a Cargo workspace with internal path dependencies
- **50+ HTTP endpoints** on the daemon: chat, streaming, agent run, models,
  catalog, channels, webhooks, presets, memory, search, research, hardware
  fit, orchestration monitoring, notes/tasks, scheduler, jobs, compare
- **Multi-provider routing:** Auto-detects available local providers (MLX,
  llama.cpp, Ollama), normalizes model IDs, falls back between providers
- **Security:** Three-tier security modes (standard/enterprise/paranoid),
  sandboxed tool execution, SSRF guards on webhooks, encrypted secrets vault,
  skill integrity verification via SHA-256 pinning
- **20 channel adapters:** WhatsApp (native QR bridge), Telegram/Discord/Slack
  (bot token), webhook channels (Google Chat, Feishu, MSTeams, Mattermost,
  Synology Chat), HTTP bridge channels (Signal, iMessage, BlueBubbles, Nostr,
  LINE, Zalo), Matrix, IRC — all with multi-account isolation
- **Formal release gate:** Zero blocking security findings, load-tested at
  c=50 with p99 < 105ms, migration/rollback verified, crash recovery tested
- **95 tests** covering scheduling, CRUD, webhook validation, toast broadcast,
  concurrent tick safety, restart recovery, cancellation

### Impact

- Transforms scattered CLI-based LLM usage into a unified desktop experience
- Brings enterprise security patterns (policy engine, audit trails,
  allow/deny rules) to local AI — a space that typically ignores them
- Open-source under active development

### What I personally built

- **Daemon architecture:** HTTP server, routing, AppState, middleware stack
- **Agent runtime integration:** state store, session store, memory store,
  EventBus, policy engine binding, provider routing, API endpoints
- **Scheduler:** Background job registry with progress streaming (SSE),
  cancellation tokens, concurrency cap, cron/once/interval task engine with
  atomic claim mechanism to prevent duplicate execution
- **Notes & Tasks module:** Full CRUD with checklist/color/pin/due date,
  task lifecycle (pause/resume/run-now/history), toast SSE broadcast,
  webhook dispatch with SSRF guards
- **Channels infrastructure:** Protocol version negotiation, transport family
  classification, multi-account session isolation
- **Frontend modularization:** Restructured 28 ES modules from monolith IIFEs,
  maintaining visual and behavioral parity while enabling independent
  development
- **Integration across all 5 sprints:** 30 commits (highest in the team),
  owned cross-cutting consolidation that made independent features work as
  one coherent product

### Portfolio-safe copy

> MLX Pilot is a local-first AI desktop platform built in Rust and Tauri.
> I led a 6-person team through 5 sprints, architecting the daemon backend,
> agent runtime, multi-provider LLM routing, and 20+ channel integrations.
> The project ships with enterprise security controls (policy engine,
> secrets vault, SSRF guards) and passed a formal release gate with zero
> blocking findings. 95 tests, 15+ crates, 50+ API endpoints.

---

## RTK — Rust Token Killer

**Role:** Author
**Context:** Personal productivity tool (referenced in global CLAUDE.md)

A token-optimized CLI proxy for development operations, achieving 60-90%
token savings on common dev commands. Acts as a transparent middleware
between the developer and shell commands, rewriting output to be
token-efficient for LLM consumption.

### Portfolio-safe copy

> Built a CLI proxy that optimizes shell command output for LLM token
> efficiency, reducing context window consumption by 60-90% for common
> development operations. Demonstrated practical systems thinking about
> developer tooling and LLM integration costs.

---

## Additional projects

**[needs verification]** — The profile owner should add:

- University capstone / TCC project details (the MLX Pilot appears to serve
  this role based on repository structure and video scripts referencing
  "professor ou banca")
- Any other public GitHub repositories
- Open-source contributions to other projects
- Hackathon or competition entries

### Suggested format for additional entries

```markdown
## Project Name

**Link:** github.com/...
**Role:** [Solo developer / Team of N / Contributor]
**Timeline:** [Month Year]

### What it is
[One paragraph]

### Tech stack
[Bullet list]

### What I personally built
[Specific, honest — don't claim team work as solo]
```
