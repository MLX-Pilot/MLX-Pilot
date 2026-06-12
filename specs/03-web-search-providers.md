# 03 — Busca Web (Provedores + Extração de Conteúdo)

> **Tipo:** completar/expandir (hoje só Brave) · **Esforço:** L
> **Depende de:** — · **Habilita:** chip "Web" do Chat, Agent web tool, Deep Research (12).

## 1. Objetivo

Transformar o suporte de busca web (hoje limitado a Brave em
`/web/brave/search`) numa camada com **múltiplos provedores** (SearXNG,
DuckDuckGo gratuito, Brave) e **extração de conteúdo** das páginas, com cache e
proteção contra SSRF. Resolve a dependência de uma única API paga e fornece a
base de coleta de fontes que o Deep Research consome.

## 2. Contexto Técnico

- **Linguagem:** Rust; novo crate `crates/search` (ou módulo `crates/daemon/src/search.rs`).
- **HTTP:** `reqwest` (já presente, rustls).
- **Parsing/Extração:** `scraper` (HTML → texto/main-content) e `regex`/`url`
  para normalização e deduplicação.
- **Provedores (trait `SearchProvider`):**
  - `DuckDuckGoProvider` (HTML/lite, gratuito, fallback padrão);
  - `SearxngProvider` (GET `/search?format=json` a uma instância configurável);
  - `BraveProvider` (reaproveita a chave já existente).
- **Cache:** arquivo/SQLite com TTL (ex.: 15 min) por query+provider.
- **Config:** estender `AppConfig` (provider padrão, instância SearXNG, SafeSearch,
  nº de resultados, filtros de domínio); segredos no cofre.
- **Segurança:** allowlist de esquema (http/https), bloqueio de IPs privados/loopback
  (reaproveitar lógica de `url_security`/`url_safety` se existir no agente).

### Referência no Odysseus (exemplo para consulta)

- `routes/search_routes.py`, `services/search/`, `src/search/` — orquestração de
  busca e provedores (SearXNG/DuckDuckGo).
- `src/url_safety.py`, `src/url_security.py` — hardening de URL/SSRF (modelo direto
  para a nossa allowlist/bloqueio de IP privado).
- No `.env.example`/README do Odysseus: variáveis `SEARXNG_INSTANCE`,
  `SEARXNG_SECRET` (modelo de configuração de provider).

## 3. Regras de Negócio e Restrições

- **PODE:** alternar provider por configuração e por requisição; combinar busca +
  fetch + extração de texto principal de cada resultado.
- **PODE:** servir resultados do cache dentro do TTL.
- **NÃO PODE:** seguir redirecionamentos para IPs privados/loopback/meta-data
  (169.254.169.254) — bloquear SSRF.
- **NÃO PODE:** baixar binários/arquivos grandes — só conteúdo textual, com limite
  de tamanho (ex.: 2 MB por página) e timeout (ex.: 8 s).
- **NÃO PODE:** quebrar o chip "Web" do Chat já existente — manter a rota Brave
  funcionando como um provider.
- **Restrição offline:** sem rede, falhar graciosamente (mensagem clara), sem travar.

## 4. Critérios de Aceite

- [ ] Trait `SearchProvider` com DuckDuckGo (default), SearXNG e Brave.
- [ ] `POST /api/search` retorna `[{url,title,snippet}]`; `POST /api/search/fetch`
      retorna texto extraído de uma URL.
- [ ] Config em Settings: escolher provider, instância SearXNG, SafeSearch, nº de
      resultados; segredos no cofre.
- [ ] Cache com TTL evita refazer a mesma query em janela curta.
- [ ] Tentativa de fetch a `http://127.0.0.1`/IP privado é **bloqueada**.
- [ ] Chip "Web" do Chat continua funcionando (via provider configurado).

## 5. Plano de Implementação

1. **Trait + tipos:** `SearchProvider`, `SearchResult`, `SearchQuery`.
2. **DuckDuckGoProvider:** request + parse de resultados (HTML lite).
3. **SearxngProvider** e adaptar **BraveProvider** ao trait.
4. **Fetch/extract:** baixar página (limite/timeout) + extrair main content com `scraper`.
5. **SSRF guard:** resolver host, recusar privados/loopback/link-local.
6. **Cache:** camada com TTL (memória + SQLite opcional).
7. **Config + segredos:** estender `AppConfig`; UI em Settings.
8. **Endpoints:** `/api/search`, `/api/search/fetch`, `/api/search/providers`,
   `/api/search/config`; migrar o chip "Web" para usar a nova camada.
