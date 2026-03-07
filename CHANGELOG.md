# Changelog

Todas as mudancas relevantes deste projeto serao documentadas aqui.

## [Unreleased] - 2026-03-07

### feat(openclaw-compatible)

- Endpoint unico `GET /agent/compat/report` com coverage, matrices, gaps e benchmark de contexto.
- Config schema versionado (`schema_version: 2`) com migracao backward-compatible de configs legadas.
- Suite E2E local `scripts/openclaw-compat-smoke.mjs` cobrindo onboarding, channels, plugins, skills, tools profile e agent loop.
- Geracao automatica de `docs/openclaw-compat-report.json` e `docs/openclaw-compat-report.md`.
- Guia operacional dedicado em `docs/openclaw-compatible-mode.md`.

### compat

- Endpoints antigos do agente preservados.
- `enabled_tools` legado sincronizado com `tool_policy.agent_overrides.default`.
- Relatorio explicita canais que exigem ativacao real por bridge/webhook sem sacrificar policy/seguranca.

### migration

- Flags de migracao:
- `config_schema_v2`
- `legacy_enabled_tools_sync`
- `compatibility_state_roundtrip`

### breaking changes

- Nenhum endpoint removido.
- Config persistida passa a gravar `schema_version: 2`.
- Workflows que liam `enabled_tools` diretamente devem considerar `tool_policy` como fonte efetiva de politica.

## [v0.1.0-agent-preview] - 2026-02-22

### feat(agent)

- AgentLoop em Rust integrado ao daemon do MLX-Pilot.
- Tool-calling com validacao por schema e filtro de tools por contexto.
- Prompt builder adaptativo para modelos locais/remotos.
- Skill runtime com compatibilidade de `SKILL.md` e sumarios compactos.
- Endpoints `agent/*` para execucao, configuracao, skills, tools, audit e approvals.

### feat(ui)

- Aba **Agent** integrada na UI desktop.
- Configuracao de provider/modelo/API key/base URL.
- Controles de execution mode, fallback e streaming.
- Listagem e toggle de skills/tools com reload.
- Visualizacao de auditoria e controles de aprovacao.

### feat(security)

- `PolicyEngine` com regras de allow/deny por glob.
- Bloqueio de paths sensiveis e egress allowlist por dominio.
- `ApprovalService` com modos `auto`, `ask` e `deny`.
- `AuditLog` JSONL com trilha de ferramentas e decisoes.
- Modo enterprise/paranoid:
- capabilities declarativas por skill (`fs_read`, `fs_write`, `network`, `exec`, `secrets_access`)
- integridade de skills (SHA256 + pin opcional + aviso de alteracao)
- secrets vault local criptografado
- modo airgapped e owner-only

### feat(providers)

- Registro unificado de providers locais e remotos.
- Suporte a OpenAI-compatible (incluindo OpenRouter/Groq/custom).
- Suporte dedicado para Anthropic e DeepSeek.
- Runtime de provider com `base_url`, `api_key` e headers customizados.
- Fallback automatico opcional entre providers.
