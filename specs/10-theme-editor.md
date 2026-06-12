# 10 — Editor de Tema (Aparência)

> **Tipo:** nova feature (majoritariamente frontend) · **Esforço:** L · **Depende de:** —

## 1. Objetivo

Permitir personalizar a aparência do app ao vivo: cores (paleta/variáveis CSS),
fonte, densidade, e efeitos de fundo, com presets e persistência. Resolve a
rigidez visual atual (tema fixo) e dá um diferencial de polimento ao TCC.

## 2. Contexto Técnico

- **Frontend:** módulo JS (padrão `wave1.js`) + aba `Aparência`. O app já usa
  **variáveis CSS** (`--cyan`, `--bg-deep`, `--text-primary`, etc.) — o editor
  apenas sobrescreve essas variáveis no `:root` em tempo real.
- **Cores:** conversões hex↔HSL em JS puro (ou crate `colorsys` se algo for ao backend).
- **Persistência:** preferências em JSON no diretório de config do app
  (`AppConfig`) via `GET/PUT /api/prefs/theme`; presets custom em
  `/api/prefs/custom-themes`.
- **Fontes:** descoberta de fontes custom em pasta local (`GET /api/fonts/custom`),
  opcional; vendorizar qualquer fonte (offline).
- **Backend:** módulo leve `crates/daemon/src/prefs.rs` só para ler/gravar prefs
  (sem lógica pesada).

### Referência no Odysseus (exemplo para consulta)

- `static/js/theme.js` — editor de tema/cores/efeitos ao vivo.
- `routes/font_routes.py` — fontes custom; `routes/emoji_routes.py` — assets de UI.
- `ROADMAP.md` do Odysseus comenta o cuidado com `static/style.css` e overrides mobile.

## 3. Regras de Negócio e Restrições

- **PODE:** editar cores/fonte/densidade/efeitos; salvar/carregar presets; resetar
  para o padrão.
- **NÃO PODE:** depender de CDN/fonte remota — assets locais (offline-first).
- **NÃO PODE:** persistir tema em local que conflite com o tema base — usar prefs
  isoladas que sobrescrevem variáveis.
- **NÃO PODE:** introduzir CSS que quebre acessibilidade básica (contraste mínimo);
  avisar se o contraste ficar muito baixo.
- **Restrição:** aplicar mudanças sem recarregar a página (mutação de `:root`).

## 4. Critérios de Aceite

- [ ] Aba `Aparência` com seletores de cor (paleta), fonte, densidade e efeito de fundo.
- [ ] Mudanças refletem imediatamente em todo o app (variáveis CSS).
- [ ] Salvar persiste e recarrega o tema no próximo boot; "Resetar" volta ao padrão.
- [ ] Presets custom podem ser salvos, listados e aplicados.
- [ ] Funciona offline; aviso de contraste baixo quando aplicável.

## 5. Plano de Implementação

1. **Prefs backend:** `prefs.rs` + `GET/PUT /api/prefs/theme` (JSON em config dir).
2. **Mapa de variáveis:** catalogar as CSS vars do tema atual que serão editáveis.
3. **UI de edição:** color pickers, seletor de fonte/densidade, sliders de efeito;
   aplicar via `document.documentElement.style.setProperty`.
4. **Persistência + presets:** salvar/carregar; lista de presets custom.
5. **Boot:** carregar tema salvo no início (no `wave1`/`app.js`) antes do render.
6. **Acessibilidade:** checagem de contraste com aviso.
