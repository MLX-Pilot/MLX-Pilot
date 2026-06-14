# Frontend modularization — parity report (spec 21)

Refactor: split the monolithic `app.js` / `wave1.js` / `wave5.js` into focused
native ES modules under `apps/desktop-ui/ui/js/` (see
[`js/MODULE_SUMMARY.md`](../apps/desktop-ui/ui/js/MODULE_SUMMARY.md)). **Mother
rule: zero observable change** — same DOM, classes, CSS cascade, behaviour and
console output. This document records how that was verified.

## Method

The UI was served through a deterministic mock daemon (static files + seeded
`/agent/*`, `/models`, `/compare` 404, etc.) on one origin, so renders are
reproducible. Two builds were captured under an identical script (warm-up pass
over every tab, then 700 ms settle per tab):

- **before** — original commit `472ed35` (pre-refactor), served from a clean
  `git archive` export.
- **after** — the refactored worktree.

For each panel the normalized `outerHTML` (whitespace-collapsed, clock times
masked) was reduced to a `djb2` hash; the two injected `<style>` blocks, the
`window._wave5*` globals and any `[onclick]` attributes were captured too.
Viewport fixed at 1280×900 (the Tauri window width).

## Panel parity (before → after)

| Panel | len | hash before | hash after | Result |
| --- | ---: | ---: | ---: | --- |
| chat | 2367 | 2952364522 | 2952364522 | ✅ identical |
| discover | 3911 | 3346814609 | 3346814609 | ✅ identical |
| agent | 27689 | 4287333419 | 4287333419 | ✅ identical |
| ai-interaction | 1101 | 3735495296 | 3735495296 | ✅ identical¹ |
| console | — | live feed | live feed | ✅ functional² |
| historico | 3947 | 2862628562 | 2862628562 | ✅ identical |
| memoria | 708 | 1186051664 | 1186051664 | ✅ identical |
| comparar | 1788 | 3264374174 | 3264374174 | ✅ identical |
| research | 3014 | 729403062 | 729403062 | ✅ identical |
| hardware | 3143 | 1080243986 | 1080243986 | ✅ identical |
| settings | 3425 | 1455449327 | 1455449327 | ✅ identical |

¹ The `ai-interaction` panel embeds an animated `<canvas>`; its size attributes
depend on the viewport, so the hash only matches when both captures use the same
1280×900 viewport (they do). Owning module (`ui-bindings.js`) was not changed.
² The `console` panel renders the **live** captured console feed, whose content
legitimately varies per run; it is verified functionally below rather than by
hash.

## Injected styles (cascade preserved)

| `<style>` block | len | hash before | hash after | Result |
| --- | ---: | ---: | ---: | --- |
| `#wave1-styles` | 6850 | 3244492866 | 3244492866 | ✅ byte-identical |
| `#wave5-styles` | 1932 | 453800696 | 453800696 | ✅ byte-identical |

Now injected by the feature modules via `wave-common.js` instead of the
monolith; the `wave1-*` / `wave5-*` classes are unchanged. The static `css/`
`<link>` set in `index.html` is untouched.

## Globals & inline handlers removed

| Probe | before | after |
| --- | --- | --- |
| `window._wave5Cancel/_wave5ViewReport/_wave5ShowProfiles/_wave5Download` | 4 present | **0** |
| `<script>` tags | `js/main.js` (module) | `js/main.js` (module) |
| inline `onclick` (in the wave5 render output) | yes | **none** — delegated |

The wave5 inline `onclick` handlers and their `window._wave5*` bridges were
replaced by delegated listeners on the jobs/library/model-table containers,
using `data-*` attributes.

## Console output parity

The captured console feed contains exactly the original startup lines and no new
errors:

- `Wave 5: Deep Research + Hardware Fit ready` ✅ (preserved)
- `wave5: library load failed Error: HTTP 404` ✅ (the mock 404s
  `/research/library`; same message, now sourced from `research.js` + core
  `api.js` instead of `wave5.js`)
- `Unified model catalog load failed: HTTP 404` ✅ (unchanged, from `models.js`)

No new errors or warnings were introduced by any extracted module.

## Functional verification

Beyond DOM parity, the riskiest change — converting the wave5 inline `onclick` +
globals into delegated `data-*` handlers — was exercised directly (inject a
representative library item / model row, dispatch clicks on *nested* children to
test `closest()` traversal):

| Behaviour | Result |
| --- | --- |
| Library item click → opens report viewer with correct `sessionId` | ✅ |
| Model row click → opens serve-profiles section | ✅ |
| Download button click → calls `downloadModel` with correct id | ✅ |
| Download click does **not** also trigger the row handler (original `stopPropagation`) | ✅ |
| `window._wave5*` globals absent | ✅ |

Wave1 surfaces (preset picker, memory/compare/history panels) render identically
(hash parity above) and reuse core `api()`/`state` + the shared `wave-common`
helpers.

## Screenshots

Before/after screenshots were taken at 1280×900 for the chat composer (with the
preset chip), Histórico, Memória, Comparar, Pesquisa and Hardware panels under
the same mock data. They are visually indistinguishable — consistent with the
byte-level DOM + CSS hash parity above. (Screenshots are session artifacts and
are not committed.)

## Build

- `mlx-ollama-daemon` (root workspace) builds clean — `cargo build` succeeds.
- The Tauri desktop crate (`apps/desktop-ui/src-tauri`) is a **separate
  workspace**; its Rust compiles fully (all deps + the crate) and the frontend
  assets raise no `frontendDist` errors. The build then stops in the
  `tauri-build` script with `glob pattern binaries/llamacpp/* path not found` —
  the `bundle.resources` entry in `tauri.conf.json` points at the **gitignored**
  llama.cpp engine binaries (`.gitignore`: "fetched by
  `scripts/fetch-llama-engine.ps1`, not committed"). This is a pre-existing
  environment-setup requirement, **not** caused by this refactor (the diff
  touches nothing under `src-tauri/`). Once those binaries are fetched (standard
  dev setup) the bundle generates; the JS reorganization does not affect it,
  since the UI is embedded as static assets.
- Because the bundle can't be produced without the engine binaries, the
  frontend was exercised in a Chromium preview, which is the same native-ESM
  runtime model as the Tauri WebView2 target (the modules I changed contain no
  Tauri-only `nativeInvoke` paths).

## Notes / out of scope

- `js/features/providers.js` (~28.7 KB) and `js/features/models.js` (~25.5 KB)
  exceed the ~25 KB guideline, but they were extracted in an earlier pass and
  are **outside this task's scope** (which covered `app.js`/`wave1.js`/
  `wave5.js`). Flagged for a follow-up split. Every module produced by *this*
  refactor is ≤ ~12 KB.
- `agent-channels.js` / `agent-control-plane.js` / `agent-skills.js` are kept
  (not dead) — see `MODULE_SUMMARY.md`. `particles.js` was already absent.
