# Skills Validation Report

## Environment

- Date: 2026-03-06T22:20:55.555Z
- macOS: 26.3
- Node: v22.22.0
- npm: 10.9.4
- go: go version go1.25.7 darwin/arm64
- brew: Homebrew 5.0.15-50-g3e682d3

## Skills tested

- obsidian
- wacli
- gog
- github
- weather
- summarize
- node-real-install

## UI smoke

- Automated via `node --test apps/desktop-ui/e2e/skills-smoke.test.js`.
- Verified enable/disable, install, configure and visual summary refresh without manual reload.

## Real install evidence

- Node install skill: `Install npm-check-updates` -> ok=true, code=0
- Go install skill: `Install stringer via go` -> ok=true, code=0
- Structured backend response snapshot:

```json
{
  "node": {
    "ok": true,
    "code": 0,
    "stdout": "\nadded 1 package in 1s\n",
    "stderr": "npm notice\nnpm notice New major version of npm available! 10.9.4 -> 11.11.0\nnpm notice Changelog: https://github.com/npm/cli/releases/tag/v11.11.0\nnpm notice To update run: npm install -g npm@11.11.0\n",
    "warnings": []
  },
  "go": {
    "ok": true,
    "code": 0,
    "stdout": "",
    "stderr": "",
    "warnings": []
  }
}
```

## Failure handling

- Network/download failure: ok=false, stderr=error sending request for url (http://127.0.0.1:9/fail)
- Permission failure: ok=false, stderr=permission denied
- Timeout failure: ok=false, stderr=command timed out after 1s

## Persistence after restart

- `node_package_manager` persisted as `npm`.
- `github` and `summarize` kept secret env refs in the vault-backed config.
- `weather` remained disabled after restart.
- Active skills after restart remained a subset of enabled + eligible skills.

## Limitations

- The Tauri window was built locally, but UI interaction evidence is headless via jsdom smoke instead of native window automation.
- Real install coverage used `node` and `go`; `brew` remained available but was not required because `go` satisfied the acceptance gate.

## Reproduction

```bash
cd /Users/kaike/mlx-ollama-pilot
node --test apps/desktop-ui/e2e/skills-smoke.test.js
cargo test -p mlx-agent-skills -p mlx-agent-core -p mlx-ollama-daemon
node scripts/skills-smoke.mjs
cd apps/desktop-ui/src-tauri && cargo tauri build
```
