# Ondas de Implementação — MLX-Pilot

Divisão das specs (`specs/*.md`) em ondas dependência-aware, com um **prompt
pronto** por onda para guiar a implementação (humano ou IA).

> **Já entregue** (não reimplementar; não quebrar): Presets, Memória (FTS),
> Histórico/Sessões, Compare, e o mecanismo de migrações SQLite versionadas.

## Mapa das ondas

| Onda | Tema | Specs | Pré-requisitos |
|---|---|---|---|
| **1** | Fundação & App Híbrido | `01` jobs/sse · `02` embeddings · `03` web search · `19` cloud+local | — |
| **2** | Observabilidade & Conteúdo | `20` monitor · `04` uploads/pdf · `05` documents | Onda 1 (SSE) |
| **3** | Produtividade do Agente | `06` notes/tasks · `07` mcp · `08` slash/skills | Onda 1 (jobs) |
| **4** | Experiência & Operação | `09` speech · `10` theme · `11` backup/vault | — |
| **5** | Pesquisa & Hardware | `12` deep research · `13` cookbook/hwfit | Ondas 1+3 (jobs/sse/web) |
| **6** | Comunicação & Pessoas | `14` email · `15` calendar · `17` contacts | Onda 1 (jobs) |
| **7** | Mídia & Segurança (opcional) | `16` gallery · `18` auth/2fa | Onda 2 (uploads) |

## Regras invioláveis (valem para TODAS as ondas — ver `specs/README.md`)

- App **nativo** (Rust + Tauri). Proibido Docker/servidor Python/só-navegador.
- Persistência no **SQLite compartilhado** (`<data>/agent/state.sqlite`) via
  migrações versionadas (`MIGRATIONS` em `crates/agent-core/src/state_store.rs`).
- Segredos **só no cofre** (`crates/daemon/src/secrets_vault.rs`).
- Chamadas LLM internas via **`chat_with_routing(&state, ChatRequest)`**.
- Endpoint Axum: `State(AppState)` + `Json<Req>` → `Result<Json<T>, ApiError>`;
  registrar antes de `.with_state(state)` em `crates/daemon/src/lib.rs`.
- UI: aba `.tab[data-panel="x"]` + painel `#panel-x` + módulo JS autocontido
  (modelo `apps/desktop-ui/ui/wave1.js`), CSS no tema, `esc()` em tudo dinâmico.
- Odysseus (clonável de github.com/pewdiepie-archdaemon/odysseus) é **referência
  conceitual** — reimplementar nativo em Rust, **não** portar Python.

---

## Prompt — Onda 1 (Fundação & App Híbrido)

```
Você vai implementar features no MLX-Pilot: app desktop NATIVO (Rust workspace +
Tauri). Backend: daemon Axum em crates/daemon/src/lib.rs (AppState com serviços
Arc<>); dados em crates/agent-core (SQLite compartilhado state.sqlite + migrações
versionadas em state_store.rs); UI estática em apps/desktop-ui/ui (index.html +
módulos JS; troca de abas por switchTab; cliente api(path)). Já existem e NÃO
podem quebrar: Presets, Memória (FTS), Histórico, Compare.

Implemente, nesta ordem, lendo cada spec inteira antes:
1. specs/01-infra-jobs-scheduler-sse.md   (registro de jobs + scheduler + SSE/cancel)
2. specs/03-web-search-providers.md        (SearXNG/DuckDuckGo/Brave + fetch/extração + SSRF guard)
3. specs/02-semantic-memory-embeddings.md  (embeddings + recuperação híbrida na Memória)
4. specs/19-hybrid-cloud-local-models.md   (seletor unificado local+cloud; chave no cofre; foco no Agent)

Regras invioláveis: app nativo (sem Docker/Python/só-navegador); persistência no
SQLite via migrações versionadas; segredos só no secrets_vault; LLM interno via
chat_with_routing; padrão de endpoint Axum (State+Json->Result<Json,ApiError>,
registrar antes de .with_state); padrão de UI (aba+painel+módulo JS estilo
wave1.js, esc() em tudo). Odysseus é só referência conceitual (reimplemente em Rust).

Processo: backend primeiro, depois UI; cargo check/build verde a cada etapa;
verifique subindo o daemon e exercitando os endpoints (curl/PowerShell) + smoke
test da UI; micro-commits por feature (feat/fix/docs, com Co-Authored-By) num
branch feat/wave-1-foundation. Conclua só quando os "Critérios de Aceite" de cada
spec passarem. Entregue código + verificação + pendências.
```

---

## Prompt — Onda 2 (Observabilidade & Conteúdo)

```
Contexto: MLX-Pilot nativo (Rust + Tauri), conforme regras de specs/README.md.
A Onda 1 já entregou a infra de jobs/scheduler/SSE — REUSE essa convenção de
streaming. Não quebre features existentes (Presets/Memória/Histórico/Compare +
Onda 1).

Implemente, nesta ordem:
1. specs/20-... (monitor_orquestracao_agentes.md) — tela de monitoramento do Agent:
   console de raciocínio em streaming, rodapé global (tempo/tokens/tarefas ativas)
   e sidebar de fases/agentes. Agregar telemetria existente (EventBus,
   ContextBudgetTelemetry/budget_tracker, AuditLog, sessões) via SSE.
2. specs/04-uploads-pdf-vision.md — uploads com dedup por SHA-256, extração de PDF
   e visão (imagem -> modelo multimodal); habilita import de Memória e Documents.
3. specs/05-documents-editor.md — editor multi-aba (CodeMirror + highlight.js
   vendorizados, offline), versionamento e edições por IA com aceite via diff.

Regras invioláveis: app nativo; SQLite + migrações; segredos no cofre; LLM via
chat_with_routing; padrão Axum e de UI (estilo wave1.js, esc()); Odysseus só como
referência (reimplementar em Rust). Use a infra de jobs (01) para OCR/trabalho
assíncrono.

Processo: backend->UI; build verde por etapa; verificar endpoints + smoke UI;
micro-commits num branch feat/wave-2-observability-content; só concluir com os
Critérios de Aceite de cada spec atendidos. Entregue código + verificação + pendências.
```

---

## Prompt — Onda 3 (Produtividade do Agente)

```
Contexto: MLX-Pilot nativo (Rust + Tauri), regras de specs/README.md. A Onda 1 já
entregou jobs/scheduler/SSE (o scheduler de tarefas DEPENDE disso). Não quebre o
que já existe.

Implemente, nesta ordem:
1. specs/06-notes-tasks-scheduler.md — notas (lembrete/checklist), to-dos e tarefas
   agendadas (once/interval/cron) que o agente executa; notificações (toast/webhook).
   O scheduler é a infra da Onda 1; ações respeitam PolicyEngine/Approval do agente.
2. specs/07-mcp-server-management.md — registrar/conectar servidores MCP (stdio/SSE/
   HTTP), injetar tools no ToolRegistry com policy aplicada, OAuth com token no cofre.
3. specs/08-slash-commands-skills.md — CRUD/edição/teste de skills (SKILL.md),
   slash-catalog e autocomplete de "/" no chat; teste/auditoria como job (juiz IA).

Regras invioláveis: app nativo; SQLite + migrações; segredos no cofre; LLM via
chat_with_routing; padrão Axum e de UI (wave1.js, esc()); manter capabilities/
integridade de skills e policy/approval do agente; tratar conteúdo de skill/MCP
como não confiável (prompt-injection). Odysseus só referência (reimplementar em Rust).

Processo: backend->UI; build verde por etapa; verificação real (criar tarefa que
dispara, conectar um MCP de exemplo, invocar slash); micro-commits num branch
feat/wave-3-agent-productivity; concluir só com Critérios de Aceite atendidos.
Entregue código + verificação + pendências.
```

---

## Prompt — Onda 4 (Experiência & Operação)

```
Contexto: MLX-Pilot nativo (Rust + Tauri), regras de specs/README.md. Features
independentes de UX e operação; não quebre o existente.

Implemente:
1. specs/09-speech-stt-tts.md — TTS (crate tts, vozes do SO; fallback Web Speech) e
   STT (whisper-rs atrás de feature flag; captura de microfone via Tauri). App deve
   funcionar sem o modelo Whisper (degrada com aviso).
2. specs/10-theme-editor.md — aba Aparência: editar variáveis CSS do tema ao vivo,
   presets, fontes/densidade/efeitos; persistir prefs; offline (assets vendorizados).
3. specs/11-backup-restore-vault-ui.md — export/import de todos os dados do SQLite
   (versionado, dedup no import), wipe seletivo por categoria, UI do cofre (status
   dos segredos, nunca o valor).

Regras invioláveis: app nativo; SQLite + migrações; segredos só no cofre (export
NÃO inclui segredos em texto puro); LLM via chat_with_routing quando aplicável;
padrão Axum e de UI (wave1.js, esc()); features atrás de flag não podem quebrar o
build. Odysseus só referência (reimplementar em Rust).

Processo: build verde com e sem feature flags; verificar TTS/STT, troca de tema ao
vivo e round-trip export->wipe->import; micro-commits num branch
feat/wave-4-experience-ops; concluir só com Critérios de Aceite. Entregue código +
verificação + pendências.
```

---

## Prompt — Onda 5 (Pesquisa & Hardware)

```
Contexto: MLX-Pilot nativo (Rust + Tauri), regras de specs/README.md. As Ondas 1 e
3 já entregaram jobs/scheduler/SSE e busca web — Deep Research DEPENDE disso.
Features XL: capriche na robustez. Não quebre o existente.

Implemente, nesta ordem:
1. specs/12-deep-research.md — loop iterativo (plan->search->extract->synthesize->
   stop) como JOB assíncrono (infra 01) com progresso por round via SSE; busca via
   camada da Onda 1/03; relatório HTML sanitizado (comrak + ammonia, CSS embarcado);
   biblioteca, export, spin-off para chat. Teto rígido de rounds/tempo.
2. specs/13-cookbook-hardware-fit.md — detectar hardware (sysinfo + nvidia-smi/
   rocm-smi, fallback CPU), pontuar modelos por fit, perfis de serve (llama.cpp),
   baixar via catálogo existente. NÃO instalar runtime de GPU nem editar config do SO.

Regras invioláveis: app nativo; SQLite + migrações (cache/histórico); segredos no
cofre; LLM via chat_with_routing; SSRF guard na busca/fetch; HTML sempre sanitizado;
padrão Axum e de UI (wave1.js, esc()). Odysseus só referência (reimplementar em Rust).

Processo: backend->UI; build verde por etapa; verificar um run de pesquisa real
(progresso/cancelamento/relatório) e o scan de hardware (incl. cenário CPU-only);
micro-commits num branch feat/wave-5-research-hardware; concluir só com Critérios de
Aceite. Entregue código + verificação + pendências.
```

---

## Prompt — Onda 6 (Comunicação & Pessoas)

```
Contexto: MLX-Pilot nativo (Rust + Tauri), regras de specs/README.md. A Onda 1 já
entregou jobs/scheduler (poller de e-mail e lembretes DEPENDEM disso). Cluster PIM,
pesado e com integração de rede — capriche na resiliência. Não quebre o existente.

Implemente, nesta ordem:
1. specs/14-email.md — contas IMAP/SMTP (credenciais SÓ no cofre), leitura/busca/
   compor/responder, poller (job) com triagem por IA (resumo/tags/urgência/rascunho/
   spam) como SUGESTÕES. Enviar/apagar SÓ com confirmação explícita do usuário.
2. specs/15-calendar.md — calendário local-first (eventos + RRULE + .ics), lembretes
   via Notes&Tasks; CalDAV opcional (fase 2) com SSRF guard e merge de conflito.
3. specs/17-contacts.md — agenda local (vCard/CSV, dedup), busca reutilizável por
   E-mail/Calendário.

Regras invioláveis: app nativo; SQLite + migrações; segredos só no cofre; LLM via
chat_with_routing (conteúdo só ao LLM local por padrão; remoto = opt-in); IA nunca
envia/apaga sem confirmação; SSRF guard no CalDAV; padrão Axum e de UI (wave1.js,
esc()). Odysseus só referência (reimplementar em Rust).

Processo: backend->UI; build verde por etapa; verificar com conta de teste
(conexão/listagem/compor; evento local + .ics; import/export de contatos);
micro-commits num branch feat/wave-6-comms-people; concluir só com Critérios de
Aceite. Entregue código + verificação + pendências.
```

---

## Prompt — Onda 7 (Mídia & Segurança — opcional)

```
Contexto: MLX-Pilot nativo (Rust + Tauri), regras de specs/README.md. A Onda 2 já
entregou uploads (a galeria reaproveita isso). Features opcionais/avaliáveis no
escopo de TCC. Não quebre o existente.

Implemente:
1. specs/16-gallery-image-editor.md — galeria (uploads + geradas) com EXIF/thumb e
   transformações locais (rotate/crop/resize via image/imageproc). Geração/inpaint
   só via servidor de difusão EXTERNO opcional — NÃO embutir modelos de difusão.
2. specs/18-auth-multiuser-2fa.md — auth OPT-IN (desligada por padrão; desktop
   single-user em 127.0.0.1 continua sem login). Usuários/privilégios/tokens/
   webhooks/2FA TOTP; segredos no cofre; gate de rotas admin. Documentar no TCC a
   decisão de manter single-user por padrão.

Regras invioláveis: app nativo; SQLite + migrações; segredos só no cofre; auth NÃO
pode ser obrigatória nem ligada por padrão; galeria/edição local funciona sem
servidor de geração; padrão Axum e de UI (wave1.js, esc()). Odysseus só referência
(reimplementar em Rust).

Processo: build verde (incl. com auth desligada = comportamento atual idêntico);
verificar galeria + transformações e o fluxo de auth opt-in; micro-commits num
branch feat/wave-7-media-security; concluir só com Critérios de Aceite. Entregue
código + verificação + pendências.
```
