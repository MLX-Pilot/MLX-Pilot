# 02 — Skills Matrix

## Legend

| Tier | Meaning |
|------|---------|
| **Strong** | Daily-driver; shipped production code; comfortable teaching/designing |
| **Professional** | Used in paid work; productive without hand-holding |
| **Academic/Project** | Used in university or personal projects; solid fundamentals |
| **Familiar** | Can read, modify, and work with existing code; would need ramp-up for greenfield |

---

## Languages

| Skill | Tier | Evidence |
|-------|------|----------|
| Rust | **Strong** | 5+ crates, async/tokio, axum, SQLite, Tauri, 95 test suite |
| TypeScript / JavaScript | **Strong** | 28 ES module features, SSE streaming, vanilla DOM, no bundler |
| Python | **Professional** | FastAPI reference patterns, scripts, AIRLLM bridge |
| SQL | **Professional** | SQLite schema design, migrations, FTS, WAL mode, complex queries |
| HTML / CSS | **Professional** | Full desktop UI with component library, animations, responsive layout |
| Bash / Shell | **Professional** | Build scripts, operational tooling, git workflows |
| Java | **Academic/Project** | [needs verification] |
| C / C++ | **Familiar** | [needs verification] |

---

## Frameworks & Runtimes

| Skill | Tier | Evidence |
|-------|------|----------|
| Axum 0.8 (Rust HTTP) | **Strong** | Full daemon with 50+ routes, SSE streaming, middleware |
| Tokio (Rust async) | **Strong** | Multi-threaded runtime, mpsc/broadcast channels, CancellationToken |
| Tauri (desktop) | **Strong** | Native shell with capabilities, CSP, icons, cross-platform bundling |
| FastAPI (Python) | **Professional** | Reference implementation study (Odysseus); patterns adapted to Rust |
| Reqwest (Rust HTTP client) | **Strong** | Webhook dispatch, HuggingFace API, Brave Search, cloud LLM providers |
| Tower-HTTP (middleware) | **Professional** | CORS, tracing layers |
| React / Next.js | **Familiar** | [needs verification] |

---

## Databases

| Skill | Tier | Evidence |
|-------|------|----------|
| SQLite (rusqlite) | **Strong** | Schema design, versioned migrations, FTS5, WAL, foreign keys, indexes |
| DB2 | **Professional** | Enterprise monitoring and batch integration |
| PostgreSQL | **Familiar** | [needs verification] |
| Redis | **Familiar** | [needs verification] |

---

## DevOps & CI/CD

| Skill | Tier | Evidence |
|-------|------|----------|
| Git (advanced) | **Strong** | Curated history, worktrees, cherry-pick, rebase, 100+ commits |
| GitHub | **Strong** | PRs, branch management, release workflows |
| CI/CD pipelines | **Professional** | Enterprise environment; release gate automation |
| Docker / Containers | **Professional** | [needs verification] |
| Kubernetes / OpenShift | **Professional** | Enterprise monitoring and deployment |
| Shell scripting | **Professional** | Operational scripts, build automation |

---

## Observability

| Skill | Tier | Evidence |
|-------|------|----------|
| Structured logging (tracing) | **Strong** | EnvFilter, per-crate log levels, audit trails |
| Metrics / Telemetry | **Professional** | Context budget tracking, latency histograms, token usage |
| Dashboards | **Professional** | Enterprise operational dashboards |
| SSE / Event streaming | **Strong** | Job progress, toast notifications, orchestration monitor |
| Release gates | **Strong** | Load testing, security audit, migration/rollback, crash recovery |

---

## AI / LLM

| Skill | Tier | Evidence |
|-------|------|----------|
| LLM providers (MLX, llama.cpp, Ollama) | **Strong** | Multi-provider routing, auto-detection, bootstrap management |
| Cloud LLM APIs (DeepSeek, OpenAI, Anthropic, Groq, OpenRouter) | **Strong** | HTTP provider with unified interface, API key management via vault |
| Agent runtimes | **Strong** | Full agent loop with tool calling, policy engine, approval flow |
| Skills / Tool definitions | **Strong** | SKILL.md loader, tool catalog, schema validation, sandboxed exec |
| Prompt engineering (adaptive) | **Professional** | Per-model prompt profiles, context budget compression |
| Embeddings / Semantic search | **Professional** | FTS5 + embedding hybrid, memory store with similarity |
| RAG patterns | **Professional** | Deep Research with iterative search + synthesis |
| Web scraping (scraper crate) | **Professional** | Search result extraction, content fetching |
| Function calling / Tool use | **Strong** | Full tool-call JSON parsing, fallback reprompt, schema validation |

---

## Cloud & Platform

| Skill | Tier | Evidence |
|-------|------|----------|
| REST API design | **Strong** | 50+ endpoint daemon, consistent error format, versioned protocols |
| WebSocket / SSE | **Strong** | SSE for jobs, toasts, orchestration; channel-bridge protocol v1 |
| Cross-platform desktop | **Strong** | Tauri app targeting Windows, macOS, Linux |
| HuggingFace API | **Professional** | Model search, download, catalog integration |
| Webhooks | **Strong** | 20 channel adapters, SSRF guard, retry, circuit breaker |

---

## Testing

| Skill | Tier | Evidence |
|-------|------|----------|
| Unit tests (Rust) | **Strong** | 95 tests, FakeClock for deterministic scheduling, tempfile isolation |
| Integration tests | **Strong** | SSE streaming tests, webhook mock servers, DB roundtrip tests |
| Load testing | **Professional** | Concurrency smoke tests (c=10, 25, 50), latency percentiles |
| E2E tests (JS) | **Professional** | Channel smoke, skills smoke, release gate scripts |
| Test-driven workflow | **Strong** | Tests committed alongside features; 95/95 passing |

---

## Architecture

| Skill | Tier | Evidence |
|-------|------|----------|
| API design | **Strong** | RESTful conventions, versioned protocols, backwards compatibility |
| Database schema design | **Strong** | Normalized SQLite, versioned migrations, FTS indexes |
| Concurrency models | **Strong** | Semaphore-capped job execution, atomic task claiming, broadcast channels |
| Plugin systems | **Professional** | Plugin manager with enable/disable/config, channel adapter registry |
| Security architecture | **Strong** | Policy engine, secrets vault, SSRF guard, sandboxing, paranoid mode |
| Monorepo management | **Strong** | Rust workspace with 15+ crates, internal path dependencies |

---

## Notes

- Items marked **[needs verification]** should be confirmed and re-tiered by the
  profile owner.
- This matrix was derived entirely from observable project work and changelogs.
  No skills were invented.
- The "Professional" tier for DB2 and Kubernetes/OpenShift is inferred from
  enterprise context clues in the codebase and spec references. Adjust tier if
  the actual depth differs.
