# Skills Validation Report

## Environment

- Date: 2026-04-16T18:54:44.793Z
- Platform: win32 10.0.26100
- Node: v22.19.0
- npm: not available
- cargo: cargo 1.93.1 (083ac5135 2025-12-15)

## Skills tested

- obsidian
- wacli
- gog
- github
- weather
- summarize
- artifact-install

## UI smoke

- Automated via `node --test apps/desktop-ui/e2e/skills-smoke.test.js`.
- Verified enable/disable, install, configure and visual summary refresh without manual reload.

## Real install evidence

- Download install skill: `Download fixture artifact` -> ok=true, code=200
- Structured backend response snapshot:

```json
{
  "node": {
    "ok": true,
    "code": 200,
    "stdout": "C:\\Users\\kaike\\AppData\\Local\\Temp\\mlx-pilot-skills-smoke-knQdo5\\skill-downloads\\artifact-install\\artifact.bin",
    "stderr": "",
    "warnings": [
      "artifact_downloaded_only"
    ]
  }
}
```

## Failure handling

- Network/download failure: ok=false, stderr=error sending request for url (http://127.0.0.1:9/fail)
- Manual install required: ok=false, stderr=manual install required
- Timeout failure: ok=false, stderr=error sending request for url (http://127.0.0.1:56739/slow-artifact.bin)

## Persistence after restart

- `node_package_manager` persisted as `npm`.
- `github` and `summarize` kept secret env refs in the vault-backed config.
- `weather` remained disabled after restart.
- Active skills after restart remained a subset of enabled + eligible skills.

## Reproduction

```bash
cd G:\ai\mlx-ollama-pilot
node --test apps/desktop-ui/e2e/skills-smoke.test.js
cargo test -p mlx-agent-skills -p mlx-agent-core -p mlx-ollama-daemon
node scripts/skills-smoke.mjs
```
