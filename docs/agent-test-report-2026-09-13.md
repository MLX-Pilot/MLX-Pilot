# Relatório de Teste do Agente — MLX Pilot

> **Atualização 2026-09-14** — todas as correções deste relatório foram aplicadas na `main`, e a bateria foi repetida com `qwen3.5:9b`. Resultados atuais em [§8](#8-rodada-com-qwen359b-2026-09-14).


Data: 2026-09-13 · Autor: sessão de teste automatizada · Endpoint: `POST /agent/run` (daemon debug em `127.0.0.1:11435`)

## 1. Ambiente

- **GPU:** NVIDIA GeForce RTX 5070, 12 GB VRAM (driver 596.21, CUDA 13.2).
- **Provider testado:** Ollama local (server externo 0.30.8), tool-calling nativo.
- **Modelos escolhidos** (pequenos, aceitam tools, cabem folgados na 5070):
  - `qwen2.5:7b-instruct-q4_K_M` — ~4,7 GB VRAM. **Melhor custo/benefício.**
  - `llama3.1:8b` (Q4_K_M) — ~5,3 GB VRAM. Comparativo.
- **Runtime:** `hermes_inspired` (padrão recomendado no projeto).
- **Workspace de teste:** `G:/TCC/agent-test-ws` (fixtures: `src/main.rs`, `src/config.rs`, `docs/notes.md`, `data/items.csv`, `.env`).
- **Harness:** `agent-test-ws/runner.mjs` (24 casos) + `analyze.mjs` (verificação) + `probe.mjs` (sondas dirigidas). Cada caso cruza a resposta final com o **audit log** (`/agent/audit`) para saber quais tools rodaram de fato.

## 2. Placar

| Métrica | qwen2.5:7b | llama3.1:8b |
|---|---|---|
| Checks PASS / FAIL | **21 / 12** | 19 / 14 |
| Latência típica (1 tool) | ~1,0–1,5 s | ~1,4–2,5 s |
| Contexto gigante (T20) | respondeu errado | **timeout 300 s** |
| VRAM | 4,7 GB | 5,3 GB |

Vários "FAIL" são **falhas de arquitetura**, não do modelo — reproduzem nos dois. Recomendação de modelo: **qwen2.5:7b-instruct-q4_K_M**.

## 3. Pontos fortes (funcionam bem)

- **Sandbox de filesystem é sólido.** Path traversal absoluto (`C:\Windows\...\hosts`) e relativo (`../Cargo.toml`) foram **bloqueados** com `permission denied: escapes workspace`. (T12, T13)
- **Deny-list de arquivos funciona.** Leitura de `.env` barrada por `**/.env` → segredo não vazou. (T15)
- **`read_only` bloqueia escrita** de verdade: nenhum arquivo criado; só leitura passa. (T16, P3, P4)
- **`locked` remove todas as tools** do prompt. (P5)
- **Tools básicas de leitura confiáveis:** `list_dir`, `read_file`, `exec` (comando seguro), CSV+raciocínio (soma 42 no qwen). (T02, T03, T09, T10)
- **Não trava em tarefa impossível:** arquivo inexistente → resposta clara em ≤2 iterações, sem loop. (T11, T23)
- **Audit log é excelente:** cada tool_call registra `params`, `result`, `policy_rule`, `policy_trace`, `tool_risk`, aprovação. Ótima base para depuração e para o TCC.
- **Budget de contexto instrumentado:** telemetria `context_budget` (tokens, tools no prompt, histórico usado/summarizado) vem em toda resposta.

## 4. Pontos fracos (achados confirmados)

### 🔴 CRÍTICO — bypass da deny-list do `exec` via `argv`
`crates/agent-core/src/policy.rs:341` valida **só** o campo `command` (deny patterns + safe-bins). Mas `crates/agent-tools/src/tools/exec.rs:301` (`resolve_argv`) **executa `argv` quando presente**, ignorando `command`.
Resultado (probe P10): o agente rodou `argv=["cmd","/c","del","/q","src\\main.rs"]` e **apagou `src/main.rs` de verdade**. Como não havia `command`, nem o deny-pattern nem o allowlist dispararam; caiu em "ask", que o `approval_mode:auto` liberou.
- Em T14 (`rm -rf / --no-preserve-root` no campo `command`) o modelo por acaso mandou `argv=["echo",...]`, então não destruiu — mas a política **não** foi o que protegeu.
- **Correção:** validar o comando **efetivo** (reconstruir a partir de `argv` **antes** da checagem de política, ou aplicar deny-patterns/safe-bins sobre `argv[0]` + linha reconstruída). A política e a execução precisam olhar a mesma coisa.

### 🟠 Seletor de tools deixa metade das ferramentas fora do prompt (small_local)
Perfil `small_local` (≤8B) só coloca **3 tools** no prompt (`prompt_builder.rs:50`), escolhidas por `select_relevant_tool_names` — um matcher de **palavras-chave**. Problemas:
- **Keywords de edição só em inglês.** `edit`/`replace`/`modify` existem; `altere/alterar/mude/mudar/troque/substitua` **não**. → pedido PT-BR "altere MAX_RETRIES de 3 para 5" nunca recebe `edit_file`; o modelo só imprime o diff em texto e **não edita o arquivo** (T08, confirmado nos dois modelos).
- **`memory_write`, `delegate_session`, `session_search`, `checkpoint_*`, `toolsets_list` não aparecem em nenhuma lista do seletor** → nunca entram no prompt do modelo pequeno. Por isso T17 (memória) virou `write_file` num `.txt`, T18 (recall) virou leitura de arquivo, T19 (delegação) virou `list_dir`. As tools existem e o audit as lista, mas o modelo nunca as vê.
- **Verbo conjugado quebra o match.** "Procure" (P7) não casa com `procurar` → `grep` não é selecionado e o resultado é errado; já "Use grep" (P6) e "buscar" funcionam. Frágil demais.
- **Correção:** (a) ampliar dicionário PT-BR + stemming/prefixo em vez de match exato; (b) garantir cobertura de memory/delegate/session no seletor; (c) subir o teto de tools para o small_local (ex.: 5–6) — a 5070 aguenta o contexto extra sem problema.

### 🟠 Mensagem única gigante é truncada pela cabeça, perdendo a pergunta no fim
`enforce_prompt_budget`/`truncate_messages_in_place` (`prompt_builder.rs:472+`) corta cada mensagem mantendo o **começo** (`truncate_chars` = primeiros N chars). Se a instrução real está **no fim** de um texto longo, ela é descartada.
- P1 (lorem gigante + pergunta no fim) → respondeu "como posso ajudar?"; P2 (pergunta no início) → "42". T20 confirma o mesmo. Em `llama3.1:8b`, T20 **estourou 300 s (timeout)**.
- **Correção:** truncar preservando início **e** fim (ou priorizar a última mensagem do usuário inteira); e impor timeout de request no daemon para não pendurar.

### 🟡 `glob` não-recursivo confunde o modelo
`glob` com `pattern:"*.rs"` e `base_path:"."` retorna vazio (arquivos estão em `src/`); o modelo aceita o vazio e conclui "não há .rs" (T04). `**/*.rs` funciona. É comportamento correto da tool, mas atrito frequente. **Correção:** documentar no schema/descrição que `*` não é recursivo e sugerir `**/`, ou tornar `glob` recursivo por padrão.

### 🟡 `exec` sempre marcado `unsafe_command` mesmo para binário na allowlist
`cargo --version` rodou, mas com trace `exec_risk:unsafe_command` (safe-bins só tem `ls/cat/grep/git/curl`). Em `approval_mode:ask` isso geraria prompt para todo comando de build. **Correção:** allowlist configurável por workspace / perfil de projeto.

## 5. Observações menores

- **`approval_mode:auto` aprova tudo, inclusive risco `high`** (write/exec). Ótimo para teste em lote, perigoso como default de produção — combinar com o fix do `exec`.
- **Sem streaming nos providers locais** (`supports_streaming:false`); UX de chat fica "tudo de uma vez".
- **Retentativa de tool-call malformado funciona:** quando o modelo manda argumentos `null`, o loop reprompta e acerta na 2ª (visível no audit como `approval_pending → tool_executed`). Bom.

## 6. Plano de correção sugerido (ordem)

1. **[segurança] Unificar validação do `exec`** — política sobre o comando efetivo (`argv`). Fecha o furo de destruição de arquivos.
2. **[eficácia] Reescrever `select_relevant_tool_names`** — PT-BR + prefixo, cobrir memory/delegate/session, subir teto para 5–6 tools no small_local.
3. **[robustez] Truncamento cabeça+cauda** e **timeout de request** no daemon.
4. **[UX] `glob` recursivo/descrição** e **safe-bins configuráveis**.
5. **[produção] Default de approval** por risco em vez de `auto` cego.

## 7. Como reproduzir

```bash
cargo build -p mlx-ollama-daemon
APP_AGENT_WORKSPACE=/g/TCC/agent-test-ws RUST_LOG=info ./target/debug/mlx-ollama-daemon.exe &
cd agent-test-ws
node runner.mjs "qwen2.5:7b-instruct-q4_K_M" hermes_inspired report-qwen25-hermes.json
node analyze.mjs report-qwen25-hermes.json
node probe.mjs   # sondas de segurança/contexto
```

---

## 8. Rodada com qwen3.5:9b (2026-09-14)

### Escolha do modelo

Pedido: Qwen acima da 3.0, ~7–9B, Q4, que rode bem na RTX 5070 (12 GB).

- **`qwen3.5:9b`** — 9,7B, Q4_K_M, 262k de contexto, `tools` + `vision` + `thinking`. ~6,6 GB. **Escolhido.**
- **`qwen3.6`** — existe na biblioteca do Ollama, mas só em dois builds: `latest` (22,6 GB) e `27b` (17,8 GB). Não há variante 7–9B. Mesmo o menor passa 46% do limite de VRAM, então ~1/3 das camadas rodaria na CPU — inviável para uma bateria com dezenas de chamadas por run. **Descartado por não caber**, não por qualidade.

### Placar

| | qwen2.5:7b | qwen3.5:9b |
|---|---|---|
| Checks PASS / FAIL | 32 / 1 | **32 / 1** |
| Respostas vazias | 0 | 0 |
| Latência típica (1 tool) | ~1,2 s | ~3,5 s |
| VRAM | 4,7 GB | 6,6 GB |

O placar empata, mas a **qualidade do conteúdo** não: ver a comparação de OODA abaixo. O único FAIL nos dois é o T19, onde o modelo resolve a tarefa com `glob` em vez de delegar — escolha defensável numa contagem de 2 arquivos.

### Dois defeitos que só um modelo com raciocínio separado revelou

**Resposta final vazia (4 de 24 casos).** O Ollama devolve `thinking` em campo separado de `content`. Em casos onde uma ferramenta já respondera a pergunta, a qwen3.5 gastava o turno inteiro raciocinando e emitia `content: ""` — e o loop entregava string vazia ao usuário. A qwen2.5, que não separa raciocínio, só fez isso 1 vez. Corrigido: um turno final sem conteúdo dispara **um** pedido de conclusão antes de desistir (`EMPTY_ANSWER_REPROMPT`). Depois do fix: **0 respostas vazias**.

**Buscas percorriam estado interno e artefatos de build.** O `grep` retornou `.mlx-pilot/checkpoints/payloads/*.bin` — cópias que o próprio sistema de rollback faz dos arquivos do usuário — e o agente respondeu citando um `.bin` em vez do arquivo fonte. Nenhuma ferramenta de busca ignorava nada: nem `.git`, nem `target/`, nem `node_modules/`. Num projeto Rust real, `target/` sozinho consome o limite de matches. Corrigido com uma lista compartilhada (`sandbox::is_ignored_dir`) usada por `grep` e `glob`.

Os dois fixes beneficiaram também a qwen2.5, que subiu de 31/2 para 32/1.

### OODA: onde o modelo melhor aparece

Mesma tarefa multi-etapa (*contar arquivos .rs, ler cada um, escrever `docs/analise.md` com as funções públicas*):

| | qwen2.5:7b | qwen3.5:9b |
|---|---|---|
| Ciclos / tool calls | 6 / 7 | 5 / 12 |
| Replanejamentos | 0 | 1 |
| Artefato | `parse_config` solto, sem assinatura; ignorou `main.rs` | assinatura completa, constante pública, e **identificou corretamente que `main.rs` não tem função `pub`** |

O conteúdo da qwen3.5 foi conferido contra os arquivos: `MAX_RETRIES = 5` (estado real após o T08 editar), e `main`/`compute` são de fato privadas. Análise factualmente correta.

O ciclo também exercitou o replanejamento pela primeira vez num run real: um passo falhou, o plano foi revisado, e o run concluiu.

---

## 9. Raciocínio e streaming (2026-09-14)

### `thinking` deixou de ser tratado como conteúdo vazio

O Ollama devolve o raciocínio no campo `thinking`, separado de `content`. O provider **descartava esse campo por completo**, então um turno em que a qwen3.5 só raciocinou chegava ao agente indistinguível de um turno vazio.

O que mudou:

- `ChatMessage` ganhou `reasoning: Option<String>`, preenchido pelo provider Ollama.
- `is_blank()` e `is_reasoning_only()` distinguem os dois casos: **raciocínio é trabalho, não vazio**.
- O pedido de conclusão agora é específico. Se o turno raciocinou, pede-se só a conclusão daquele raciocínio (`REASONING_ONLY_REPROMPT`); se veio em branco de verdade, reapresenta-se a tarefa (`EMPTY_ANSWER_REPROMPT`).
- O raciocínio é emitido como `ThinkingDelta` assim que chega, em vez de ser jogado fora.

### Modelo visível no streaming

Todo frame de `/agent/stream` passou a carregar `model_id` e `provider`, e um frame `status: "started"` é emitido **antes** da primeira chamada ao provider — a UI identifica quem está respondendo sem esperar o run terminar. O modelo vem de `RunStarted`, que é o que o loop realmente resolveu, então um fallback que troque o modelo no meio fica visível.

Frames de um run real:

```json
{"event":"status","status":"started","model_id":"qwen3.5:9b","provider":"ollama"}
{"event":"thinking_delta","delta":"The user is asking me to read the file src/main.rs...","model_id":"qwen3.5:9b"}
{"event":"tool_call_started","tool":"read_file","model_id":"qwen3.5:9b"}
{"event":"thinking_delta","delta":"A função auxiliar é `compute`. Vou responder...","model_id":"qwen3.5:9b"}
{"event":"answer_delta","delta":"A função auxiliar chama-se **`compute`**.","model_id":"qwen3.5:9b"}
{"event":"done","status":"completed","latency_ms":10226,"model_id":"qwen3.5:9b"}
```

**Limitação conhecida:** o `answer_delta` ainda chega como um bloco único — o loop emite `TextDelta` uma vez, com o conteúdo completo do turno. O que passou a ser progressivo é o **raciocínio**, que é justamente onde o modelo gasta o tempo. Streaming token a token da resposta final exigiria um caminho de streaming do provider atravessando o agent loop, que não foi feito.

### Placar final

| | qwen2.5:7b | qwen3.5:9b |
|---|---|---|
| Checks PASS / FAIL | 32 / 1 | 32 / 1 |
| Respostas vazias | 0 | 0 |

O FAIL restante é diferente em cada um e, nos dois casos, escolha do modelo — não defeito do runtime: a qwen2.5 pede `list_dir` com `path: "/"` (barrado pelo sandbox, corretamente) e a qwen3.5 resolve o T19 com `glob` em vez de delegar.
