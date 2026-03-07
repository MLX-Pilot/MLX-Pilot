# OpenClaw-Compatible Mode

Guia operacional para declarar o MLX-Pilot em modo compativel com OpenClaw sem depender de leitura de codigo.

## Objetivo

Este modo garante:

- matriz automatizada de paridade para channels, plugins, skills e tool profiles
- onboarding nao interativo via API/UI
- migracao versionada de config
- relatorio unico de diagnostico em `GET /agent/compat/report`
- benchmark sintetico de contexto para modelos locais pequenos
- preservacao dos endpoints antigos do agente

## Setup do zero

### 1. Dependencias minimas

- Rust toolchain estavel
- Node.js para a UI desktop e scripts E2E
- ambiente local para o daemon (`cargo run -p mlx-ollama-daemon`)

### 2. Subir o daemon

```bash
cargo run -p mlx-ollama-daemon
```

Health check:

```bash
curl http://127.0.0.1:11435/health
```

### 3. Configurar onboarding nao interativo

Fluxo minimo:

1. `GET /agent/config`
2. `POST /agent/config` com:
   - `provider`
   - `model_id`
   - `base_url` quando usar `custom`
   - `workspace_root`
   - `approval_mode`
   - `execution_mode`
   - `enabled_skills`
   - `tool_policy.profile`

Recomendacao local para validacao:

- `provider: custom`
- `model_id: mock-small`
- `base_url: http://127.0.0.1:<mock-port>`
- `approval_mode: auto`

## Validacao local completa

Rodar a suite end-to-end:

```bash
node /Users/kaike/mlx-ollama-pilot/scripts/openclaw-compat-smoke.mjs
```

O script valida:

1. migracao de config legada para schema v2
2. onboarding completo nao interativo
3. WhatsApp local: conta, login, probe e envio
4. Telegram token-bot: conta, login, probe e envio
5. plugin enable/disable
6. skills check/install/enable/config
7. troca de tools profile
8. execucao de `POST /agent/run`
9. budget telemetry
10. `GET /agent/compat/report`

Arquivos gerados:

- `docs/openclaw-compat-report.json`
- `docs/openclaw-compat-report.md`

## Endpoint de diagnostico

`GET /agent/compat/report` consolida:

- cobertura percentual do modo compativel
- schema/migration status
- matriz de channels
- matriz de plugins
- skills e elegibilidade
- cobertura de tool profiles
- status de backward compatibility dos endpoints antigos
- benchmark de consumo de contexto para `small_local`
- gaps restantes com acao recomendada

## Migracao de configuracao

Schema atual: `v2`

Flags de migracao expostas no report:

- `config_schema_v2`
- `legacy_enabled_tools_sync`
- `compatibility_state_roundtrip`

Comportamento:

- configs antigas sem `schema_version` sao carregadas por merge sobre defaults atuais
- `enabled_tools` legado e sincronizado para `tool_policy.agent_overrides.default`
- o arquivo persistido volta a disco como `schema_version: 2` na proxima mutacao salva

## Canais com ativacao real externa

Os adapters existem e sao cobertos no report, mas alguns canais dependem de bridge/webhook real para uso produtivo:

- `signal`
- `imessage`
- `bluebubbles`
- `nostr`
- `nextcloud-talk`
- `line`
- `zalo`
- `zalouser`
- `tlon`
- `googlechat`
- `feishu`
- `msteams`
- `mattermost`
- `synology-chat`

Checklist de ativacao real:

1. provisionar o bridge/webhook do conector
2. registrar URL/token no account ou `adapter_config`
3. executar `login` e `probe` pelo Control Plane
4. enviar mensagem teste
5. validar logs em `Runtime & Health`

## Backward compatibility

Nao houve remocao de endpoints do agente. O report marca explicitamente os contratos mantidos, incluindo:

- `/agent/config`
- `/agent/skills`
- `/agent/skills/check`
- `/agent/tools`
- `/agent/tools/catalog`
- `/agent/tools/effective-policy`
- `/agent/tools/profile`
- `/agent/plugins`
- `/agent/channels`
- `/agent/context/budget`
- `/agent/run`

## Operacao diaria

Sequencia recomendada:

1. abrir a UI desktop na aba `Agent`
2. ajustar provider/model/profile
3. conectar channels necessarios
4. habilitar plugins necessarios
5. rodar `Skills check`
6. configurar env/keys faltantes
7. consultar `Runtime & Health`
8. validar `compat/report` antes de declarar o modo compativel em producao local
