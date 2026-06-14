# Auditoria de Implementação das Specs — 2026-06-13

## Escopo e método

Auditoria feita no branch `refactor/frontend-esm-modularization`, commit
`7c27a3b`, cruzando:

- critérios de aceite em `specs/*.md`;
- código atual em `crates/*` e `apps/desktop-ui`;
- histórico Git das ondas já implementadas;
- testes direcionados de Hardware-Fit, Model-Fit e catálogo híbrido.

Esta é uma auditoria de código e testes automatizados. Features visuais e
integrações externas ainda precisam de verificação end-to-end no app Tauri.

## Resultado executivo

| Spec | Estado | Evidência principal |
|---|---|---|
| 01 Jobs/Scheduler/SSE | **Parcial, bloqueadora** | Registry/SSE existem, mas o scheduler executa um job genérico sem ação real, não atualiza `last_run_at`, não desabilita `once`, não grava `job_id` e cria tabelas fora de `MIGRATIONS`. |
| 02 Memória semântica | **Parcial** | Busca híbrida e `ProviderEmbedder` existem; `OnnxEmbedder` é um stub que sempre retorna erro. |
| 03 Busca web | **Parcial** | Backend DuckDuckGo/SearXNG/Brave, cache e SSRF existem; faltam Settings gerais para provider, SearXNG, SafeSearch e limite de resultados. |
| 04 Upload/PDF/Vision | **Ausente** | Não há rotas multipart, armazenamento de uploads, extração ou fluxo multimodal. |
| 05 Documents | **Ausente** | Não há store, rotas, versionamento nem editor. |
| 06 Notes/Tasks | **Ausente** | Só existe o scheduler incompleto da spec 01; não há notas, tarefas ou executor de ações. |
| 07 MCP | **Ausente** | Não há registry/CRUD/runtime MCP. |
| 08 Slash/Skills | **Parcial** | Loader, enable/disable/install de skills existem; faltam CRUD/editor, slash catalog, autocomplete e runner/judge. |
| 09 Speech | **Ausente** | Não há STT/TTS nem integração de microfone. |
| 10 Tema | **Ausente** | Há variáveis CSS, mas não editor, presets, persistência ou análise de contraste. |
| 11 Backup/Vault UI | **Parcial de infraestrutura** | O cofre existe, mas não há export/import/wipe nem UI genérica de status dos segredos. |
| 12 Deep Research | **Parcial e quebrada na UI** | Backend existe, porém `wave5.js` chama várias rotas sem `/api`; sessões são JSON solto, contrariando a persistência unificada. |
| 13 Hardware-Fit | **Concluída** | Rotas, UI, CPU fallback e testes existem; 16 testes direcionados passaram. |
| 14 E-mail | **Ausente** | Não há IMAP/SMTP, cache ou poller. |
| 15 Calendário | **Ausente** | Não há eventos, RRULE, ICS ou CalDAV. |
| 16 Galeria | **Ausente** | A tela IA Visual não implementa galeria, EXIF ou transformações locais. |
| 17 Contatos | **Ausente** | Não há store, rotas ou import/export. |
| 18 Auth/2FA | **Ausente** | Não há auth opt-in, usuários, sessões ou TOTP. |
| 19 Cloud + Local | **Concluída** | Catálogo unificado, cofre, airgap e roteamento existem; testes do catálogo DeepSeek V4 passaram. |
| 20 Monitor de agentes | **Ausente** | Não há registry/snapshot/SSE nem aba Monitor. |
| 21 Modularização frontend | **Parcial** | CSS e parte do JS foram separados, mas `main.js` ainda importa `app.js`; `wave1.js` tem ~46,5 KB e `wave5.js` ~26,1 KB, ambos fora do objetivo final. |

Resumo: **2 concluídas, 7 parciais e 12 ausentes**.

## Ordem recomendada

A ordem abaixo substitui a ordem histórica das ondas porque considera o estado
real do código em 2026-06-13 e evita construir features novas sobre fundações
incompletas.

| Ordem | Spec | Modelo recomendado | Motivo |
|---:|---|---|---|
| 1 | 01 Jobs/Scheduler/SSE | **Top-tier texto suficiente** | Corrige a fundação usada por tarefas, pesquisa, e-mail e monitor. |
| 2 | 21 Modularização frontend | **Top-tier com VL obrigatório** | Evita ampliar os monólitos antes das novas telas; exige comparação visual. |
| 3 | 02 Memória semântica | **Top-tier texto suficiente** | Fecha a implementação real do backend de embeddings. |
| 4 | 03 Busca web | **Top-tier texto suficiente** | Fecha configuração e integração antes de Research. |
| 5 | 12 Deep Research | **Top-tier com VL recomendado** | Corrige integração quebrada e persistência antes de depender dela. |
| 6 | 20 Monitor de agentes | **Top-tier com VL obrigatório** | Depende de Jobs/SSE e tem UI de alta complexidade visual. |
| 7 | 04 Upload/PDF/Vision | **Top-tier com VL obrigatório** | Habilita anexos, documentos e galeria. |
| 8 | 05 Documents | **Top-tier com VL recomendado** | Depende de uploads e exige editor/diff bem verificados. |
| 9 | 06 Notes/Tasks | **Top-tier texto suficiente** | Depende diretamente do scheduler corrigido. |
| 10 | 07 MCP | **Top-tier texto suficiente** | Integração de protocolo e segurança, sem necessidade de visão. |
| 11 | 08 Slash/Skills | **Top-tier texto suficiente** | Reusa Jobs e a infraestrutura de skills já existente. |
| 12 | 17 Contatos | **Top-tier texto suficiente** | Deve preceder E-mail e Calendário para fornecer autocomplete. |
| 13 | 14 E-mail | **Top-tier texto suficiente** | Depende de Jobs e se beneficia de Contatos. |
| 14 | 15 Calendário | **Top-tier com VL recomendado** | Depende de Notes/Tasks e tem visão mês/semana visualmente sensível. |
| 15 | 11 Backup/Vault UI | **Top-tier texto suficiente** | Feita após a maioria dos schemas para reduzir retrabalho no formato de backup. |
| 16 | 10 Tema | **Top-tier com VL obrigatório** | Mudanças globais e contraste exigem inspeção visual de todas as abas. |
| 17 | 09 Speech | **Top-tier texto suficiente** | Não requer visão, mas requer acesso a áudio/microfone ou fixtures reais. |
| 18 | 16 Galeria | **Top-tier com VL obrigatório** | Depende de Uploads e exige validar imagens e transformações. |
| 19 | 18 Auth/2FA | **Top-tier texto, revisão de segurança obrigatória** | É opcional, transversal e de alto risco; deve entrar por último. |

`DeepSeek v4 Pro` é adequado para os itens marcados como “texto suficiente”.
Nas specs 07, 11, 14 e 18, use também uma segunda revisão independente por causa
de segurança, segredos e efeitos externos. Para itens com VL obrigatório, use um
modelo top-tier capaz de inspecionar screenshots, como Opus, GPT ou Mimo com
visão. “VL recomendado” significa que um modelo textual pode implementar, mas
não deve declarar a UI pronta sem uma revisão visual posterior.

## Regras comuns para todos os prompts

Ao enviar qualquer prompt específico abaixo, envie junto este bloco de regras:

1. Ler integralmente `specs/README.md` e a spec indicada antes de editar.
2. Inspecionar o código atual e preservar implementações válidas; não reescrever
   a feature do zero quando o estado for parcial.
3. App nativo Rust + Tauri, offline-first; sem Docker, servidor Python ou CDN.
4. Persistência em `<data_dir>/agent/state.sqlite`, com migrations versionadas em
   `crates/agent-core/src/state_store.rs`; binários podem ficar no data dir, mas
   metadados e estados não podem virar JSON solto.
5. Segredos somente em `crates/daemon/src/secrets_vault.rs`.
6. LLM interno somente via `chat_with_routing(&state, ChatRequest)`.
7. Frontend em ES modules focados, sem globals/handlers inline e sem
   `alert()`, `confirm()` ou `prompt()` nativos; usar modal/toast do app.
8. Adicionar testes de regressão e verificar backend, UI e build Tauri.
9. Não alterar nem reverter mudanças preexistentes fora do escopo.
10. Fazer micro-commits profissionais por unidade lógica e executar push apenas
    depois de todos os testes passarem.

## Prompts na ordem de execução

### 1. Spec 01 — Jobs, Scheduler e SSE

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente.

```text
Trabalhe no repositório G:\TCC e conclua a spec
specs/01-infra-jobs-scheduler-sse.md. Leia também specs/README.md.

Não reimplemente o JobRegistry e o SSE que já funcionam. Audite e corrija o
scheduler atual em crates/daemon/src/jobs.rs. Hoje ele cria um job genérico sem
executar a ação de job_kind/payload, não atualiza last_run_at, não desabilita
uma tarefa once, não grava task_runs.job_id, tem cálculo cron incorreto e cria
scheduled_tasks/task_runs fora do mecanismo MIGRATIONS.

Implemente migrations versionadas, claim transacional de execuções para impedir
duplicidade, dispatcher extensível de ações reais, atualização atômica de
job_id/status/last_run_at, semântica correta para once/interval/cron, recuperação
após restart e cancelamento cooperativo. Preserve os endpoints existentes ou
mantenha compatibilidade.

Crie testes determinísticos com relógio injetável ou tempo controlado cobrindo:
once exatamente uma vez, interval, cron, restart, concorrência de ticks, job_id,
falha, cancelamento e ausência de job órfão. Execute cargo fmt, clippy, testes do
daemon e build. Faça micro-commits separados para migration/store, dispatcher,
correções de scheduling e testes; depois faça push.
```

### 2. Spec 21 — Modularização do frontend

**Modelo:** top-tier com visão obrigatório.

```text
Conclua specs/21-frontend-modularization.md em G:\TCC sem mudança visual ou
funcional. Leia specs/README.md e trate isto como refator puro.

O trabalho atual é parcial: apps/desktop-ui/ui/js/main.js ainda importa app.js;
wave1.js tem cerca de 46,5 KB e wave5.js cerca de 26,1 KB; wave5 ainda é IIFE,
duplica api/daemonUrl/esc e usa handlers inline/globals. Extraia o restante de
app.js, wave1.js e wave5.js para ES modules focados, reutilizando js/core e
js/features. Nenhum módulo deve exceder aproximadamente 25 KB. Remova globals,
onclick inline e duplicação de clientes HTTP/SSE. Preserve ordem de bootstrap,
estado, eventos e cascata CSS.

Antes de editar, capture screenshots e estado DOM de todas as abas e menus em
estados representativos. Depois, compare visualmente antes/depois em viewport
desktop usando capacidade VL e teste o app Tauri real, não apenas HTML isolado.
Verifique chat streaming, modelos, Agent, Console, Histórico, Memória, Comparar,
Pesquisa, Hardware e Config. Exija zero erro novo no console e gere bundle Tauri.

Documente a matriz de paridade e screenshots. Faça micro-commits por módulo,
um commit chore separado para código morto confirmado e push somente após a
paridade visual e funcional.
```

### 3. Spec 02 — Memória semântica

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente.

```text
Conclua specs/02-semantic-memory-embeddings.md em G:\TCC preservando a busca
híbrida e o ProviderEmbedder existentes.

O gap principal está em crates/agent-core/src/embeddings.rs: OnnxEmbedder está
atrás da feature semantic-embeddings, mas é um stub que sempre retorna erro.
Implemente inferência ONNX real com dependências opcionais, tokenizer,
attention mask, pooling e normalização compatíveis com o modelo escolhido.
Carregue o modelo de forma lazy e segura; não baixe artefatos silenciosamente.
Mantenha NullEmbedder e fallback FTS idênticos quando a feature/modelo não
estiver disponível.

Revise persistência, reindex em lote e ProviderEmbedder. Adicione testes de
paráfrase em que FTS falha e semântica encontra o registro, combinação de score,
dimensões inválidas, modelo ausente e fallback. Verifique cargo build/test sem a
feature e com --features semantic-embeddings. Faça micro-commits para runtime
ONNX, integração e testes; depois push.
```

### 4. Spec 03 — Busca web

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente.

```text
Conclua specs/03-web-search-providers.md em G:\TCC sem reescrever o backend de
busca existente.

DuckDuckGo, SearXNG, Brave, cache, fetch e SSRF guard já existem. Falta cumprir a
configuração geral exigida: adicione em Config/Settings seleção do provider,
URL SearXNG, SafeSearch e número máximo de resultados; persista via AppConfig e
guarde BRAVE_API_KEY somente no cofre. Use os endpoints existentes
/api/search/config e /api/search/providers ou corrija-os com compatibilidade.

Garanta que o chip Web do Chat use o provider configurado e que Deep Research
herde o default quando não houver override. Valide URL de SearXNG, limites e
erros degradados. Adicione testes para persistência/reload, chave ausente,
fallback, max_results, bloqueio de IP privado/redirect e integração do chat.
Implemente a UI em módulo ES, com mensagens no padrão do app. Rode fmt, clippy,
testes, smoke Tauri e faça micro-commits + push.
```

### 5. Spec 12 — Deep Research

**Modelo:** top-tier com visão recomendado.

```text
Conclua e repare specs/12-deep-research.md em G:\TCC. Não reescreva os crates de
research/hardware já existentes.

Corrija primeiro a integração quebrada: wave5.js chama /research/start,
/research/cancel, /research/library, /research/spinoff e /research/{id}, enquanto
o daemon expõe /api/research/*. Preserve compatibilidade de resposta retornando
id e job_id se necessário. Extraia Research/Hardware de wave5.js para os ES
modules definidos pela spec 21.

Migre a persistência de crates/research/src/lib.rs, hoje em research/*.json, para
tabelas e stores no state.sqlite via MIGRATIONS. Implemente migração segura dos
JSON legados. Feche os critérios restantes: biblioteca, report sanitizado,
export HTML/print-PDF, hide/reroll persistente, spinoff, cancelamento em até um
round e ausência de jobs órfãos. Reuse a busca da spec 03 e Jobs/SSE da spec 01.

Crie testes de API e um fluxo end-to-end real start -> SSE -> result -> report ->
library -> spinoff -> delete, além de cancelamento e restart. Faça inspeção visual
do relatório e da biblioteca. Micro-commits para rotas, SQLite/migração, UI e
testes; depois push.
```

### 6. Spec 20 — Monitor de orquestração

**Modelo:** top-tier com visão obrigatório.

```text
Implemente specs/monitor_orquestracao_agentes.md em G:\TCC após as specs 01 e 21.
Leia as fontes reais de telemetria antes de desenhar contratos novos.

Crie OrchestrationRegistry/snapshots persistidos no SQLite, agregando EventBus,
ContextBudgetTelemetry, AuditLog, sessões e JobRegistry. Exponha snapshot,
histórico, SSE incremental com sequence id/replay e fallback polling. Reconexão
deve ressincronizar sem duplicar eventos. O monitor fechado não pode alterar o
comportamento ou desempenho do Agent.

Implemente a aba Monitor como ES module: runs ativos/históricos, console central,
ações destacadas, fases/agentes, tokens, ferramentas, duração, progresso e barra
global persistente. Use dados reais; não invente métricas. Cancelar deve usar a
infra de Jobs quando aplicável.

Use um modelo VL para comparar a tela implementada com a composição descrita na
spec em estados vazio, executando, reconectando, concluído e erro. Teste SSE,
replay, polling, restart e cancelamento; capture screenshots. Faça micro-commits
backend/store, SSE, UI e testes; depois push.
```

### 7. Spec 04 — Uploads, PDF e Vision

**Modelo:** top-tier com visão obrigatório.

```text
Implemente specs/04-uploads-pdf-vision.md em G:\TCC sobre Jobs/SSE corrigidos e o
frontend modular.

Crie migrations e store de metadados no state.sqlite. Arquivos binários podem
ficar em diretório gerenciado por hash, com deduplicação SHA-256 e nomes não
controlados pelo usuário. Implemente multipart com limite, sniffing de conteúdo,
rejeição de MIME forjado, path traversal impossível e limpeza consistente.
Extração pesada deve ser job cancelável. PDF/texto deve funcionar offline.

Implemente /api/uploads, metadados/raw/extract/vision e integração drag-and-drop
no Chat. Vision deve usar chat_with_routing e capability real do modelo; quando
não houver multimodal, retornar erro tratável e claro. Não rotule um modelo como
vision apenas pelo nome.

Use VL para testar imagens reais, PDFs, preview, anexos e respostas multimodais.
Cubra 413, MIME forjado, dedup, PDF corrompido, cancelamento, modelo sem visão e
restart. Faça micro-commits storage/API, extração, vision/chat, UI e testes; push.
```

### 8. Spec 05 — Documents

**Modelo:** top-tier com visão recomendado.

```text
Implemente specs/05-documents-editor.md em G:\TCC após Uploads e a modularização.

Crie migrations/stores para documentos, abas e versões, com CRUD, restore e
concorrência previsível. Implemente autosave com debounce e versionamento sem
gerar uma versão por tecla. Vendorize CodeMirror/highlight.js ou alternativa
compatível; nenhum asset pode depender de CDN.

Crie a aba Documentos em ES modules: multi-aba, linguagem, preview Markdown,
export md/html/txt e recuperação de estado. A ação de IA deve enviar apenas o
contexto necessário por chat_with_routing, retornar proposta estruturada e
mostrar diff seguro; aceitar aplica e versiona, rejeitar não altera o documento.
Sanitize preview/export HTML.

Faça testes de CRUD, versão/restore, conflito, autosave, export e sugestão de IA.
Use revisão VL para layout, tabs, editor, preview e diff em resoluções diferentes.
Micro-commits para store/API, editor, IA/diff e testes; depois push.
```

### 9. Spec 06 — Notes & Tasks

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente.

```text
Implemente specs/06-notes-tasks-scheduler.md em G:\TCC reutilizando o scheduler e
dispatcher concluídos na spec 01.

Adicione migrations/stores e APIs para notas, checklist, cor, pin e vencimento,
além de tarefas once/interval/cron com ações tipadas. Não crie um segundo
scheduler. Cada execução deve virar Job e task_run com output/erro. Implemente
run now, pause/resume e histórico.

Ações de Agent/LLM devem passar por policy/approval e chat_with_routing. Toast
deve chegar à UI por evento/SSE. Webhooks precisam de validação de URL, SSRF
guard, timeout e segredo no cofre quando houver autenticação.

Crie UI modular sem diálogos nativos e testes para restart, once único, cron,
falha, cancelamento, run now, toast e webhook bloqueado em destino privado.
Execute build/test e smoke Tauri. Faça micro-commits por store/API, dispatcher,
UI e testes; depois push.
```

### 10. Spec 07 — MCP

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente, com revisão de segurança.

```text
Implemente specs/07-mcp-server-management.md em G:\TCC.

Crie migrations/store e CRUD de servidores MCP. Suporte stdio primeiro e depois
SSE/HTTP conforme a spec, com lifecycle, timeout, backoff e estado degraded sem
derrubar o daemon. Nunca execute comando concatenado em shell; use argumentos
estruturados e limites de processo. Tokens/OAuth ficam no cofre.

Descubra tools e injete-as no ToolRegistry com namespace estável, schemas
validados, enable/disable por tool e aplicação integral de PolicyEngine,
Approval e AuditLog. Trate descrições, resultados e recursos MCP como conteúdo
não confiável e resistente a prompt injection.

Crie um servidor fixture MCP stdio para testes de connect/list/call/reconnect,
tool disable, processo morto, schema inválido e token ausente. Implemente UI
modular para CRUD, status, tools e OAuth. Rode testes e revisão de segurança.
Micro-commits para store/protocolo, ToolRegistry/policy, UI e testes; depois push.
```

### 11. Spec 08 — Slash Commands e Skills

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente.

```text
Conclua specs/08-slash-commands-skills.md em G:\TCC preservando o crate
agent-skills e os endpoints enable/disable/install existentes.

Adicione CRUD seguro de skills, edição de frontmatter+corpo e reload atômico.
Implemente GET /agent/skills/slash-catalog e autocomplete ao digitar "/" no Chat,
com teclado, filtro, descrição e inserção/execução. Não use globals nem handlers
inline. Preserve validação de capabilities, integridade, roots permitidos e
limites de tamanho; impeça path traversal e symlink escape.

Implemente runner de teste como Job com progresso. O juiz IA deve usar
chat_with_routing, produzir veredicto estruturado e nunca conceder capabilities.
Conteúdo de skill é não confiável.

Teste CRUD/reload, frontmatter inválido, slash filtering/keyboard, seleção,
capability bloqueada, cancelamento e judge failure. Faça smoke do Chat no Tauri.
Micro-commits API/editor, slash UX, runner/judge e testes; depois push.
```

### 12. Spec 17 — Contatos

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente.

```text
Implemente specs/17-contacts.md em G:\TCC antes de E-mail e Calendário.

Crie migrations/store e CRUD/search de contatos com normalização de nome,
e-mail e telefone. Implemente import/export vCard e CSV com parser estruturado,
preview de import, deduplicação determinística e merge explícito; não faça split
manual ingênuo de CSV/vCard.

Exponha um contrato reutilizável de autocomplete para as futuras specs 14 e 15.
Crie UI modular para listar, buscar, criar, editar, excluir, importar, revisar
duplicados e exportar. Use modal próprio para confirmação.

Adicione testes de CRUD, busca case/acentos, múltiplos e-mails, CSV quoted,
vCard round-trip, dedup/merge e dados malformados. Verifique build e UI Tauri.
Faça micro-commits store/API, import/export, UI e testes; depois push.
```

### 13. Spec 14 — E-mail

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente, com revisão de segurança.

```text
Implemente specs/14-email.md em G:\TCC sobre Jobs/Scheduler e Contatos.

Crie contas IMAP/SMTP com credenciais exclusivamente no cofre, teste de conexão,
cache SQLite e sincronização incremental por job. Use bibliotecas Rust maduras,
TLS validado, timeouts, limites de mensagem/anexo e estado degraded. Sanitize
HTML de e-mail e trate conteúdo como prompt injection.

Implemente listar/abrir/buscar, compor, responder e rascunhos. Enviar, apagar ou
alterar servidor exige confirmação explícita em modal do app. Triagem por IA
gera somente sugestões e usa chat_with_routing; conteúdo vai a provider remoto
apenas com opt-in claro. Integre autocomplete de Contatos.

Crie fixtures/servidor de teste para IMAP/SMTP ou testes de integração
reproduzíveis. Cubra sync incremental, reconnect, credencial inválida, HTML
hostil, confirmação de envio, cancelamento e falha da IA. Faça micro-commits para
contas/cofre, sync/cache, compose/triagem, UI e testes; depois push.
```

### 14. Spec 15 — Calendário

**Modelo:** top-tier com visão recomendado.

```text
Implemente specs/15-calendar.md em G:\TCC após Notes/Tasks e Contatos.

Crie migrations/store para calendários, eventos, recorrências, exceções,
participantes e lembretes. Use biblioteca RRULE/ICS adequada e datas com timezone
explícito; não implemente recorrência por manipulação de strings. Import/export
.ics deve preservar UID, timezone e recorrência.

Implemente visão mês/semana responsiva, CRUD, cores, quick parse via
chat_with_routing e autocomplete de Contatos. Lembretes devem reutilizar a spec
06. Para CalDAV, use cofre, SSRF guard, sync token/ETag e política de conflito
visível; falha remota não pode afetar eventos locais.

Teste DST/timezone, RRULE, exceções, ICS round-trip, lembrete, conflito CalDAV e
quick parse. Use VL para revisar mês/semana, sobreposição, overflow e modal.
Micro-commits store/recorrência, ICS/CalDAV, UI e testes; depois push.
```

### 15. Spec 11 — Backup, Restore e Vault UI

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente, com revisão de segurança.

```text
Conclua specs/11-backup-restore-vault-ui.md em G:\TCC depois que os schemas das
specs anteriores estiverem estáveis.

Implemente formato de backup versionado, manifesto, export consistente do SQLite
e inclusão segura dos arquivos gerenciados referenciados por uploads/documentos,
quando aplicável. Import deve validar versão/schema/tamanho, operar em transação,
deduplicar por IDs/hashes e fazer rollback integral em erro. Nunca exporte
segredos em texto puro; documente claramente se o cofre é excluído ou exportado
apenas como blob criptografado não portável.

Implemente wipe seletivo por categoria com preview de contagem e modal digitável,
sem confirm() nativo. A UI do cofre lista somente nome, origem e status; nunca
valor ou fragmento. Registre auditoria de export/import/wipe sem dados sensíveis.

Teste round-trip, versão incompatível, arquivo adulterado, rollback, dedup,
referências entre tabelas e ausência de segredo no export. Faça revisão de
segurança, micro-commits formato/export, import/wipe, Vault UI e testes; push.
```

### 16. Spec 10 — Editor de tema

**Modelo:** top-tier com visão obrigatório.

```text
Implemente specs/10-theme-editor.md em G:\TCC sobre o frontend modular.

Centralize tokens visuais em variáveis CSS sem quebrar o tema atual. Crie
persistência de preferências e presets customizados, com migration/store ou
mecanismo de settings aprovado pelo projeto. A aba Aparência deve editar cores,
fonte, densidade e efeitos ao vivo, salvar, resetar e importar/aplicar presets.
Assets e fontes devem funcionar offline.

Implemente cálculo de contraste WCAG para combinações relevantes e aviso sem
bloquear o usuário. Garanta foco, hover, disabled, charts, modais e syntax
highlight em todos os presets.

Use VL obrigatoriamente: capture e compare todas as abas no tema padrão antes e
depois, além de presets claro/escuro, densidades e resoluções. Teste boot sem
flash incorreto, reset, preset inválido e persistência. Micro-commits tokens,
store/UI, contraste e testes visuais; depois push.
```

### 17. Spec 09 — Speech

**Modelo:** top-tier textual; `DeepSeek v4 Pro` é suficiente. Visão não ajuda; acesso a áudio ajuda.

```text
Implemente specs/09-speech-stt-tts.md em G:\TCC com a feature speech desligada
por padrão.

Implemente STT com whisper-rs atrás de feature flag, modelo configurável e sem
download silencioso. Capture microfone pelo caminho nativo/Tauri com permissões,
limites, cancelamento e formato PCM/WAV bem definido. Implemente TTS usando voz
do SO e fallback Web Speech quando disponível, com stop/pause e seleção de voz.

Crie endpoints transcribe/synthesize e integração no Chat: gravar preenche o
input; ouvir reproduz resposta do assistente. Sem feature/modelo/dispositivo,
degrade com mensagem clara e mantenha o app funcional.

Use fixtures WAV reais e mocks de dispositivo para CI. Teste build com e sem a
feature, transcrição, síntese, cancelamento, permissão negada e dispositivo
ausente. Faça teste manual com áudio audível em Windows. Micro-commits backend,
captura/Tauri, UI e testes; depois push.
```

### 18. Spec 16 — Galeria e editor de imagem

**Modelo:** top-tier com visão obrigatório.

```text
Implemente specs/16-gallery-image-editor.md em G:\TCC reutilizando integralmente
o storage e metadados da spec 04.

Crie índice de imagens, thumbnails em jobs, EXIF básico e transformações locais
não destrutivas ou versionadas: rotate, crop e resize via crates image/imageproc.
Preserve original, orientação, perfil de cor quando possível e metadados de
derivação. Limite dimensões/memória para evitar image bombs.

Implemente a aba Galeria em ES modules com grid virtualizado/paginado, preview,
download, seleção e editor. Geração/inpaint só pode usar servidor externo
opcional configurado no cofre; ausência deve desabilitar a ação com aviso, sem
erro ou instalação automática de modelos.

Use VL para validar thumbnails, orientação EXIF, crop, resize, estados vazio/erro
e imagens reais variadas. Teste formatos, arquivo corrompido, image bomb,
cancelamento e restart. Micro-commits índice/thumbs, transforms, UI e integração
externa/testes; depois push.
```

### 19. Spec 18 — Auth, Multiusuário e 2FA

**Modelo:** top-tier textual com raciocínio forte; revisão de segurança independente obrigatória.

```text
Implemente specs/18-auth-multiuser-2fa.md em G:\TCC por último e mantenha
APP_AUTH_ENABLED=false como default absoluto.

Antes de codar, produza threat model curto. Com auth desligada, rotas e UX devem
permanecer idênticas. Com auth ligada, implemente bootstrap admin explícito,
hash Argon2id, sessões revogáveis, logout, rate limit, TOTP com recovery codes,
tokens de API armazenados apenas como hash e RBAC para rotas admin. Segredos de
webhook ficam no cofre. Evite enumeração de usuário, session fixation, CSRF e
logs sensíveis. Bypass loopback somente em build/dev explicitamente marcado.

Crie middleware central e testes de matriz de rotas; não espalhe checks ad hoc.
Cookies devem ser HttpOnly/SameSite e Secure quando HTTPS. A UI deve cobrir
setup, login, 2FA, recovery, sessões e tokens sem diálogos browser.

Teste auth off, bootstrap, login, lockout, TOTP/replay, revogação, RBAC, CSRF,
token API e webhook. Faça revisão independente antes do push. Micro-commits
schema/core, middleware, 2FA/tokens, UI, testes e documentação do TCC; depois push.
```

## Specs fora da fila

- **13 — Hardware-Fit:** manter e somente corrigir regressões descobertas.
- **19 — Cloud + Local:** manter; o catálogo atual já contém DeepSeek V4 Pro e
  V4 Flash, e o seletor unificado foi corrigido no branch atual.

## Observação sobre o workspace atual

Há uma alteração preexistente e não relacionada em
`apps/desktop-ui/src-tauri/Cargo.lock`. Nenhum executor deve revertê-la,
sobrescrevê-la ou incluí-la automaticamente em commits sem entender sua origem.
