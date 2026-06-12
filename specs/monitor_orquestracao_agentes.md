# 20 — Monitor de Orquestração de Agentes

> **Tipo:** nova feature (observabilidade) · **Esforço:** L · **Depende de:**
> EventBus/telemetria existentes; opcionalmente Infra Jobs/SSE (01) para padronizar
> streaming · **Foco principal:** Agent.

## 1. Objetivo

Criar uma **interface de monitoramento de workflows de agentes** dentro do app,
inspirada em terminais de orquestração avançada (estilo console de orquestração
multi-agente). O usuário acompanha, em tempo real, o **raciocínio** da IA, as
**fases** de um run, os **sub-agentes/delegações** envolvidos e as **métricas
agregadas** (tempo, tokens, tarefas ativas).

Resolve a falta de **observabilidade** do agente: hoje a telemetria existe
espalhada (EventBus, budget tracker, audit, sessões), mas não há uma tela única
que mostre "o que o agente está pensando e fazendo agora, quanto custou e em que
estágio está". É um diferencial forte de TCC (transparência e explicabilidade do
agente local).

A tela tem três regiões:

- **Console Central (Logs de Raciocínio):** feed de texto em streaming com o
  pensamento da IA, destacando ações ("Leu um arquivo", "Iniciou um fluxo de
  trabalho", "Chamou ferramenta X") e exibindo metadados do run (ex.: "21 Agentes",
  "4 min 02 s").
- **Barra de Status Global (rodapé persistente):** tempo total de execução,
  contador agregado de **tokens** (ex.: "1.7M tokens") e indicador de **tarefas
  ativas** (ex.: "1 tarefa em execução").
- **Barra Lateral de Fases e Agentes:** agentes agrupados por **Fase** (ex.:
  `Compare`, `Synthesize`); cada agente mostra nome (ex.: `cmp:deep-research`),
  tokens (ex.: `92.4k`), nº de ferramentas usadas e tempo; com **progresso da fase**
  visível.

## 2. Contexto Técnico

- **Backend:** Rust nativo (Tauri) + daemon **Axum** que gerencia estado e expõe
  rotas de API; o frontend **consulta/ouve** o daemon para atualizar métricas em
  tempo real (SSE primário; polling como fallback).
- **Fontes de telemetria já existentes (agregar, não recriar):**
  - `crates/agent-core/src/events.rs` — `EventBus`/`AgentEvent` (pensamento,
    tool-call, tool-result, etc.).
  - `crates/agent-core/src/agent_runtime.rs` — `AgentTurnEvent`,
    `DelegateTaskRequest` (delegações = sub-agentes).
  - `crates/agent-core/src/context_budget.rs` — `ContextBudgetTelemetry`
    (tokens por sessão); já exposto via `AppState.agent_state.budget_tracker`
    (`Arc<RwLock<BTreeMap<String, ContextBudgetTelemetry>>>`) e
    `GET /agent/context/budget`.
  - `crates/agent-core/src/audit.rs` — `AuditLog` (trilha de ferramentas/aprovações),
    via `GET /agent/audit`.
  - Sessões/eventos em `state_store.rs`; streaming já existente em
    `crates/daemon/src/chat_stream.rs` e `POST /agent/stream`.
- **Nova camada de agregação:** módulo `crates/daemon/src/orchestration.rs` com um
  `OrchestrationRegistry` (campo `Arc` no `AppState`) que **assina o EventBus** e
  mantém o estado vivo dos runs (fases, agentes, métricas), além de derivar
  agregados globais. Para runs longos, alinhar com a **Infra Jobs/SSE (spec 01)**.
- **Modelo de dados (telemetria):**
  - `OrchestrationRun { run_id, root_session_id, label, status, started_at,
    ended_at, phases: Vec<Phase>, metrics: RunMetrics }`.
  - `Phase { name, status, progress (0..1), agents: Vec<AgentActivity> }`.
  - `AgentActivity { id, label (ex.: "cmp:deep-research"), status,
    tokens_total, tool_calls, started_at, elapsed_ms }`.
  - `ReasoningEvent { ts, kind (thinking|action|tool_call|tool_result|phase),
    text, meta }`.
  - `GlobalMetrics { active_runs, total_tokens, total_elapsed_ms }`.
- **Frontend:** nova aba **`Monitor`** (padrão `wave1.js`: módulo JS autocontido,
  CSS injetado com variáveis de tema, `esc()` em todo conteúdo). Layout em 3
  regiões; atualização incremental via `EventSource` (SSE).

### Referência no Odysseus (exemplo para consulta)

> O Odysseus é clonável de `https://github.com/pewdiepie-archdaemon/odysseus`.
> A **inspiração visual** é o console de orquestração multi-agente (estilo terminal
> avançado); o Odysseus serve de referência para o **streaming de logs/telemetria**:

- `src/event_bus.py` — barramento de eventos (modelo de fan-out de eventos).
- `src/agent_runs.py`, `src/assistant_log.py` — ciclo de vida de execuções e log
  do assistente (feed de raciocínio).
- `src/bg_monitor.py`, `src/service_health.py`, `routes/diagnostics_routes.py` —
  monitoração de jobs/serviços e métricas.
- `static/js/assistant.js`, `static/js/researchSynapse.js` — UI de progresso/log
  em streaming (cards de fase/agente, barras de progresso).

## 3. Regras de Negócio e Restrições

- **PODE:** exibir runs ativos e o histórico de runs concluídos; mostrar o feed de
  raciocínio, fases, agentes e métricas em tempo real; (opcional) **cancelar** um
  run ativo reutilizando o mecanismo de cancelamento da Infra Jobs (01).
- **PODE:** representar delegações/sub-agentes (`DelegateTaskRequest` / spawn de
  sessão) como `AgentActivity` dentro de uma fase; derivar fases do estágio do run
  ou do agrupamento de delegações.
- **PODE:** agregar tokens a partir de `ContextBudgetTelemetry` e contagem de
  ferramentas a partir do `AuditLog`/eventos.
- **NÃO PODE:** ser um caminho de **controle** do agente além de cancelar — é uma
  tela de **observabilidade** (read-mostly); não altera políticas, prompts ou estado de execução.
- **NÃO PODE:** bloquear o runtime nem o agente — a agregação roda fora do caminho
  crítico (assinante do EventBus); se a UI cair, o agente continua.
- **NÃO PODE:** re-renderizar a tela inteira a cada evento — atualização
  **incremental** (append no feed, patch das linhas de agente/fase) para suportar
  milhares de eventos sem travar.
- **NÃO PODE:** vazar segredos no feed — sanitizar conteúdo (sem API keys/tokens) e
  aplicar `esc()` em tudo.
- **NÃO PODE:** crescer indefinidamente em memória — limitar o buffer de eventos por
  run (ex.: últimos N) e mover runs antigos para histórico persistido (SQLite),
  podados por retenção configurável.
- **Restrição de tempo real:** SSE como canal primário; se o navegador/daemon não
  suportar, cair para polling do snapshot (`GET /agent/orchestration/{run_id}`).
- **Restrição offline/single-user:** tudo local; nenhuma telemetria sai da máquina.

## 4. Critérios de Aceite

- [ ] Iniciar um run do Agent faz aparecer, na aba **Monitor**, um run **ativo** com
      feed de raciocínio em streaming.
- [ ] O **Console Central** exibe o pensamento em streaming e **destaca** ações
      ("Leu um arquivo", "Iniciou um fluxo de trabalho", "Chamou ferramenta…") e
      mostra metadados do run (ex.: "N Agentes", "Xm Ys").
- [ ] A **Barra de Status Global** (rodapé) mostra, em tempo real e persistente,
      tempo total, **tokens agregados** (ex.: "1.7M tokens") e **tarefas ativas**
      (ex.: "1 tarefa em execução").
- [ ] A **Sidebar** agrupa agentes por **Fase** (ex.: `Compare`, `Synthesize`); cada
      agente mostra **nome** (ex.: `cmp:deep-research`), **tokens** (ex.: `92.4k`),
      **nº de ferramentas** e **tempo**; a fase exibe **progresso**.
- [ ] Métricas vêm das fontes reais: tokens de `ContextBudgetTelemetry`, ferramentas
      do `AuditLog`/eventos, tempo de timestamps.
- [ ] Runs **concluídos** ficam no histórico e podem ser reabertos (replay do
      snapshot final).
- [ ] SSE atualiza incrementalmente; queda de conexão reconecta e ressincroniza via
      snapshot; sem SSE, polling mantém a tela funcional.
- [ ] (Opcional) Botão **Cancelar** encerra um run ativo via Infra Jobs (01).
- [ ] `cargo build -p mlx-ollama-daemon` verde; agente continua funcionando se a aba
      Monitor estiver fechada.

## 5. Plano de Implementação

1. **Modelo de telemetria:** definir em `orchestration.rs` os tipos `OrchestrationRun`,
   `Phase`, `AgentActivity`, `ReasoningEvent`, `RunMetrics`, `GlobalMetrics`
   (serializáveis com `serde`).
2. **Registry agregador:** `OrchestrationRegistry` (`Arc` no `AppState`) que **assina
   o `EventBus`** e, a cada `AgentEvent`/`AgentTurnEvent`, atualiza o run
   correspondente (append de evento de raciocínio, incremento de tool-calls, mudança
   de fase/status). Manter `broadcast` para assinantes SSE.
3. **Mapear agentes/fases:** correlacionar `root_session_id` + delegações
   (`DelegateTaskRequest`/spawn) em `AgentActivity`; definir as fases (a partir do
   estágio do run ou do agrupamento de delegações) e calcular `progress`.
4. **Integrar métricas existentes:** ler tokens de `budget_tracker`
   (`ContextBudgetTelemetry`) e contagem de ferramentas do `AuditLog`; consolidar em
   `RunMetrics`/`GlobalMetrics`.
5. **Endpoints (Axum):**
   - `GET /agent/orchestration` — lista de runs (ativos + recentes) + `GlobalMetrics`.
   - `GET /agent/orchestration/{run_id}` — snapshot completo (fases/agentes/eventos/métricas).
   - `GET /agent/orchestration/{run_id}/stream` — **SSE** (feed de raciocínio + deltas de métricas).
   - `GET /agent/orchestration/metrics` — agregados para o rodapé global.
   - (Opcional) `POST /agent/orchestration/{run_id}/cancel` (via Infra Jobs 01).
6. **Persistência + retenção:** ao concluir, gravar o snapshot do run em SQLite
   (tabela `orchestration_runs`, via migração) para o histórico; buffer de eventos
   limitado em memória; poda por retenção configurável.
7. **UI — layout em 3 regiões** (aba `Monitor`):
   - **Console Central:** feed em streaming via `EventSource`; renderizar eventos
     incrementais; **estilizar/realçar** ações por `kind` (ler arquivo, iniciar
     workflow, tool-call…) e exibir badges de metadados do run.
   - **Rodapé global persistente:** assinar `/agent/orchestration/metrics` (ou os
     deltas do SSE) e atualizar tempo/tokens/tarefas-ativas continuamente.
   - **Sidebar de Fases/Agentes:** lista agrupada por fase, com barra de progresso da
     fase e, por agente, nome/tokens/ferramentas/tempo; patch incremental das linhas.
8. **Resiliência do front:** reconexão SSE com ressincronização por snapshot; fallback
   de polling; render incremental (sem reflow global); `esc()` + sanitização.
9. **Histórico:** listar runs concluídos e permitir reabrir (replay do snapshot final).
10. **Smoke test:** disparar um run real do Agent (com delegação/ferramentas),
    validar feed, fases, métricas e rodapé; validar reconexão e histórico.
