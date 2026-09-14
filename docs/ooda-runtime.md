# Runtime OODA — agente autônomo para tarefas multi-etapa

Implementa o issue [#35](https://github.com/MLX-Pilot/MLX-Pilot/issues/35).

## Por que

O `AgentLoop` é reativo: o modelo emite `tool_calls`, o loop executa, devolve o resultado, e repete até o modelo decidir parar. Funciona bem num pedido direto ("leia esse arquivo"), mas num pedido de várias etapas o modelo local pequeno perde o fio.

Medido com `qwen2.5:7b-instruct-q4_K_M` na tarefa *"descubra quantos arquivos .rs existem em src/, leia cada um, e escreva docs/analise.md listando as funções públicas"*:

| | `hermes_inspired` | `ooda` |
|---|---|---|
| Desfecho | parou na 4ª iteração dizendo "agora vou ler `main.rs`" — e não leu | 6/6 passos concluídos |
| Artefato gerado | template vazio (`// Aqui vêm as funções públicas encontradas`) | `parse_config` — a função real do projeto |
| Tool calls | 3 | 7 |

O runtime reativo não abandona a tarefa por erro: ele *acha que terminou*. Sem um plano explícito não há nada que discorde.

## As quatro fases

| Fase | O que faz | Onde |
|---|---|---|
| **Observe** | Resume o estado: passos concluídos, resultados, falhas | `OodaState::observation` |
| **Orient** | Pede ao modelo um plano de passos curtos; revisa quando algo falha | `orient_prompt` + `parse_plan_steps` |
| **Decide** | Escolhe a próxima ação a partir do estado e dos limites | `decide` |
| **Act** | Executa o passo com as ferramentas disponíveis | `AgentLoopExecutor::act` |

A fase Act delega ao `AgentLoop`, então **política, aprovação, auditoria e seleção de ferramentas continuam valendo** — o OODA orquestra, não substitui.

### Decide é função pura

`decide(&OodaState, &OodaLimits) -> OodaDecision` não faz I/O. É a regra de parada do agente, e regra de parada precisa ser testável sem subir modelo nem daemon — há 11 testes cobrindo cada limite isoladamente.

Decisões possíveis: `Execute { step_id }`, `Replan { reason }`, `Stop { reason }`.

## Controle de estado e limites

Todo limite tem um `stop_reason` correspondente, então o motivo da parada sempre aparece na resposta — nunca há parada silenciosa.

| Limite | Default | `stop_reason` |
|---|---|---|
| `max_cycles` | 12 | `max_cycles` |
| `max_steps` | 8 | — (trunca o plano) |
| `max_attempts_per_step` | 2 | marca o passo como `failed` |
| `max_tool_calls` | 40 | `max_tool_calls` |
| `max_replans` | 2 | — (para de replanejar, segue executando) |
| `deadline` | 600s | `deadline` |

Dois detalhes que só aparecem na prática:

- **Tentativas sobrevivem ao replanejamento.** Se o modelo devolve um passo com a mesma descrição, é o mesmo passo, e o contador de tentativas é preservado. Zerá-lo permitia que um passo insistentemente falho consumisse tentativas para sempre, porque `max_attempts_per_step` nunca era alcançado.
- **A fase Act não enxerga os passos futuros.** Com o plano inteiro à vista, o modelo se adianta: no passo "ler cada arquivo" ele já pulava para "escrever o relatório", e o relatório saía com conteúdo inventado (`funcao1`, `funcao2`) porque os arquivos nunca foram lidos. `observation_for_act` mostra só o histórico e o passo atual.

## Uso

```bash
curl -X POST http://127.0.0.1:11435/agent/run \
  -H 'content-type: application/json' \
  -d '{
    "message": "analise o projeto e escreva docs/analise.md",
    "provider": "ollama",
    "model_id": "qwen2.5:7b-instruct-q4_K_M",
    "runtime_variant": "ooda",
    "workspace_root": "/caminho/do/workspace",
    "ooda_max_cycles": 8,
    "ooda_deadline_secs": 300
  }'
```

Campos opcionais: `ooda_max_cycles`, `ooda_max_steps`, `ooda_max_tool_calls`, `ooda_deadline_secs`. Valor `0` é tratado como ausente — todo run OODA precisa de um teto.

A resposta ganha um bloco `ooda` com o plano, os ciclos e o motivo da parada:

```json
{
  "final_response": "...",
  "ooda": {
    "stop_reason": "completed",
    "plan": {
      "objective": "...",
      "steps": [
        { "id": 1, "description": "Contar arquivos .rs", "status": "done",
          "attempts": 1, "result": "Foram encontrados 2 arquivos .rs em src/." }
      ]
    },
    "cycles": [
      { "cycle": 1, "decision": "execute", "step_id": 1, "tool_calls": 2, "elapsed_ms": 1840 }
    ],
    "tool_calls_made": 7,
    "replans": 0,
    "elapsed_ms": 21751
  }
}
```

## Limitações conhecidas

- **Qualidade do plano é do modelo.** Um 7B produz passos redundantes ("salvar e fechar o arquivo") e às vezes não cobre tudo. O ciclo garante que os passos sejam executados e registrados; não garante que o plano seja bom.
- **O parser de plano é textual, não JSON.** Modelos pequenos não produzem JSON confiável, então pede-se lista numerada e o parser aceita `1.`, `1)`, `-` e `*`, ignorando preâmbulo e despedida. Um plano vazio vira `stop_reason: "empty_plan"` em vez de um run inútil.
- **Custo.** Um run OODA gasta mais chamadas que o reativo (plano + síntese, além dos passos). Vale para tarefas multi-etapa; para um pedido direto, use `classic` ou `hermes_inspired`.

## Onde olhar no código

- `crates/agent-core/src/ooda.rs` — fases, limites, controlador, executor (25 testes)
- `crates/agent-core/src/agent_runtime.rs` — despacho da variante, `ooda_response`, `map_ooda_stop_reason`
- `crates/daemon/src/agent_api.rs` — `resolve_ooda_limits`, campo `ooda` na resposta
