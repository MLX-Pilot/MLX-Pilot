# Changelog

Todas as mudancas relevantes deste projeto serao documentadas aqui.

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
