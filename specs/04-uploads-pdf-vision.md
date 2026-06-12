# 04 — Uploads, Extração de PDF e Visão

> **Tipo:** nova feature · **Esforço:** L · **Depende de:** Infra Jobs (01, p/ OCR
> assíncrono) · **Habilita:** import de Memória, Documents (05), anexos no Chat/Agent.

## 1. Objetivo

Permitir anexar arquivos (imagens, PDFs, documentos Office/texto) ao Chat e ao
Agent, com (a) **deduplicação por hash**, (b) **extração de texto** de PDF e
Office para uso como contexto, e (c) **visão**: enviar imagens a modelos
multimodais. Resolve a ausência total de ingestão de arquivos, hoje um requisito
básico de qualquer workspace de IA.

## 2. Contexto Técnico

- **Linguagem:** Rust; módulo `crates/daemon/src/uploads.rs`.
- **Upload:** Axum `Multipart` (`axum::extract::Multipart`); limites por env
  (`APP_UPLOAD_MAX_BYTES`).
- **Hash/dedup:** `sha2` (já no workspace) → caminho `<data_dir>/uploads/<aa>/<sha256>`.
- **Imagens:** crate `image` (thumbnail, dimensões, EXIF básico); `base64` para
  enviar a modelos de visão via `chat_with_routing` (mensagem multimodal).
- **PDF:** extração de texto com `pdf-extract` ou `lopdf` (puro Rust; **evitar**
  PyMuPDF/AGPL). Render de página (opcional, fase 2) com `pdfium-render`.
- **Office → texto:** fase 2 — conversão `.docx/.xlsx/.pptx` via util externo
  opcional; no MVP, suportar texto/markdown/csv direto.
- **Metadados:** tabela `uploads` no SQLite (id, sha256, nome, mime, tamanho,
  width/height, criado_em, texto_extraído_ref).
- **Limpeza:** job (infra 01) remove uploads órfãos antigos.

### Referência no Odysseus (exemplo para consulta)

- `routes/upload_routes.py`, `src/upload_handler.py`, `src/upload_limits.py` —
  fluxo de upload, limites e dedup.
- `src/pdf_forms.py`, `src/pdf_form_doc.py`, `src/pdf_runtime.py` — PDF e formulários.
- `src/markitdown_runtime.py` — conversão Office→Markdown (dependência opcional).
- `src/personal_docs.py`, `src/document_processor.py` — ingestão/processamento.

## 3. Regras de Negócio e Restrições

- **PODE:** aceitar imagem/PDF/texto; deduplicar por SHA-256; extrair texto;
  rotear imagem a modelo de visão quando o provider suportar.
- **NÃO PODE:** exceder o limite configurável de bytes; rejeitar acima disso com 413.
- **NÃO PODE:** executar conteúdo do arquivo nem confiar no mime do cliente —
  validar por assinatura/extensão.
- **NÃO PODE:** depender de PyMuPDF (AGPL) ou de serviço externo para extração.
- **NÃO PODE:** quebrar se o provider ativo não tiver visão — degradar para
  "extrair texto / não suportado" com mensagem clara.
- **Privacidade:** arquivos ficam locais; nada some sem ação do usuário.

## 4. Critérios de Aceite

- [ ] `POST /api/uploads` (multipart) salva, deduplica e retorna metadados.
- [ ] `GET /api/uploads/{id}` e `GET /api/uploads/{id}/raw` servem metadado/arquivo.
- [ ] `POST /api/uploads/{id}/extract` retorna texto de PDF/texto.
- [ ] `POST /api/uploads/{id}/vision` (com prompt) responde via modelo multimodal
      quando disponível; senão erro tratável.
- [ ] Upload acima do limite → 413; mime forjado é rejeitado.
- [ ] Anexo no Chat: arrastar/soltar imagem ou PDF e usá-lo na próxima mensagem.
- [ ] `cargo build` verde.

## 5. Plano de Implementação

1. **Tabela `uploads`** via migração; helpers de CRUD em `state_store.rs`.
2. **Endpoint multipart** com limite, validação de assinatura e dedup por SHA-256.
3. **Armazenamento** particionado em `<data_dir>/uploads/`; thumbnails com `image`.
4. **Extração de PDF/texto** (`pdf-extract`/`lopdf`); persistir texto extraído.
5. **Visão:** montar mensagem multimodal (base64) e chamar `chat_with_routing`;
   detectar capacidade de visão do provider.
6. **Job de limpeza** de órfãos (infra 01).
7. **UI:** botão/área de anexo no Chat (drag-drop) + painel simples de uploads;
   feeds para "importar em Memória" e Documents.
