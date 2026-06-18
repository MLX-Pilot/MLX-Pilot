# 01 — Professional Profile

## Professional Summary

Full Stack Developer with a strong backend focus and deep experience in Rust,
TypeScript, and systems-level engineering. I architect and build local-first,
privacy-respecting tools that put AI workloads under user control — from
multi-provider LLM orchestration to desktop-native agent runtimes.

I led the technical direction of **MLX Pilot**, a Rust/Tauri desktop platform
that unifies local LLM inference (MLX, llama.cpp, Ollama) with an extensible
agent runtime, multi-channel integrations, and enterprise-grade security
controls. The project shipped across 5 sprints with a 6-person team, passing a
formal release gate with zero blocking security findings.

Before focusing on AI/LLM tooling, I worked in enterprise environments —
building monitoring dashboards, automation pipelines, and operational tooling
for large-scale systems backed by DB2, Kubernetes/OpenShift, and mainframe-
adjacent batch workloads. That experience taught me to design for reliability
from day one.

## Current Positioning

- **Primary stack:** Rust (async/tokio, axum, SQLite), TypeScript/JavaScript
  (ES modules, Tauri frontend), Python (FastAPI reference patterns)
- **Domain:** AI/LLM orchestration, desktop-native tooling, agent runtimes,
  backend services with strong correctness guarantees
- **Platform:** Windows primary, cross-platform (macOS, Linux) aware
- **Open source:** Active on GitHub under `github.com/MLX-Pilot`

## Preferred Role Positioning

**Full Stack Developer with strong backend focus.** I'm most effective in roles
where I own significant backend architecture (API design, database schemas,
concurrency models, security boundaries) while staying close enough to the
frontend to ensure the product feels coherent end-to-end.

I thrive on:
- Designing clean internal APIs and data models
- Building reliable async/concurrent systems
- Integrating AI/LLM capabilities into products users actually run locally
- Leading technical integration across multiple contributors
- Shipping with tests, not after them

## Strongest Selling Points

1. **Rust in production.** 5+ crates in a real desktop application: daemon with
   HTTP API, agent runtime with policy engine, multi-provider LLM routing,
   SQLite-backed state store, secrets vault, job scheduler with cron support,
   and 20+ channel adapters. Async/tokio throughout.

2. **Technical leadership.** Led a 6-person team through 5 sprints. Owned
   integration and consolidation — the work that makes independent features
   work as one coherent product. Maintained curated git history and changelogs.

3. **AI/LLM depth.** Built a complete agent runtime (tool calling, skills,
   policy/approval, context budget management, memory/embeddings). Integrated
   3 local inference providers (MLX, llama.cpp, Ollama) plus cloud providers
   (DeepSeek, OpenAI, Anthropic, Groq, OpenRouter) behind a unified interface.

4. **Desktop-native shipping.** Tauri app from scratch: Rust backend + vanilla
   ES module frontend, 28 feature modules, SSE streaming, no bundler/build step
   complexity. The app passes a formal release gate (load tests, security audit,
   migration/rollback, crash recovery).

5. **Security mindset.** Enterprise/paranoid security modes, secrets vault
   with encryption at rest, SSRF guards on webhooks, sandboxed tool execution,
   allow/deny policies, skill integrity verification, and release-gate security
   review with zero blocking findings.

## Enterprise Experience (Generalized)

I have professional experience in large-scale enterprise environments —
specifically banking-sector infrastructure — where I worked on:

- **Operational monitoring and dashboards** for critical batch and online
  systems
- **Automation pipelines** reducing manual toil in deployment and incident
  response
- **API integrations** connecting heterogeneous systems (mainframe-adjacent,
  distributed, cloud)

Technologies included DB2, Kubernetes/OpenShift, CI/CD pipelines,
observability stacks, and scripting/automation tooling. These roles taught
me to think about reliability, audit trails, and graceful degradation as
first-class concerns — patterns that carry directly into how I design AI
systems today.

> **See also:** `05-enterprise-experience-sanitized.md` for safe case studies.
