# Release Gate Report

[Ver os vídeos, os pacotes testados e os cenários cobertos na entrega de 22/11/2026.](release-gate/evidence/2026-11-22/README.md)

- Generated at: 2026-11-22T09:54:53.931Z
- Final decision: GO
- Repo: .
- Revision: 24b9525e26bf0194228703d9f2c23ccd008f5a71; dirty: false
- Environment: Node v22.22.0, darwin/arm64
- Scope: full

## Reexecução em Ubuntu e cobertura

O GitHub Actions repetiu os oito blocos em Ubuntu x86_64, instalou o pacote Debian em um ambiente sem modelo e gravou a criação de uma sessão pela interface. O teste de carga da revisão anterior detectou respostas HTTP 502 durante a atualização das contas. Com a gravação atômica das configurações, as 510 requisições locais e as 510 no Ubuntu retornaram HTTP 200, sem timeouts, com até 50 operações simultâneas. Qualquer resposta com erro agora reprova o bloco de carga. [Execução no GitHub](https://github.com/MLX-Pilot/MLX-Pilot/actions/runs/45009094679) · [Resultados do gate em Ubuntu](release-gate/evidence/2026-11-22/gate/ubuntu-ci.json).

Os três instaladores demonstrados correspondem à revisão do aplicativo `24b9525`. O pacote x86_64 foi gerado a partir do merge de teste do GitHub, cuja árvore é idêntica a essa revisão. A primeira tentativa de navegação x86_64 encontrou a janela, mas não acionou o botão da aba. A repetição registrada criou a sessão e validou o rascunho. [Abertura do pacote x86_64](https://github.com/MLX-Pilot/MLX-Pilot/actions/runs/45009094679/job/137168870490).

Os transportes de canais usaram receptores locais. O teste do WhatsApp chegou ao QR de pareamento. O envio por contas de produção, o vínculo físico de um telefone e a inferência com credenciais de nuvem ficaram fora desta execução. O roteiro para a aula inclui as conversas com o modelo local e a abertura dos três pacotes nativos.

## Executive Summary

- Reading Notes and Studio in Chromium: PASS.
- Bloco 1 - Canais e controle de contas: PASS.
- Bloco 2 - Carga curta e concorrencia: PASS.
- Bloco 3 - Upgrade e rollback de migracao: PASS.
- Bloco 4 - Auditoria pratica de seguranca: PASS.
- Bloco 5 - Recuperacao de falha: PASS.
- Bloco 6 - Release dry-run: PASS.
- Plugin broker and bridge security: PASS.

## Objective Criteria

- all_required_blocks_executed_and_passed: pass
- plugin_security_cases_executed_and_passed: pass
- plugin_browser_cases_executed_and_passed: pass
- no_release_blocking_security_findings: pass
- no_state_loss_in_migration_or_restart: pass
- no_crash_under_short_load: pass
- pending_channels_tested_with_explicit_result: pass
- dry_run_completed_or_checklist_validated: pass

## Channels

- bluebubbles: pass
- discord: pass
- feishu: pass
- googlechat: pass
- imessage: pass
- irc: pass
- line: pass
- matrix: pass
- mattermost: pass
- msteams: pass
- nextcloud-talk: pass
- nostr: pass
- signal: pass
- slack: pass
- synology-chat: pass
- telegram: pass
- tlon: pass
- whatsapp: pass
- zalo: pass
- zalouser: pass

## Load Smoke

- /agent/run @ c=10: p50=128ms p95=179ms p99=185ms errors=0 timeouts=0
- /agent/message/send @ c=10: p50=30ms p95=37ms p99=42ms errors=0 timeouts=0
- /agent/channels/probe @ c=10: p50=58ms p95=66ms p99=73ms errors=0 timeouts=0
- /agent/run @ c=25: p50=216ms p95=417ms p99=433ms errors=0 timeouts=0
- /agent/message/send @ c=25: p50=80ms p95=111ms p99=119ms errors=0 timeouts=0
- /agent/channels/probe @ c=25: p50=59ms p95=67ms p99=75ms errors=0 timeouts=0
- /agent/run @ c=50: p50=472ms p95=637ms p99=728ms errors=0 timeouts=0
- /agent/message/send @ c=50: p50=149ms p95=208ms p99=217ms errors=0 timeouts=0
- /agent/channels/probe @ c=50: p50=141ms p95=202ms p99=208ms errors=0 timeouts=0

## Security Findings

- No findings in the completed security audit.

## Browser and daemon

- install: PASS
- configure: PASS
- search: PASS
- document: PASS
- conflict: PASS
- exports: PASS
- revoke: PASS
- disable_enable: PASS
- remove: PASS

## Plugin Security

- Block: pass
- plugin_document_isolation: PASS
- plugin_workspace_isolation: PASS
- plugin_grant_revocation: PASS
- plugin_credentials_redaction: PASS
- plugin_network_authorization: PASS
- plugin_revision_conflict: PASS
- plugin_wasm_limit: PASS
- plugin_request_quota: PASS
- plugin_response_limit: PASS
- bridge_ui_contract: PASS
- sdk_bridge_contract: PASS

## Blockers

- Nenhum bloqueador.

## Reproduction

```bash
cargo build --locked -p mlx-ollama-daemon
cargo test -p mlx-ollama-daemon
npm ci --prefix apps/desktop-ui
npm ci --prefix packages/plugin-sdk
npm ci --prefix plugins/reading-notes
npm run build --prefix plugins/reading-notes
npm run pack --prefix plugins/reading-notes
npm exec --prefix apps/desktop-ui -- playwright install chromium
npm run test:e2e:channels-smoke --prefix apps/desktop-ui
npm run test:e2e:skills-smoke --prefix apps/desktop-ui
node scripts/release-gate/run-all.mjs
```


O relatório de 30/10/2026 está preservado na [revisão histórica](https://github.com/MLX-Pilot/MLX-Pilot/blob/cb21a55/docs/release-gate-report.md).
