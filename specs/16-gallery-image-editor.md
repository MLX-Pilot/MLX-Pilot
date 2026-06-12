# 16 — Galeria e Editor de Imagem

> **Tipo:** nova feature · **Esforço:** L (galeria) → XL (editor/geração)
> **Depende de:** Uploads (04) · **Escopo TCC:** galeria read-only primeiro;
> edição/geração opcional.

## 1. Objetivo

Uma galeria de imagens (uploads + geradas) com metadados/EXIF e transformações
básicas, evoluindo para um editor (crop/rotate/filtros) e **geração/inpaint** via
servidores de difusão externos. Resolve a ausência de qualquer fluxo visual e
complementa a capacidade de visão (04). **Nota:** geração depende de infra externa
(Stable Diffusion) raramente presente num desktop local — manter opcional.

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/gallery.rs`; reutiliza armazenamento/
  dedup de Uploads (04). EXIF/thumbnail/transform via `image` + `imageproc`.
- **Geração (opcional, fase 2):** proxy HTTP a um servidor de difusão configurável
  (A1111/ComfyUI) — **não** embutir modelos de difusão no app.
- **Frontend:** aba `Galeria` (padrão `wave1.js`): grid de imagens, visualização,
  rotacionar/recortar; (fase 2) canvas de edição e inpaint.

### Referência no Odysseus (exemplo para consulta)

- `routes/gallery_routes.py`, `routes/gallery_helpers.py` — galeria, transform, limites.
- `mcp_servers/image_gen_server.py` — geração de imagem como servidor MCP.
- `static/js/gallery.js`, `static/js/galleryEditor.js` — grid e editor/canvas.
- `services/faces/` — utilidades de face (referência avançada, opcional).

## 3. Regras de Negócio e Restrições

- **PODE:** listar/ver/baixar imagens; rotacionar/recortar/redimensionar;
  (fase 2) gerar/inpaint via servidor externo configurado.
- **NÃO PODE:** embutir/baixar modelos de difusão no app (peso/licença); geração é
  sempre via servidor externo opcional.
- **NÃO PODE:** exceder limites de upload/transform (config por env).
- **NÃO PODE:** quebrar se nenhum servidor de geração estiver configurado — galeria
  e edição local seguem funcionando.
- **Restrição:** transformações locais não devem travar a UI (operar no backend ou
  em worker).

## 4. Critérios de Aceite

- [ ] Aba `Galeria` lista imagens (uploads + geradas) com thumbnails e metadados.
- [ ] Visualizar, baixar e rotacionar/recortar/redimensionar funcionam (transform local).
- [ ] EXIF básico exibido quando presente.
- [ ] (Fase 2) Geração/inpaint via servidor externo configurado; ausência → recurso
      desabilitado com aviso, sem erro.
- [ ] `cargo build` verde.

## 5. Plano de Implementação

1. **Galeria read-only:** reaproveitar metadados de Uploads (04); endpoint de listagem
   filtrando imagens; grid na UI.
2. **Transformações locais:** endpoints de rotate/crop/resize com `image`/`imageproc`.
3. **Metadados/EXIF:** extrair e exibir.
4. **(Fase 2) Proxy de geração:** config de servidor externo; endpoints de
   gerar/inpaint; canvas de edição na UI.
