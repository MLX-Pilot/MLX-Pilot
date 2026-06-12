# 05 — Documents (Editor Multi-aba com Assistência de IA)

> **Tipo:** nova feature · **Esforço:** L · **Depende de:** — (opcional: Uploads 04)
> **Habilita:** redação assistida, notas longas, artefatos do TCC.

## 1. Objetivo

Um editor de documentos multi-aba dentro do app (Markdown/HTML/CSV/texto) onde
**o usuário escreve e a IA assiste** (resumir, reescrever trechos, sugerir
edições), com versionamento e biblioteca. Resolve a ausência de um espaço de
autoria persistente — hoje todo texto vive efêmero no chat.

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/documents.rs`; tabelas `documents`
  e `document_versions` (SQLite, via migração). LLM via `chat_with_routing`.
- **Frontend:** novo módulo JS (padrão `wave1.js`) + aba `Documentos`. Editor com
  **CodeMirror 6** (mais leve que Monaco) e realce via `highlight.js`. Assets
  vendorizados localmente em `apps/desktop-ui/ui/vendor/` (offline-first).
- **Formatos:** markdown, html, csv, txt; preview de markdown reaproveitando o
  `renderMarkdown()` já existente no `app.js`.
- **Edições por IA:** seleção de trecho → ação (resumir/reescrever/continuar) →
  request ao LLM → diff aplicável (aceitar/rejeitar).

### Referência no Odysseus (exemplo para consulta)

- `routes/document_routes.py`, `routes/document_helpers.py`,
  `routes/editor_draft_routes.py` — CRUD, rascunhos e versões.
- `src/document_processor.py`, `src/document_actions.py` — processamento e ações de IA.
- `static/js/document.js`, `static/js/documentLibrary.js`, `static/js/editor/` —
  UI do editor multi-aba, biblioteca e fluxo de edição assistida.

## 3. Regras de Negócio e Restrições

- **PODE:** criar/editar/excluir/listar documentos; salvar versões; exportar;
  aplicar edições de IA com aceite explícito do usuário.
- **PODE:** múltiplas abas abertas simultâneas no editor.
- **NÃO PODE:** sobrescrever conteúdo do usuário com saída de IA sem confirmação
  (sempre via diff/aceite).
- **NÃO PODE:** perder dados — toda gravação cria/atualiza versão; autosave com debounce.
- **NÃO PODE:** depender de CDN remoto para o editor — assets locais.
- **Limite:** tamanho máximo de documento configurável (ex.: 5 MB de texto).

## 4. Critérios de Aceite

- [ ] CRUD: `GET/POST /api/documents`, `GET/PUT/DELETE /api/documents/{id}`.
- [ ] Versões: `GET /api/documents/{id}/versions` e restauração de versão.
- [ ] Aba `Documentos` com editor multi-aba, seletor de linguagem e preview MD.
- [ ] Ação de IA sobre seleção gera sugestão exibida como diff; aceitar aplica,
      rejeitar descarta.
- [ ] Autosave + restauração de versão funcionam; export (md/html/txt) baixa arquivo.
- [ ] Editor funciona **offline** (assets vendorizados).

## 5. Plano de Implementação

1. **Tabelas** `documents` (+ conteúdo, linguagem, atualizado_em) e
   `document_versions`; CRUD em `state_store.rs`.
2. **Endpoints** de CRUD, versões, export.
3. **Vendor** CodeMirror 6 + highlight.js em `ui/vendor/`.
4. **UI base:** aba/painel, abas internas de documentos, biblioteca (listar/abrir).
5. **Editor:** integrar CodeMirror, troca de linguagem, preview MD, autosave (debounce).
6. **Edições por IA:** endpoint `POST /api/documents/{id}/assist` (ação + seleção)
   usando `chat_with_routing`; render de diff e aceite/rejeição no front.
7. **Versionamento na UI:** linha do tempo de versões + restaurar.
