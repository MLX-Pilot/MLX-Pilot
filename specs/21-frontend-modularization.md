# 21 — Modularização do Frontend (refator sem mudança visual)

> **Tipo:** refator de arquitetura (frontend) · **Esforço:** L · **Depende de:** —
> **Restrição-mãe:** paridade total — **nada** do que o usuário vê ou de como o app
> se comporta pode mudar. É reorganização de código, não redesign.

## 1. Objetivo

Quebrar o frontend monolítico (hoje `app.js` ~159 KB num único IIFE, `wave1.js`
~46 KB, `styles.css` ~70 KB, `index.html` ~41 KB) em **módulos ES nativos**
focados e pequenos, mantendo **exatamente** a mesma aparência e o mesmo
comportamento. Resolve o gargalo de manutenção: cada nova feature (Wave 2–7) hoje
precisa mexer em arquivos gigantes, com alto risco de conflito/regressão. Modular,
futuras implementações viram "adicionar um módulo + uma aba", como já fizemos com
`wave1.js` — só que de forma sistemática.

## 2. Contexto Técnico

- **Stack atual (mantida):** app desktop Tauri servindo assets estáticos de
  `apps/desktop-ui/ui` (`frontendDist: "../ui"`), **sem bundler / sem passo de
  build de frontend**. `index.html` carrega `<script src="app.js">` e
  `<script src="wave1.js">` (scripts planos, IIFE, sem `import`).
- **Viabilidade no Tauri:** os webviews-alvo (WebView2 no Windows, WKWebView no
  macOS, WebKitGTK no Linux) suportam **ES Modules nativos**. O protocolo de assets
  do Tauri serve `.js` com MIME correto, então `<script type="module" src="js/main.js">`
  + `import/export` funciona **sem bundler** — fiel à filosofia build-free atual.
- **Estratégia escolhida:** ES Modules nativos (sem Vite/esbuild). Um bundler seria
  uma mudança maior de workflow (dev server, `dist/`, HMR) e fica como **alternativa
  de fase 2** numa spec própria, se algum dia for desejado. Esta spec NÃO introduz
  bundler nem dependências novas.
- **Pontos a favor já verificados:** **0 handlers `onclick` inline** no HTML/JS
  ativo (tudo é `addEventListener`) — então não há o problema clássico de "global
  some ao virar módulo". O `app.js` compartilha um objeto `state` e funções por
  closure; modularizar exige extrair `state`/`api`/utils para módulos exportados.
- **Código morto a investigar:** `agent-channels.js`, `agent-control-plane.js`,
  `agent-skills.js`, `particles.js` usam `import/export` mas **não** são
  referenciados por `app.js`/`wave1.js`/`index.html`. Confirmar se são inertes e, se
  forem, removê-los em commit `chore` separado (não assumir cegamente).
- **CSP:** o `tauri.conf.json` não define `security.csp` (CSP atualmente
  permissiva). Se um dia ligar CSP, módulos exigem `script-src 'self'`; a remoção de
  inline handlers (já é o caso) facilita endurecer depois.

### Referência no Odysseus (exemplo para consulta)

> O Odysseus (clonável de github.com/pewdiepie-archdaemon/odysseus) **já tem o
> frontend modularizado** — é o melhor exemplo de como dividir:

- `static/js/` — dezenas de módulos focados (`chat.js`, `memory.js`, `sessions.js`,
  `calendar.js`, `compare/`, `editor/`, `markdown/`, `util/`, etc.).
- `static/js/MODULE_SUMMARY.md` — descreve o mapa de módulos do front (ótimo modelo
  de organização de pastas/responsabilidades).
- `static/index.html` + `static/app.js` — entrypoint que orquestra os módulos.
- README do Odysseus: "modular front-end". *Use como mapa conceitual; o nosso é
  vanilla ESM, sem o stack Python.*

## 3. Regras de Negócio e Restrições

- **PODE:** dividir `app.js`/`wave1.js` em módulos por domínio (core + features),
  dividir `styles.css` em arquivos lógicos, trocar os `<script>` planos por
  `<script type="module">`, extrair `state`/`api`/utils para módulos.
- **NÃO PODE (regra-mãe):** alterar QUALQUER coisa observável — estrutura do DOM,
  `id`/`class`, valores das variáveis CSS, layout, cores, textos, animações, atalhos
  de teclado ou fluxos. Saída renderizada idêntica, byte-a-byte no comportamento.
- **NÃO PODE:** introduzir feature nova, "melhorias" de UX, libs/CDN novas, bundler
  ou passo de build Node. Refator puro.
- **NÃO PODE:** introduzir `onclick`/`oninput` inline dependendo de globais — manter
  `addEventListener`. Se algum módulo precisar de função no `window`, documentar e
  justificar (preferir não).
- **NÃO PODE:** quebrar o carregamento no Tauri — caminhos de import relativos devem
  resolver sob o protocolo de assets; sem dependência de ordem fora do grafo de
  módulos; sem import circular.
- **NÃO PODE:** mudar o `frontendDist` nem mover a pasta `ui` (continua
  `apps/desktop-ui/ui`).
- **Restrição de processo:** refator **incremental** com verificação de paridade a
  cada passo; nunca um "big bang" que reescreve tudo de uma vez.

## 4. Critérios de Aceite

- [ ] App sobe por `cargo tauri dev`/build e está **visual e funcionalmente
      idêntico**: todas as abas (chat, discover, agent, ai-interaction, console,
      historico, memoria, comparar, settings), seletor de modelo, streaming do chat,
      agente, presets, busca, e atalhos funcionam como antes.
- [ ] **Zero** novo erro/aviso no console do webview.
- [ ] `index.html` usa `<script type="module">`; `app.js` deixou de ser monólito —
      lógica dividida em módulos ES focados (meta-guia: nenhum módulo > ~25 KB).
- [ ] `styles.css` dividido em arquivos lógicos com **cascata/resultado idênticos**.
- [ ] Nenhum handler inline dependente de global introduzido.
- [ ] Check de paridade documentado (antes/depois por aba: screenshot + estado de
      DOM) — sem diffs perceptíveis.
- [ ] Código morto (`agent-*.js`/`particles.js`) confirmado e removido em commit
      `chore` separado, OU mantido com justificativa se estiver em uso.
- [ ] Bundle do Tauri continua gerando e rodando na plataforma-alvo.

## 5. Plano de Implementação

1. **Baseline de paridade:** capturar o estado atual por aba (screenshots + dumps de
   DOM/estado) como referência de comparação. Mapear no `app.js` os símbolos globais
   da closure (`state`, `api`, `switchTab`, `renderMarkdown`, `esc`, etc.) e as
   referências cruzadas entre funções.
2. **Confirmar código morto:** verificar se `agent-channels.js`/`agent-control-plane.js`/
   `agent-skills.js`/`particles.js` são carregados em runtime. Se inertes, remover em
   commit `chore` à parte (antes do refator, para reduzir ruído).
3. **Entry point modular:** criar `js/main.js` e trocar, no `index.html`, os
   `<script src=app.js>`/`<script src=wave1.js>` por `<script type="module" src="js/main.js">`.
   `main.js` inicialmente pode reexportar/orquestrar o existente para manter tudo
   funcionando durante a transição.
4. **Extrair o core** (primeiro, pois todos dependem dele): `js/core/state.js`
   (singleton de estado), `js/core/api.js` (daemonUrl + `api()` + helpers SSE),
   `js/core/dom.js` (`esc`, helpers de elemento, toast), `js/core/router.js`
   (`switchTab` + show/hide de painel), `js/core/markdown.js` (`renderMarkdown`).
5. **Migrar feature por feature** para `js/features/*.js` (chat, models/discover,
   agent, ai-visual, console, settings, e os do wave1: memory, history, compare,
   presets), trocando refs de closure por `import`. **Verificar paridade após CADA
   feature migrada** contra o baseline.
6. **Dividir o CSS** em `css/` (ex.: `theme.css` com as variáveis, `base.css`,
   `components.css`, e opcionalmente `panels/*.css`), referenciado por `@import`
   num arquivo de entrada ou múltiplos `<link>` — mantendo a cascata e o resultado
   idênticos.
7. **Limpeza final:** remover restos do monólito; conferir ausência de imports
   circulares; revisar que nada virou global desnecessário.
8. **Verificação final de paridade:** rodar o app real (Tauri), comparar todas as
   abas com o baseline (visual + funcional), console limpo, e validar o bundle.
9. **Micro-commits profissionais:** trabalhar num branch dedicado
   (`refactor/frontend-esm-modularization`), em commits pequenos e coerentes —
   `chore: remove dead frontend files`, `refactor(ui): extract core state/api/dom`,
   `refactor(ui): modularize chat feature`, etc. — cada um buildando e mantendo
   paridade; finalizar cada mensagem com `Co-Authored-By: Claude Opus 4.8
   <noreply@anthropic.com>`. Nada de commit gigante.
