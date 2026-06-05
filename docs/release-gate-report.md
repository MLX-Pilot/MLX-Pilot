# Release Gate Report

- Generated at: 2026-03-07T01:29:23.366Z
- Final decision: GO
- Repo: /Users/kaike/mlx-ollama-pilot

## Executive Summary

- Bloco 1 - Canais bridge/webhook: PASS.
- Bloco 2 - Carga curta e concorrencia: PASS.
- Bloco 3 - Upgrade e rollback de migracao: PASS.
- Bloco 4 - Auditoria pratica de seguranca: PASS.
- Bloco 5 - Recuperacao de falha: PASS.
- Bloco 6 - Release dry-run: PASS.

## Objective Criteria

- no_release_blocking_security_findings: pass
- no_state_loss_in_migration_or_restart: pass
- no_crash_under_short_load: pass
- pending_channels_tested_with_explicit_result: pass
- dry_run_completed_or_checklist_validated: pass

## Channels

- bluebubbles: pass
- feishu: pass
- googlechat: pass
- imessage: pass
- line: pass
- mattermost: pass
- msteams: pass
- nextcloud-talk: pass
- nostr: pass
- signal: pass
- synology-chat: pass
- tlon: pass
- zalo: pass
- zalouser: pass

## Load Smoke

- /agent/run @ c=10: p50=34ms p95=49ms p99=50ms errors=0 timeouts=0
- /agent/message/send @ c=10: p50=4ms p95=9ms p99=10ms errors=0.05 timeouts=0
- /agent/channels/probe @ c=10: p50=5ms p95=7ms p99=8ms errors=0.1 timeouts=0
- /agent/run @ c=25: p50=36ms p95=51ms p99=51ms errors=0 timeouts=0
- /agent/message/send @ c=25: p50=13ms p95=18ms p99=20ms errors=0.06 timeouts=0
- /agent/channels/probe @ c=25: p50=8ms p95=17ms p99=17ms errors=0.2 timeouts=0
- /agent/run @ c=50: p50=63ms p95=101ms p99=105ms errors=0 timeouts=0
- /agent/message/send @ c=50: p50=27ms p95=40ms p99=42ms errors=0.04 timeouts=0
- /agent/channels/probe @ c=50: p50=23ms p95=30ms p99=31ms errors=0.03 timeouts=0

## Security Findings

- No findings.

## Blockers

- Nenhum bloqueador.

## Reproduction

```bash
cargo check -p mlx-ollama-daemon
cargo test -p mlx-ollama-daemon
cd apps/desktop-ui && npm run test:e2e:channels-smoke
cd apps/desktop-ui && npm run test:e2e:skills-smoke
node scripts/release-gate/run-all.mjs
```

