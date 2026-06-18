# 07 — Site Content

> Final copy for the portfolio website. Tone: confident, technical, direct.
> No hype. No cringe. English primary; Portuguese-friendly phrasing where
> natural.

---

## SEO Meta

```html
<title>Kaike Vitorino — Full Stack Developer | Rust, AI/LLM, Backend</title>
<meta name="description" content="Full Stack Developer with strong backend
focus. I build local-first AI tooling in Rust and TypeScript — LLM
orchestration, agent runtimes, desktop-native apps with enterprise-grade
security. Based in Brazil." />
```

---

## Hero

**Hi, I'm Kaike.**

I build AI tools that run on your machine, not someone else's cloud —
local-first, privacy-respecting, and fast. I work primarily in **Rust**
and **TypeScript**, with a strong backend focus and a weakness for clean
internal APIs.

Currently leading technical development of **MLX Pilot**, an open-source
desktop platform for local LLM orchestration with an extensible agent
runtime.

---

## About

I'm a Full Stack Developer who gravitates toward the backend — the layer
where API design, data models, concurrency, and security boundaries all
meet. I care about building systems that are correct, testable, and
explainable.

My main project right now is MLX Pilot: a Rust/Tauri desktop app that
unifies local LLM inference, agent-based automation, and multi-channel
integrations. I've led its technical direction across 5 sprints with a
6-person team — from the first HTTP endpoint to a formal release gate with
zero blocking security findings.

Before focusing on AI/LLM tooling, I worked in enterprise environments
where I learned to treat reliability, audit trails, and security as
first-class concerns — not afterthoughts. That experience shapes how I
architect everything I build now.

When I'm not writing Rust, I'm probably optimizing something that didn't
need optimizing, or convincing myself that vanilla JavaScript is still a
valid life choice.

**Location:** Brazil
**Languages:** Portuguese (native), English

---

## Experience

### Technical Lead — MLX Pilot
**2026 (5 sprints, ongoing)** · Open Source

Led a 6-person team building a Rust/Tauri desktop platform for local LLM
orchestration. I own the daemon architecture, agent runtime integration,
scheduler/job system, channels infrastructure, and cross-sprint
consolidation.

- **Stack:** Rust (tokio, axum, rusqlite), TypeScript (vanilla ES modules),
  Tauri, SQLite, 3 local + 5 cloud LLM providers
- **Highlights:** 15+ crates, 50+ endpoints, 20 channel adapters, 95 tests,
  enterprise security modes, formal release gate

### Full Stack Developer — Enterprise Banking Environment
**Timeline: [needs verification]**

Built operational monitoring dashboards, automated deployment pipelines,
and API integration layers for large-scale banking infrastructure spanning
mainframe-adjacent batch systems, DB2 databases, and Kubernetes/OpenShift
workloads.

- **Stack:** DB2, Kubernetes, OpenShift, CI/CD platforms, scripting
- **Highlights:** Unified monitoring across heterogeneous systems,
  automated health checks replacing manual verification, resilient
  integration layers with graceful degradation

> Detailed case studies in `05-enterprise-experience-sanitized.md`

---

## Projects

### MLX Pilot — Local AI Orchestration Platform
**Rust · Tauri · TypeScript · SQLite · 15+ crates**

Desktop app that runs LLMs locally with no cloud dependency. Multi-provider
routing (MLX, llama.cpp, Ollama + cloud), full agent runtime with tool
calling and policy engine, 20 channel integrations, enterprise security
controls. 95 tests, zero-blocking release gate.

[github.com/MLX-Pilot/MLX-Pilot](https://github.com/MLX-Pilot/MLX-Pilot)

### RTK — Rust Token Killer
**Rust · CLI**

Token-optimized CLI proxy that rewrites shell command output for LLM
context efficiency. 60-90% token savings on common dev operations.

### [Additional projects — needs verification]

---

## Skills

**Primary:** Rust · TypeScript · SQL (SQLite, DB2) · Python · HTML/CSS
**Backend:** Axum · Tokio · REST APIs · SSE/WebSocket · SQLite (rusqlite)
**AI/LLM:** Agent runtimes · Tool calling · Multi-provider routing ·
Embeddings/Semantic search · Prompt engineering
**Desktop:** Tauri · Vanilla JS ES modules · Cross-platform bundling
**Infrastructure:** Kubernetes · OpenShift · CI/CD · Git (advanced)
**Practices:** Test-driven development · Security-first design ·
Observability · Technical leadership

> Full matrix: `02-skills-matrix.md`

---

## Certifications

My background is primarily demonstrated through shipped work. For roles
that require specific certifications, I'm happy to discuss.

> Details: `03-certifications.md`

---

## Contact

- **Email:** kaikevoliveira@gmail.com
- **GitHub:** [github.com/MLX-Pilot](https://github.com/MLX-Pilot)
- **Location:** Brazil
- **LinkedIn:** [needs verification]

---

## Design notes for the site

- **Keep it fast.** No frameworks unless you have a good reason. The MLX
  Pilot frontend runs 28 ES modules with zero build step — the portfolio
  should be lighter, not heavier.
- **Dark theme preferred.** Matches the work (MLX Pilot is dark-themed).
  Use the same CSS variable naming convention if you want.
- **One page, maybe two.** Hero + about + experience + projects + contact
  on one scrollable page. Link out to the detailed profile files for depth.
- **Show, don't claim.** Link to the GitHub repo. The code speaks louder
  than adjectives.
- **No stock photos.** No AI-generated headshots. The MLX Pilot wordmark
  or a simple geometric logo is enough.
