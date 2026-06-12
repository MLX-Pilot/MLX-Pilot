# 08 — Slash Commands e Marketplace de Skills

> **Tipo:** completar/expandir (crate `agent-skills` já existe) · **Esforço:** M
> **Depende de:** Infra Jobs (01, p/ teste/auditoria em lote).

## 1. Objetivo

Tornar as **skills** editáveis e testáveis pela UI e adicionar **slash commands**
(`/comando`) com autocomplete no campo de chat, invocando skills/ações rapidamente.
Resolve o fato de hoje as skills serem geridas só por arquivos/CLI, sem edição,
teste ou descoberta amigável.

## 2. Contexto Técnico

- **Backend:** Rust; estende `crates/agent-skills` e `crates/daemon/src/agent_api.rs`
  (já há `/agent/skills*`). Skills persistidas como arquivos `SKILL.md`
  (frontmatter + corpo) e/ou em `MemoryStore` (kind=`skill`).
- **Frontmatter:** parse via `regex`/`serde_yaml` (nome, descrição, gatilho/slash,
  capabilities). LLM via `chat_with_routing` (juiz de teste/auditoria).
- **Teste/auditoria:** rodar a skill com um caso e avaliar saída (juiz IA) — como
  **job** (infra 01) para não travar a request.
- **Frontend:** expandir a aba "Ferramentas & Skills" (grade de cards, editor de
  frontmatter+corpo, runner de teste) e **popup de autocomplete de slash** no
  `#chat-input` (e `#agent-chat`), alimentado por `GET /agent/skills/slash-catalog`.

### Referência no Odysseus (exemplo para consulta)

- `routes/skills_routes.py`, `static/js/skills.js` — CRUD, edição e teste de skills.
- `static/js/slashCommands.js`, `static/js/slashAutocomplete.js` — catálogo e popup
  de autocomplete de comandos `/`.

## 3. Regras de Negócio e Restrições

- **PODE:** criar/editar/excluir/instalar skills; testar e auditar; expor um
  catálogo de slash commands; invocar skill via slash.
- **NÃO PODE:** burlar as `capabilities`/integridade (SHA256) já existentes no
  modo enterprise/paranoid — manter validação.
- **NÃO PODE:** tratar conteúdo de skill como confiável — auditoria de
  prompt-injection sobre skills/notas/documentos é requisito.
- **NÃO PODE:** executar skill sem passar por policy/approval do agente.
- **Restrição:** autocomplete deve ser local e instantâneo (sem round-trip por tecla).

## 4. Critérios de Aceite

- [ ] CRUD de skills via API; editor de frontmatter+corpo na UI salva e recarrega.
- [ ] `GET /agent/skills/slash-catalog` lista comandos; digitar `/` no chat abre
      popup com filtragem.
- [ ] Selecionar um slash insere/dispara a skill correspondente.
- [ ] Runner de teste executa a skill e mostra resultado + veredicto do juiz IA
      (como job, com progresso).
- [ ] Validação de capabilities/integridade preservada.

## 5. Plano de Implementação

1. **Endpoints de CRUD/markdown** de skill (criar/editar/excluir/obter conteúdo).
2. **Slash-catalog:** endpoint que deriva comandos do frontmatter das skills ativas.
3. **Autocomplete no front:** detectar `/` no `#chat-input`, popup filtrável,
   inserção/disparo.
4. **Runner de teste/auditoria:** endpoint que cria job (infra 01), roda a skill,
   aplica juiz IA, retorna progresso/veredicto.
5. **UI:** grade de skills, editor, runner; integrar com a aba existente.
6. **Salvaguardas:** reusar validação de capabilities/integridade.
