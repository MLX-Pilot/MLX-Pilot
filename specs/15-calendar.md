# 15 — Calendário (Local + CalDAV)

> **Tipo:** nova feature · **Esforço:** L (local) → maior com CalDAV
> **Depende de:** Infra Jobs (01, sync) · **Escopo TCC:** versão local primeiro;
> CalDAV opcional.

## 1. Objetivo

Um calendário local-first (eventos, recorrência, lembretes) com **sync CalDAV
opcional** (Radicale/Nextcloud/Apple/Fastmail) e import/export `.ics`, ciente do
agente (criar evento por linguagem natural). Resolve a ausência de gestão de
tempo integrada e conecta com Notes & Tasks (06).

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/calendar.rs`; tabela `calendar_events`
  (SQLite). Recorrência via RRULE com crate `rrule`; `.ics` via crate `icalendar`.
- **CalDAV (fase 2):** cliente HTTP (`reqwest`) PROPFIND/REPORT; credenciais no cofre;
  sync como job (infra 01). SSRF guard ao falar com servidores remotos.
- **Quick-parse:** "reunião amanhã 15h" → evento, via `chat_with_routing`.
- **Frontend:** aba `Calendário` (padrão `wave1.js`): visões mês/semana, cores por
  calendário, criar/editar evento, import/export `.ics`.

### Referência no Odysseus (exemplo para consulta)

- `routes/calendar_routes.py` — CRUD de eventos, `.ics`, cores por calendário.
- `src/caldav_sync.py`, `src/caldav_writeback.py` — sync CalDAV (pull/writeback).
- `static/js/calendar.js`, `static/js/calendar/` — UI do calendário.

## 3. Regras de Negócio e Restrições

- **PODE:** CRUD de eventos locais; recorrência (RRULE); lembretes (via Notes/Tasks);
  import/export `.ics`; sync CalDAV opcional.
- **NÃO PODE:** exigir CalDAV — o calendário local funciona 100% sozinho.
- **NÃO PODE:** falar com IP privado/loopback no CalDAV sem o SSRF guard.
- **NÃO PODE:** sobrescrever eventos remotos sem estratégia de merge/conflito clara.
- **Restrição:** expansão de RRULE deve ser limitada (janela de visualização), sem
  explodir memória.

## 4. Critérios de Aceite

- [ ] CRUD de eventos locais; visão mês/semana; cores por calendário.
- [ ] Recorrência (RRULE) expande corretamente na janela visível.
- [ ] Import/export `.ics` round-trip preserva os eventos.
- [ ] Lembrete de evento dispara via Notes & Tasks (06).
- [ ] (Fase 2) Sync CalDAV pull/writeback com uma conta de teste; conflito tratado.
- [ ] Quick-parse cria evento a partir de frase em linguagem natural.

## 5. Plano de Implementação

1. **Tabela `calendar_events`** + CRUD; modelo com RRULE.
2. **Endpoints** locais (CRUD, range query, `.ics` import/export).
3. **UI:** aba `Calendário` (mês/semana, criar/editar, cores).
4. **Lembretes:** integrar com o scheduler (06/01).
5. **Quick-parse:** endpoint que usa LLM para extrair evento de texto.
6. **(Fase 2) CalDAV:** cliente PROPFIND/REPORT, credenciais no cofre, sync job,
   SSRF guard, merge de conflitos.
