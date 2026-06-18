# 00 — Source Inventory

> **Date inspected:** 2026-06-16
> **Scope:** All available material in the MLX-Pilot repository and associated
> Claude memory files. No external resumes, certificates, or profile pictures
> were found in the expected directories (`docs/private-source/`,
> `public/assets/`).

## Sources inspected

| # | Source | Type | Contains personal data? |
|---|--------|------|------------------------|
| 1 | `README.md` + `README_DEV.md` | Public project | No — project description |
| 2 | `changelog.md` | Public project | Yes — author names, sprint structure |
| 3 | `changelog_users.md` | Public project | Yes — per-author contribution breakdown |
| 4 | `docs/release-gate-report.md` | Public project | Yes — filesystem path hinting at macOS username |
| 5 | `docs/video-scripts-ptbr.md` | Public project | No — promotional scripts |
| 6 | `docs/hermes-inspired-runtime.md` | Public project | No — architecture |
| 7 | `docs/llm-ecosystem-assimilation-2026-04-18.md` | Public project | No — ecosystem review |
| 8 | `docs/mlx-pilot-review-2026-04-18.md` | Public project | No — project review |
| 9 | `docs/runtime-hardening-cycle-2026-04-18.md` | Public project | No — hardening review |
| 10 | `docs/skills-validation-report.md` | Public project | No — skills report |
| 11 | `docs/agent_architecture.md` | Public project | No — architecture |
| 12 | `docs/agent-tool-parity.md` | Public project | No — tool parity matrix |
| 13 | `docs/channel-bridge-protocol-v1.md` | Public project | No — protocol spec |
| 14 | `docs/channel-transports.md` | Public project | No — transport docs |
| 15 | `docs/frontend-modularization-parity.md` | Public project | No — frontend report |
| 16 | `docs/local-runtime-doctor.md` | Public project | No — runtime doctor |
| 17 | `docs/research/` directory | Public project | No — deep research docs |
| 18 | `specs/01` through `specs/08` + additional specs | Public project | No — feature specs (reference Odysseus at `github.com/pewdiepie-archdaemon/odysseus`) |
| 19 | `temp/odysseus/` directory | Reference codebase | No — Python/FastAPI reference implementation |
| 20 | Git history (commit authors, messages) | Public project | Yes — author name, email pattern |
| 21 | Git remote (`github.com/MLX-Pilot/MLX-Pilot`) | Public project | Yes — GitHub org |
| 22 | Claude memory: `preview-ui-verification.md` | Reference | No — technical note |
| 23 | Claude memory: `agent-telemetry-sources.md` | Reference | No — technical note |
| 24 | Claude global `CLAUDE.md` (RTK.md) | Reference | No — token-optimization CLI tool reference |
| 25 | `crates/` source code (Rust) | Public project | No — application code |
| 26 | `apps/desktop-ui/` source code (JS/CSS/HTML) | Public project | No — frontend code |

## Sources NOT found

| Expected location | Type | Status |
|-------------------|------|--------|
| `docs/private-source/` | Resume, certificates, profile pictures, internal presentations | **Directory does not exist** |
| `public/assets/` | Public-facing assets | **Directory does not exist** |
| Any `.pdf`, `.jpg`, `.png` containing personal/professional documents | Resume, certificates, photos | **Not found** |
| LinkedIn export, CV document, certification PDFs | Professional verification | **Not found** |

## What was extractable

The repository itself is a high-signal source. From commit history, changelogs,
and project structure we can derive:

- **Identity:** Kaike Vitorino (kaikevoliveira@gmail.com)
- **Role:** Technical lead and primary integrator across 5 sprints (30 commits,
  most of any contributor)
- **Team:** Led a 6-person team through a Rust/Tauri LLM orchestration platform
- **Technical depth:** Rust (async/tokio, axum, SQLite), Tauri desktop, agent
  runtimes, multi-provider LLM routing, policy engines, secrets vault, SSE/SSE
  streaming, channels/webhooks, observability
- **Contextual clues:** References to enterprise/banking patterns (Getronics
  in spec 06 notes), enterprise/paranoid security modes throughout the codebase
- **Public GitHub:** `github.com/MLX-Pilot/MLX-Pilot`

## Limitations

Without resumes, certificates, or explicit enterprise documentation, sections
marked **[needs verification]** in other profile files require the author to
fill in details. The profile was built conservatively — nothing was invented.
