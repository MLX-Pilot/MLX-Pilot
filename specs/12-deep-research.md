# 12 — Deep Research (Pesquisa Iterativa + Relatório Visual)

> **Tipo:** nova feature (vitrine) · **Esforço:** XL · **Depende de:** Infra Jobs/SSE
> (01), Busca Web (03) · **Opcional:** Embeddings (02) p/ dedup/recall.

## 1. Objetivo

Executar **pesquisas multi-etapa** dirigidas pelo LLM (planejar → buscar →
ler/extrair → sintetizar → decidir parar), produzindo um **relatório visual em
HTML** (hero, sumário/TOC, fontes colapsáveis, export PDF/HTML) e permitindo
"spin-off" para uma conversa de chat. É a feature mais impressionante para o TCC:
demonstra um agente autônomo de pesquisa rodando localmente.

## 2. Contexto Técnico

- **Backend:** novo crate `crates/research` + módulo `crates/daemon/src/research_routes.rs`.
- **Loop (IterResearch):** orquestração em Rust; LLM via `chat_with_routing`;
  busca/fetch via camada da **spec 03**.
- **Execução:** **job** assíncrono (infra 01) com progresso por round via **SSE**;
  cancelável.
- **Persistência:** resultados em `<data_dir>/research/{id}.json` (query, rounds,
  fontes, achados, stats, categoria, hidden_images, owner) — biblioteca lê via glob.
- **Relatório:** Markdown → HTML com `comrak`, **sanitizado com `ammonia`**, CSS
  embarcado (sem fontes remotas), TOC gerado, imagens com hide/reroll.
- **Extração:** `scraper`/`select`; `uuid`; `tokio`; `serde_json`; `regex`.
- **Frontend:** aba `Pesquisa` (padrão `wave1.js`): formulário (query, modelo,
  nº de rounds, provider de busca, categoria), cards de job com barra de progresso,
  biblioteca, visualizador do relatório (iframe/modal) com export e "spin-off".

### Referência no Odysseus (exemplo para consulta)

- `src/deep_research.py`, `src/research_handler.py` — loop iterativo e ciclo de vida.
- `src/visual_report.py` — geração do relatório HTML (hero, TOC, galeria, reroll).
- `routes/research_routes.py`, `services/research/` — endpoints e serviços.
- `static/js/research/`, `static/js/researchSynapse.js` — UI de jobs, biblioteca,
  visualização e spin-off.

## 3. Regras de Negócio e Restrições

- **PODE:** rodar N rounds (limite configurável); gerar queries por round com
  deduplicação; extrair e sintetizar; decidir parar; persistir e reabrir.
- **PODE:** transformar um relatório em nova sessão de chat (spin-off com contexto).
- **NÃO PODE:** loop infinito — teto rígido de rounds/tempo e parada por decisão do LLM.
- **NÃO PODE:** violar SSRF/limites da camada de busca (spec 03).
- **NÃO PODE:** injetar HTML não sanitizado no relatório (sempre via `ammonia`).
- **NÃO PODE:** bloquear a request — execução é sempre via job assíncrono + SSE.
- **Restrição:** funcionar com modelos locais modestos — prompts enxutos, tolerância
  a respostas curtas; degradar se a busca estiver offline.

## 4. Critérios de Aceite

- [ ] `POST /api/research/start` cria job e retorna `{id}`; `…/stream/{id}` emite
      progresso por fase/round; `…/cancel/{id}` interrompe.
- [ ] `GET /api/research/result/{id}` retorna markdown + fontes + achados;
      `…/report/{id}` serve HTML sanitizado.
- [ ] Biblioteca lista pesquisas concluídas; abrir reexibe o relatório.
- [ ] Export PDF (print) e HTML; hide/reroll de imagem persiste.
- [ ] `POST /api/research/spinoff/{id}` cria sessão de chat semeada com o relatório.
- [ ] Cancelar no meio interrompe em ≤1 round; sem jobs órfãos.

## 5. Plano de Implementação

1. **Crate `research`:** tipos (`ResearchSession`, `Round`, `Source`, `Finding`).
2. **Loop:** plan → gen-queries → search (spec 03) → fetch/extract → synthesize →
   decide-stop, com callbacks de progresso.
3. **Integração com jobs (01):** rodar o loop como job; emitir progresso; cancelamento.
4. **Persistência FS** dos resultados + biblioteca (glob).
5. **Relatório:** `comrak` + `ammonia` + CSS embarcado + TOC + imagens.
6. **Endpoints** (start/status/stream/cancel/result/report/library/spinoff).
7. **UI:** aba `Pesquisa` (form, cards de job, biblioteca, visualizador, export, spin-off).
8. **Robustez:** limites de round/tempo, prompts enxutos, degradação offline.
