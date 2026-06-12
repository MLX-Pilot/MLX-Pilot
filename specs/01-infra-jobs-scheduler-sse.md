# 01 — Infra: Background Jobs, Scheduler e Convenção SSE

> **Tipo:** infraestrutura habilitadora · **Esforço:** M · **Depende de:** —
> **Habilita:** Deep Research (12), Notes & Tasks (06), Email (14), e qualquer
> tarefa longa/recorrente.

## 1. Objetivo

Fornecer um serviço único, no daemon, para (a) executar **tarefas assíncronas
longas** (pesquisa, polling de e-mail, OCR de upload) com acompanhamento de
progresso e cancelamento; (b) agendar **tarefas recorrentes/cron/uma-vez**; e
(c) padronizar o **streaming de progresso via SSE** para o frontend. Hoje cada
fluxo longo teria que reinventar esse mecanismo — esta spec centraliza isso para
evitar duplicação e inconsistência.

## 2. Contexto Técnico

- **Linguagem/Runtime:** Rust, `tokio` (já no workspace).
- **HTTP:** Axum 0.8; streaming via `axum::response::sse::Sse` ou
  `Body::from_stream` com `text/event-stream` (o daemon já usa NDJSON em
  `chat_stream.rs` — reaproveitar o padrão de `ReceiverStream` + `tokio::mpsc`).
- **Concorrência:** `Arc<RwLock<HashMap<JobId, JobHandle>>>` para registro
  em memória; `tokio::task::JoinHandle` + `tokio_util::sync::CancellationToken`
  (adicionar `tokio-util` ao `crates/daemon/Cargo.toml`).
- **Persistência:** tarefas agendadas duráveis em SQLite (tabela
  `scheduled_tasks` definida na spec 06); progresso efêmero fica só em memória.
- **Agendamento:** loop `tokio::time::interval` (tick de 30–60 s) avaliando
  triggers; expressões cron via `cron` (crate `cron = "0.12"`) ou comparação
  simples de horário para casos `once`/`daily`.
- **Local:** novo módulo `crates/daemon/src/jobs.rs`; campo `jobs: Arc<JobRegistry>`
  em `AppState`.

### Referência no Odysseus (exemplo para consulta)

> O Odysseus (Python/FastAPI) é clonável de
> `https://github.com/pewdiepie-archdaemon/odysseus`. Use os arquivos abaixo como
> referência conceitual de comportamento — **não** porte código Python; reimplemente
> nativamente em Rust.

- `src/task_scheduler.py` — agendador principal (cron/once/recurring, ações).
- `src/bg_jobs.py` e `src/bg_monitor.py` — registro/monitor de jobs em background.
- `src/event_bus.py` — barramento de eventos (modelo para progresso/SSE).
- `src/agent_runs.py` — ciclo de vida de execuções assíncronas do agente.
- `routes/chat_routes.py` / `routes/chat_helpers.py` — streaming SSE de progresso.

## 3. Regras de Negócio e Restrições

- **PODE:** registrar job com `JobId` (uuid), status (`queued|running|done|error|cancelled`),
  `progress` (0–100 + mensagem/fase), resultado e timestamps; emitir eventos de
  progresso; cancelar via token; reaproveitar o provider LLM via `chat_with_routing`.
- **PODE:** expor um endpoint SSE genérico por feature
  (`/<feature>/stream/{job_id}`) e um `POST /<feature>/cancel/{job_id}`.
- **NÃO PODE:** vazar threads — todo job cancelado deve respeitar o
  `CancellationToken` em pontos de checagem (entre rounds/iterações).
- **NÃO PODE:** persistir progresso efêmero no SQLite a cada tick (só o estado
  durável de tarefas agendadas).
- **NÃO PODE:** bloquear o runtime — trabalho CPU-bound deve usar
  `spawn_blocking`; trabalho I/O usa async.
- **Limite:** teto configurável de jobs concorrentes (padrão 4) para não saturar
  modelos locais; jobs excedentes ficam `queued`.

## 4. Critérios de Aceite

- [ ] `JobRegistry` permite `spawn(kind, future) -> JobId`, `get(id)`,
      `cancel(id)`, `list()`.
- [ ] Um job de teste reporta progresso incremental observável via SSE e pode ser
      cancelado no meio (o token interrompe em ≤1 checkpoint).
- [ ] O scheduler dispara uma tarefa `once` no horário e uma `cron` na cadência,
      registrando execução em `task_runs`.
- [ ] Reinício do daemon recarrega tarefas agendadas duráveis do SQLite.
- [ ] `cargo build -p mlx-ollama-daemon` verde; nenhum job órfão após `cancel`.
- [ ] Convenção SSE documentada e reutilizada por ao menos uma feature (Deep Research).

## 5. Plano de Implementação

1. **Tipos base:** definir `JobStatus`, `JobProgress`, `JobRecord`, `JobId` em
   `jobs.rs`; estrutura `JobRegistry { inner: Arc<RwLock<HashMap<...>>>, sem: Semaphore }`.
2. **Spawn/cancel:** implementar `spawn` (cria token, guarda `JoinHandle`,
   respeita `Semaphore`) e `cancel` (dispara token + marca status).
3. **Canal de progresso:** cada job recebe um `mpsc::Sender<JobProgress>`; o
   registry guarda o último progresso + um `broadcast` para múltiplos assinantes SSE.
4. **Endpoint SSE genérico:** helper `sse_for_job(job_id)` que converte o
   `broadcast` em `Sse<EventStream>`; helper para `cancel`.
5. **Scheduler:** task de background com `interval`; lê `scheduled_tasks` do
   SQLite, avalia triggers (`once`/`interval`/`cron`), e chama `spawn` para a ação.
6. **Integração no AppState:** instanciar `JobRegistry` e iniciar o loop do
   scheduler em `serve()` (junto do `StartupCoordinator`).
7. **Smoke test:** rota temporária de teste que cria um job dummy de 5 passos;
   validar progresso/cancelamento via `curl`/SSE; remover ao final.
