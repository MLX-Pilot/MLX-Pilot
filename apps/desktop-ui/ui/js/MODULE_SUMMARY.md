# Frontend module map

The MLX Pilot desktop UI is a **build-free, native ES-module** frontend served
by Tauri from `apps/desktop-ui/ui` (`frontendDist: "../ui"`). `index.html` loads
a single entry point — `<script type="module" src="js/main.js">` — and every
behaviour lives in focused ES modules under `js/`. There is **no bundler and no
Node build step**; the WebView2/WKWebView/WebKitGTK targets load `import`/
`export` natively.

This file is the conceptual map of those modules (modelled on Odysseus's
`static/js/MODULE_SUMMARY.md`). It replaced the previous monolith
(`app.js` ~159 KB, `wave1.js` ~46 KB, `wave5.js` ~26 KB, all flat IIFEs).

## Entry point

- **`js/main.js`** — imports the modules in the exact order their side effects
  ran in the old monolith (console capture → core feature modules → the former
  wave1 features → the former wave5 features). Importing a feature module runs
  its top-level wiring (event listeners, tab binders, style injection).

## `js/core/` — shared primitives (imported by features, never the reverse)

| Module | Responsibility |
| --- | --- |
| `state.js` | The single mutable `state` object + config constants + `localStorage` readers. Imported by reference, never reassigned. |
| `api.js` | `nativeInvoke` (Tauri bridge), the `api()` fetch wrapper against the daemon, and the SSE stream-decoder factory. |
| `dom.js` | Pure helpers: `esc` (text-node escaping), byte/number/duration formatting, model icon, `showToast`, `runConfirmation`. |
| `markdown.js` | `renderMarkdown`. |
| `router.js` | `switchTab()` + tab / agent-view / config-subtab / model-picker / discover-subtab click wiring. |
| `console-capture.js` | Installs the global `console.*` override + window error/rejection hooks; exposes `pushConsoleEntry`. Imported first by `main.js`. |

## `js/features/` — one module per surface

| Module | Tab / surface | Daemon endpoints |
| --- | --- | --- |
| `runtime.js` | Splash, boot, daemon resolution, topbar/sidebar status | `/health`, `/runtime/*` |
| `providers.js` | Agent provider profiles + cloud catalogs | `/agent/provider-profiles`, `/models/*` |
| `settings.js` | Daemon config form | `/config` |
| `models.js` | Modelos / Discover, downloads, model picker | `/models`, `/catalog/*` |
| `chat.js` | Chat tab: streaming, sessions sidebar | `/chat`, `/agent/sessions` |
| `agent.js` | Agent workspace, audit, plugins/skills/tools/channels | `/agent/*` |
| `console.js` | Console + environment editor | `/environment`, native log bridge |
| `agent-shortcuts.js` | Natural-language agent shortcut replies | `/agent/*` |
| `ui-bindings.js` | Composition-root wiring, AI-visual canvas, global keyboard shortcuts | — |
| `presets.js` | Chat composer preset picker | `/agent/presets` |
| `memory.js` | Memória tab | `/agent/memory*` |
| `compare.js` | Comparar tab | `/compare/*` |
| `history.js` | Histórico tab | `/agent/sessions/*` |
| `research.js` | Pesquisa (Deep Research) + SSE | `/research/*`, `/api/research/stream` |
| `hardware.js` | Hardware & Model Fit | `/api/hwfit/*`, `/catalog/downloads` |
| `wave-common.js` | Shared presentation helpers for the six former wave modules | — |

### `wave-common.js`

`presets`, `memory`, `compare`, `history`, `research` and `hardware` came out of
the old `wave1.js`/`wave5.js` and share a small presentation kit:

- `esc` — **attribute-safe** HTML escaping (escapes `"` and stringifies falsy
  values). This intentionally differs from `core/dom.esc` (text-node escaping):
  the wave templates interpolate values into HTML *attributes*, so swapping in
  `dom.esc` would change escaping behaviour. Kept separate for parity.
- `el`, `fmtDate`, `toast` (the `#wave1-toast` notice), `openModal` (the
  `.wave1-overlay` modal).
- `injectWave1Styles()` / `injectWave5Styles()` — inject the `#wave1-styles` /
  `#wave5-styles` `<style>` blocks (byte-identical to the originals; the classes
  are namespaced `wave1-*` / `wave5-*`, so the cascade is preserved). They are
  idempotent and called by each feature that needs them.

HTTP for these features goes through **core `api()`**; only `/compare/run` and
`/compare/{id}/synthesize` (multi-model generation / LLM judge) pass an explicit
generous `timeoutMs`, because the original wave client had no client-side
timeout. The `hwfit`/`catalog` endpoints keep raw `fetch` + `r.json()` to
preserve their original parse-error surfacing.

## CSS

Static styles live in `css/` (`theme.css`, `base.css`, `layout.css`,
`components.css`, `animations.css`), linked from `index.html` — split from the
former monolithic `styles.css` with an identical cascade. The two `wave*` style
blocks remain **JS-injected** (the established per-wave pattern) via
`wave-common.js`.

## Not wired into the live app

`agent-channels.js`, `agent-control-plane.js` and `agent-skills.js` sit at the
`ui/` root and are **not** imported by `index.html`/`main.js`. They are *not*
dead code: each is imported by a committed Node `--test` E2E suite
(`e2e/{channels,agent-control-plane,skills}-smoke.test.js`, run via the
`test:e2e:*` npm scripts, with `jsdom` as a declared devDependency) and exports a
tested controller (`createAgent*Controller`) staged for future wiring. They are
kept with justification rather than removed.
