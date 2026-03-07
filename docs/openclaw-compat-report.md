# OpenClaw Compatibility Report

- Generated at: 2026-03-07T01:01:35.644821+00:00
- Mode: openclaw-compatible
- Coverage: 98.2% (56/57)
- Critical gaps: 0
- Warning gaps: 2
- Config schema: v2
- Migration flags: config_schema_v2, legacy_enabled_tools_sync, compatibility_state_roundtrip

## Validated flows

- onboarding non-interativo atualizado via /agent/config
- WhatsApp local: upsert, login, probe e envio de mensagem
- Telegram token-bot: upsert, login, probe e envio de mensagem
- Plugin memory habilitado e desabilitado sem regressao
- Skills check/install/enable/config executados de ponta a ponta
- Troca de tools profile para full com politica efetiva sincronizada
- Agent loop executado com provider OpenAI-compatible local + budget telemetry
- Compatibility matrix consolidada via /agent/compat/report com cobertura >= 95%

## Channels

- bluebubbles: adapter_ready_external, contas=0, local_testable=false
- discord: supported_local, contas=0, local_testable=true
- feishu: adapter_ready_external, contas=0, local_testable=false
- googlechat: adapter_ready_external, contas=0, local_testable=false
- imessage: adapter_ready_external, contas=0, local_testable=false
- irc: supported_local, contas=0, local_testable=true
- line: adapter_ready_external, contas=0, local_testable=false
- matrix: supported_local, contas=0, local_testable=true
- mattermost: adapter_ready_external, contas=0, local_testable=false
- msteams: adapter_ready_external, contas=0, local_testable=false
- nextcloud-talk: adapter_ready_external, contas=0, local_testable=false
- nostr: adapter_ready_external, contas=0, local_testable=false
- signal: adapter_ready_external, contas=0, local_testable=false
- slack: supported_local, contas=0, local_testable=true
- synology-chat: adapter_ready_external, contas=0, local_testable=false
- telegram: supported_local, contas=1, local_testable=true
- tlon: adapter_ready_external, contas=0, local_testable=false
- whatsapp: supported_local, contas=1, local_testable=true
- zalo: adapter_ready_external, contas=0, local_testable=false
- zalouser: adapter_ready_external, contas=0, local_testable=false

## Plugins

- auth: managed, enabled=false, health=disabled
- automation-helpers: managed, enabled=false, health=disabled
- device-pair: managed, enabled=false, health=disabled
- diffs: managed, enabled=false, health=disabled
- memory: managed, enabled=false, health=disabled
- voice-call: managed, enabled=false, health=disabled

## Skills

- mock-install: active, eligible=true, active=true
- summarize: active, eligible=true, active=true
- weather: active, eligible=true, active=true

## Tool Profiles

- minimal: coverage=100%, allowed=7, blocked=0
- coding: coverage=100%, allowed=12, blocked=0
- messaging: coverage=100%, allowed=8, blocked=0
- full: coverage=100%, allowed=13, blocked=0

## Context Benchmark

- Model: qwen2.5-coder:7b
- Profile: small_local
- Status: tight
- Prompt tokens: 1177/1200
- Summaries: 1
- Recommendation: Reduzir ferramentas expostas e manter profile small_local com compressao agressiva.

## Remaining Gaps

- [warning] channels/external_activation: 14 channel adapters depend on external bridge/webhook activation for production use. Action: Usar o mock E2E local para validar o adapter e seguir o checklist de ativacao real no ambiente alvo.
- [warning] context/small_local_budget: Synthetic small-local benchmark hit critical context headroom. Action: Reduzir ferramentas expostas e manter profile small_local com compressao agressiva.

## Re-run

```bash
node /Users/kaike/mlx-ollama-pilot/scripts/openclaw-compat-smoke.mjs
```
