# Especificações Técnicas — MLX-Pilot

Este diretório contém os **Documentos de Requisitos Técnicos (DRT)** das features
ainda não implementadas no MLX-Pilot, derivadas da análise comparativa com o
[Odysseus](https://github.com/pewdiepie-archdaemon/odysseus).

Cada arquivo segue estritamente a estrutura:

1. **Objetivo** — o que a feature faz e qual problema resolve.
2. **Contexto Técnico** — linguagens, frameworks e bibliotecas.
3. **Regras de Negócio e Restrições** — o que o código PODE e NÃO PODE fazer.
4. **Critérios de Aceite** — o que define a feature como 100% pronta.
5. **Plano de Implementação** — passo a passo lógico de desenvolvimento.

## Princípios transversais (valem para TODAS as specs)

- **App nativo, sempre.** Backend em Rust (workspace Cargo), UI desktop em Tauri
  com frontend estático (HTML/JS). **Proibido**: Docker como dependência de
  runtime, servidor web Python, ou qualquer caminho que torne o app
  "somente-navegador". Tudo roda local, single-user, offline-first.
- **Persistência unificada.** Usar o SQLite compartilhado em
  `<data_dir>/agent/state.sqlite` através dos *stores* públicos de
  `crates/agent-core`. Tabelas novas entram pelo mecanismo de migração versionada
  (`MIGRATIONS` em `crates/agent-core/src/state_store.rs`). Nada de espalhar
  arquivos JSON soltos.
- **Segredos.** Credenciais (SMTP, API keys, tokens CalDAV/MCP) só no cofre
  criptografado (`crates/daemon/src/secrets_vault.rs`). Nunca em JSON puro.
- **Chamadas LLM internas.** Reutilizar `chat_with_routing(&state, ChatRequest)`
  do daemon (roteia para mlx/llamacpp/ollama/remoto). Nada de cliente HTTP novo.
- **Padrão de endpoint.** Axum 0.8 em `crates/daemon/src/lib.rs`: handler
  `State(AppState)` + `Json<Req>` → `Result<Json<T>, ApiError>`; registrar a rota
  antes de `.with_state(state)`.
- **Padrão de UI.** Aba (`.tab[data-panel="x"]`) + painel (`#panel-x`) no
  `index.html`; lógica num módulo JS autocontido carregado após `app.js` (modelo
  do `wave1.js`): `daemonUrl()`, `api()`, CSS injetado com as variáveis de tema,
  e `esc()` em todo conteúdo dinâmico. O `switchTab()` já cuida do show/hide.

## Ordem recomendada de implementação

### Infraestrutura habilitadora
- [`01-infra-jobs-scheduler-sse.md`](01-infra-jobs-scheduler-sse.md)
- [`02-semantic-memory-embeddings.md`](02-semantic-memory-embeddings.md)
- [`03-web-search-providers.md`](03-web-search-providers.md)

### Destaque — App híbrido (cloud + local)
- [`19-hybrid-cloud-local-models.md`](19-hybrid-cloud-local-models.md) — seletor
  unificado que mostra modelos locais **e** cloud (DeepSeek/OpenAI/…) quando a API
  key é salva no cofre. Foco no Agent. Reaproveita os providers remotos já existentes.

### Refator de base (frontend)
- [`21-frontend-modularization.md`](21-frontend-modularization.md) — quebrar o
  frontend monolítico (`app.js` ~159 KB) em módulos ES nativos, **sem bundler** e
  **sem mudar nada visual**. Viabiliza features futuras como "novo módulo + nova
  aba". Refator puro com paridade total.

### Observabilidade do Agente
- [`monitor_orquestracao_agentes.md`](monitor_orquestracao_agentes.md) — tela de
  monitoramento de workflows de agentes: console de raciocínio em streaming, rodapé
  global (tempo/tokens/tarefas ativas) e sidebar de fases/agentes. Agrega a
  telemetria que o daemon já produz (EventBus, budget, audit, sessões) via SSE.

### Wave 2 — alto valor, esforço médio
- [`04-uploads-pdf-vision.md`](04-uploads-pdf-vision.md)
- [`05-documents-editor.md`](05-documents-editor.md)
- [`06-notes-tasks-scheduler.md`](06-notes-tasks-scheduler.md)
- [`07-mcp-server-management.md`](07-mcp-server-management.md)
- [`08-slash-commands-skills.md`](08-slash-commands-skills.md)
- [`09-speech-stt-tts.md`](09-speech-stt-tts.md)
- [`10-theme-editor.md`](10-theme-editor.md)
- [`11-backup-restore-vault-ui.md`](11-backup-restore-vault-ui.md)

### Wave 3 — pesado / opcional (avaliar escopo de TCC)
- [`12-deep-research.md`](12-deep-research.md)
- [`13-cookbook-hardware-fit.md`](13-cookbook-hardware-fit.md)
- [`14-email.md`](14-email.md)
- [`15-calendar.md`](15-calendar.md)
- [`16-gallery-image-editor.md`](16-gallery-image-editor.md)
- [`17-contacts.md`](17-contacts.md)
- [`18-auth-multiuser-2fa.md`](18-auth-multiuser-2fa.md)

> Já entregues (fora deste diretório): Presets, Memória (keyword/FTS),
> Sessions & History, Compare, e o mecanismo de migrações SQLite.
