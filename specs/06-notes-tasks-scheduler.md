# 06 — Notes & Tasks (Notas, To-dos e Tarefas Agendadas)

> **Tipo:** nova feature · **Esforço:** L · **Depende de:** Infra Jobs/Scheduler (01)
> **Habilita:** automações do agente, lembretes, rotinas.

## 1. Objetivo

Oferecer **notas rápidas** (com lembrete e checklist), uma **lista de to-dos**, e
**tarefas agendadas** (cron/uma-vez/recorrente) que o agente executa
automaticamente — emitindo notificações. Resolve a falta de persistência de
"coisas a fazer" e de qualquer automação proativa do agente.

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/notes_tasks.rs`; tabelas `notes`,
  `scheduled_tasks`, `task_runs` (SQLite, via migração).
- **Scheduler:** usa diretamente a **infra 01** (loop de tick + triggers
  `once|interval|cron`). Cron via crate `cron`.
- **Ações de tarefa:** `llm_prompt` (via `chat_with_routing`), `agent_run`
  (dispara o agente), `builtin` (ex.: rodar uma skill); resultado salvo em `task_runs`.
- **Notificações:** canais `toast` (UI), `email` (via SMTP — cofre, ver spec 14),
  `webhook`/`ntfy` (HTTP). MVP: `toast` + `webhook`.
- **Frontend:** aba `Notas & Tarefas` (padrão `wave1.js`): grade de notas
  (cor/fixar/vencimento/checklist) + lista de tarefas com construtor de agendamento
  e histórico de execuções.

### Referência no Odysseus (exemplo para consulta)

- `routes/note_routes.py` — notas, lembretes, checklist.
- `routes/task_routes.py` + `src/task_scheduler.py` — tarefas agendadas e execução.
- `static/js/notes.js`, `static/js/tasks.js` — UI de notas e tarefas/agendamento.

## 3. Regras de Negócio e Restrições

- **PODE:** CRUD de notas/to-dos; criar tarefa com trigger e ação; rodar tarefa
  manualmente ("run now"); pausar/retomar; ver histórico.
- **PODE:** uma tarefa disparar o agente com um prompt/skill definidos.
- **NÃO PODE:** executar comandos arbitrários fora das políticas do agente
  (`PolicyEngine`/`ApprovalService` continuam valendo).
- **NÃO PODE:** rodar tarefas sobrepostas do mesmo id (lock/skip se a anterior
  ainda roda).
- **NÃO PODE:** enviar notificação a canal não configurado (validar credenciais
  no cofre antes).
- **Restrição:** persistir tarefas duráveis no SQLite; sobreviver a reinício.

## 4. Critérios de Aceite

- [ ] CRUD de notas (`/api/notes*`) com fixar/cor/checklist/vencimento.
- [ ] CRUD de tarefas (`/api/tasks*`) com trigger `once|interval|cron` e ação.
- [ ] Tarefa `once` dispara no horário; `cron` na cadência; execução registrada
      em `task_runs` (status, saída, erro).
- [ ] "Run now", pausar/retomar e histórico funcionam na UI.
- [ ] Notificação `toast` aparece na UI quando uma tarefa termina; `webhook`
      dispara HTTP.
- [ ] Reinício do daemon recarrega e continua honrando as tarefas.

## 5. Plano de Implementação

1. **Tabelas** `notes`, `scheduled_tasks`, `task_runs`; CRUD em `state_store.rs`.
2. **Endpoints** de notas e tarefas (CRUD, run-now, pause, runs).
3. **Integração com infra 01:** o scheduler lê `scheduled_tasks` e dispara as ações.
4. **Executores de ação:** `llm_prompt`, `agent_run`, `builtin` (respeitando políticas).
5. **Notificações:** `toast` (canal SSE/poll para a UI) + `webhook` (HTTP).
6. **UI:** aba `Notas & Tarefas` — grade de notas + lista de tarefas + construtor
   de agendamento + histórico; polling de notificações.
