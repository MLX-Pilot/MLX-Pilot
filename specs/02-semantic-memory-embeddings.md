# 02 — Memória Semântica (Embeddings + Recuperação Híbrida)

> **Tipo:** completar feature existente (Memória) · **Esforço:** M
> **Depende de:** Memória keyword/FTS (já entregue) · **Habilita:** melhor recall
> em Memória, Deep Research (12), Sessions search.

## 1. Objetivo

Elevar a Memória de busca puramente lexical (FTS5) para **recuperação híbrida
semântica + keyword**, de modo que o agente recupere conhecimento por significado
(ex.: "linguagem favorita do usuário" encontra "ele prefere Rust") e não só por
correspondência de palavras. Resolve o limite do FTS para sinônimos/paráfrases,
fortalecendo o argumento de TCC de um "LLM local que evolui e lembra".

## 2. Contexto Técnico

- **Linguagem:** Rust (`crates/agent-core`).
- **Embeddings (duas vias, com fallback):**
  1. **Provider local primeiro** — se o provider ativo (Ollama/llama.cpp) expõe
     endpoint de embeddings, gerar vetores via ele (reutilizando a infra de
     provider; sem novas deps). É a via mais "nativa".
  2. **ONNX embarcado (fallback opcional)** — crate `ort` (ONNX Runtime) +
     modelo `bge-small-en`/`multilingual-e5-small` (~30–50 MB), atrás de uma
     *Cargo feature* (`semantic-embeddings`) para o binário poder ser distribuído
     sem o modelo.
- **Armazenamento:** colunas **já criadas** `embedding BLOB` e `embedding_dim`
  em `memory_records` (vetor `f32` serializado como bytes). Similaridade do
  cosseno calculada em Rust.
- **Recuperação híbrida:** combinar score FTS (bm25 normalizado) com score
  vetorial: `score = 0.6 * cos + 0.4 * fts_norm` (pesos configuráveis).
- **Local:** estender `crates/agent-core/src/memory.rs` (`MemoryStore`) e adicionar
  `embeddings.rs` (trait `Embedder` + impls `ProviderEmbedder`, `OnnxEmbedder`,
  `NullEmbedder`).

### Referência no Odysseus (exemplo para consulta)

- `src/memory_vector.py`, `src/rag_vector.py` — vetorização e busca vetorial.
- `src/memory_provider.py`, `src/embeddings.py`, `src/embedding_lanes.py` —
  geração de embeddings e "lanes" de embedding.
- `src/chroma_client.py` — integração ChromaDB (no MLX-Pilot **não** usar ChromaDB;
  substituímos por BLOB no SQLite + cosseno em Rust).
- `mcp_servers/memory_server.py`, `routes/memory_routes.py`, `static/js/memory.js`
  — superfícies de memória já espelhadas na nossa aba Memória.

## 3. Regras de Negócio e Restrições

- **PODE:** gerar embedding na criação/edição de um registro e ao importar.
- **PODE:** rodar busca híbrida quando há embeddings; cair para FTS puro quando
  não há (degradação graciosa).
- **NÃO PODE:** bloquear a criação de memória se o embedder falhar — salvar o
  registro mesmo sem vetor (gera depois, em lote).
- **NÃO PODE:** baixar modelo ONNX automaticamente sem a feature habilitada nem
  sem aviso; o app precisa funcionar sem o modelo (fallback FTS).
- **NÃO PODE:** introduzir dependência de servidor externo (ChromaDB etc.).
- **Privacidade:** embeddings ficam locais; nunca enviados a serviços remotos
  salvo se o usuário escolher explicitamente um provider remoto de embeddings.

## 4. Critérios de Aceite

- [ ] Trait `Embedder` com pelo menos `ProviderEmbedder` (via Ollama/llama.cpp) e
      `NullEmbedder` (sem-op) funcionando; `OnnxEmbedder` atrás de feature flag.
- [ ] Novos registros recebem `embedding`/`embedding_dim` quando há embedder ativo.
- [ ] Endpoint `POST /agent/memory/reindex` recomputa embeddings em lote.
- [ ] Busca retorna resultado semântico correto num caso onde o FTS falha
      (consulta paráfrase) e mantém os resultados FTS quando aplicável.
- [ ] Sem embedder/feature, o app compila e a busca FTS continua idêntica.
- [ ] `cargo build` verde com e sem `--features semantic-embeddings`.

## 5. Plano de Implementação

1. **Trait `Embedder`** (`embeddings.rs`): `async fn embed(&self, texts: &[String]) -> Vec<Vec<f32>>`.
2. **`ProviderEmbedder`**: detectar/usar endpoint de embeddings do provider local;
   se indisponível, retornar erro tratável (cai para Null).
3. **`OnnxEmbedder`** (feature `semantic-embeddings`): carregar modelo via `ort`,
   tokenizar, gerar vetores; documentar download manual do modelo.
4. **Persistência:** helpers em `state_store.rs` para salvar/ler `embedding` (BLOB
   ↔ `Vec<f32>`); util de cosseno.
5. **Recuperação híbrida** em `MemoryStore::search`: rodar FTS + vetorial, normalizar
   e combinar scores; ordenar/deduplicar.
6. **Hooks de escrita:** gerar embedding em `add_memory`/`update_memory`; tolerar
   falha (vetor vazio → backfill posterior).
7. **Reindex:** endpoint + job (usa infra 01) para vetorizar registros sem vetor.
8. **UI:** indicador "semântico ativo/inativo" na aba Memória; botão "Reindexar".
