# Workflows nativos (`mlx-flow`)

O MLX Pilot executa workflows com um motor proprio, compilado dentro do daemon.
Nao ha servico externo para subir, porta extra para abrir nem chave de API para
gerar: salvar e executar um fluxo e uma operacao local do proprio binario.

Isso substitui a integracao anterior com o n8n, que dependia de uma instancia
rodando em `127.0.0.1:5678`, de uma API key criada a mao e de um runtime Node
completo (Node >= 24 e pnpm) construido a partir de `vendor/n8n`.

## Arquitetura

O crate fica em `crates/flow` e nao conhece o daemon:

| Modulo | Responsabilidade |
| --- | --- |
| `model` | Formato `mlxflow.v1`: nos e arestas. |
| `graph` | Validacao estrutural e ordenacao topologica em camadas (Kahn). |
| `expr` | Interpretador das expressoes `{{ ... }}`. |
| `registry` | Contrato `NodeExecutor` e catalogo de tipos de no. |
| `engine` | Escalonador assincrono sobre Tokio. |
| `nodes` | Nos embutidos. |
| `host` | Trait `FlowHost`: ponte para o agente e as ferramentas. |
| `store` | Persistencia em disco de fluxos e execucoes. |
| `import` | Conversor offline de workflows exportados do n8n. |

O daemon implementa `FlowHost` em `crates/daemon/src/flows.rs`, o que permite
testar o motor inteiro sem subir provedor de modelo nenhum.

### Execucao

1. O grafo e validado e convertido em camadas topologicas.
2. Cada camada roda com os nos em paralelo, limitados por `settings.max_parallel`.
3. Entre camadas ha uma barreira: todo no ve as saidas completas dos anteriores.
4. Um no so executa se pelo menos uma aresta de entrada estiver **ativa** — isto
   e, se o no de origem teve sucesso e emitiu itens naquela porta. E assim que o
   `flow.if` poda o ramo nao escolhido.

Ciclos sao rejeitados na validacao, com o caminho completo do ciclo na mensagem.

## Formato `mlxflow.v1`

```json
{
  "schema": "mlxflow.v1",
  "name": "Resumo diario",
  "active": false,
  "nodes": [
    {
      "id": "n1",
      "name": "Inicio",
      "kind": "trigger.manual",
      "parameters": {},
      "position": { "x": 240, "y": 200 }
    },
    {
      "id": "n2",
      "name": "Dobrar",
      "kind": "data.set",
      "parameters": {
        "keep_only_set": true,
        "assignments": [{ "name": "dobro", "value": "{{ $json.n * 2 }}" }]
      },
      "position": { "x": 500, "y": 200 }
    }
  ],
  "edges": [{ "from": "n1", "to": "n2" }],
  "settings": { "timeout_secs": 300, "max_parallel": 8, "save_runs": true }
}
```

Diferencas em relacao ao formato do n8n:

- As conexoes sao uma **lista plana de arestas** com porta de origem explicita,
  e nao um mapa aninhado indexado pelo nome do no. Renomear um no nao quebra o
  grafo.
- `position` e um objeto `{ x, y }`, nao um par posicional.
- Nao existe `typeVersion`: o tipo do no e o contrato.

O **nome** de cada no precisa ser unico dentro do fluxo, porque e a chave usada
por `$node["Nome"]`. A validacao recusa nomes repetidos em vez de deixar a
expressao falhar silenciosamente em tempo de execucao.

### Por no

| Campo | Efeito |
| --- | --- |
| `disabled` | O no repassa a entrada para a saida sem executar. |
| `on_error` | `stop` (padrao) aborta a execucao; `continue` segue com os itens de entrada. |
| `retry` | `{ "max_attempts": 3, "delay_ms": 500 }`, ate 10 tentativas. |

## Nos embutidos

| Tipo | O que faz |
| --- | --- |
| `trigger.manual` | Inicia sob demanda, pelo botao Executar ou pela API. |
| `trigger.webhook` | Publica `/flows/webhook/<caminho>` no proprio daemon. |
| `trigger.schedule` | Dispara por expressao cron de seis campos. |
| `http.request` | Chamada HTTP, uma por item de entrada. |
| `data.set` | Define campos em cada item; nomes com ponto criam objetos aninhados. |
| `flow.if` | Divide os itens entre as saidas `true` e `false`. |
| `flow.merge` | Reune os itens de varios ramos. |
| `debug.log` | Registra uma mensagem no historico e repassa os itens. |
| `agent.run` | Chama o agente do MLX Pilot **no mesmo processo**. |
| `tool.call` | Roda uma ferramenta do agente dentro do sandbox existente. |

`agent.run` e `tool.call` sao o motivo de o motor ser embutido: nao ha viagem de
ida e volta por HTTP, nao ha credencial para configurar, e `tool.call` reaproveita
o `ToolRegistry` e o sandbox que o agente ja usa.

O catalogo — rotulos, cores, parametros padrao e os campos do formulario — e
publicado em `GET /flows/node-types`. A paleta e o inspetor da UI sao montados a
partir dele, entao registrar um no novo em Rust o faz aparecer na interface sem
mexer no JavaScript.

## Expressoes

Qualquer string de parametro que contenha `{{ ... }}` e tratada como template.
Se a string inteira for uma unica expressao, o tipo JSON e preservado
(`{{ 1 + 1 }}` vira o numero `2`); com texto em volta, o resultado e interpolado.

Nao ha motor JavaScript embutido: o interpretador e proprio, deterministico e
sem acesso ao sistema de arquivos, a rede ou ao ambiente do processo.

### Variaveis

| Variavel | Conteudo |
| --- | --- |
| `$json` | Item atual. |
| `$items` | Todos os itens de entrada do no. |
| `$index` | Indice do item atual. |
| `$node["Nome"]` | `{ json, items, count }` da saida de um no anterior. |
| `$env` | Variaveis passadas na execucao. **Nao** expoe o ambiente do processo. |
| `$now` | Instante da execucao, em RFC 3339. |
| `$runId`, `$flowId`, `$nodeName` | Identificadores da execucao. |

Caminho inexistente devolve `null` em vez de erro, para um campo opcional
faltando nao derrubar o fluxo. Ja uma variavel desconhecida e erro, o que pega
erro de digitacao.

### Operadores e funcoes

Operadores: `+ - * / %`, `== != < <= > >=`, `&& || !`, com `&&` e `||` em
curto-circuito. Indice negativo conta do fim: `$json.lista[-1]`.

Funcoes: `upper`, `lower`, `trim`, `str`, `json`, `parseJson`, `num`, `bool`,
`len`, `abs`, `round`, `min`, `max`, `default`, `if`, `contains`, `split`,
`join`, `replace`, `keys`, `values`, `get`, `now`, `uuid`.

```
{{ upper($json.user.name) }}
{{ default($json.titulo, 'sem titulo') }}
{{ $node["Buscar dados"].json.status == 200 }}
{{ join(split($json.tags, ','), ' | ') }}
```

Regra de veracidade, usada por `!`, `&&`, `||` e pelo `flow.if`: `null`, `false`,
`0`, string vazia, lista vazia e objeto vazio sao falsos; o resto e verdadeiro.

## API

| Rota | Efeito |
| --- | --- |
| `GET /flows` | Lista os fluxos salvos. |
| `POST /flows` | Cria ou atualiza. Valida antes de gravar. |
| `GET /flows/{id}` | Um fluxo completo. |
| `DELETE /flows/{id}` | Remove o fluxo e seu historico. |
| `POST /flows/{id}/run` | Executa e devolve o registro completo. |
| `POST /flows/validate` | Valida sem gravar. |
| `GET /flows/node-types` | Catalogo de nos e ferramentas. |
| `GET /flows/runs` | Historico geral. |
| `GET /flows/{id}/runs` | Historico de um fluxo. |
| `GET /flows/runs/{run_id}` | Uma execucao completa. |
| `POST /flows/import/n8n` | Converte um workflow do n8n. |
| `ANY /flows/webhook/{caminho}` | Gatilho de webhook. |

Executar um fluxo:

```bash
curl -s -X POST http://127.0.0.1:11435/flows/$FLOW_ID/run \
  -H 'content-type: application/json' \
  -d '{"payload": {"n": 30}}'
```

O `payload` vira os itens do no inicial; uma lista JSON gera varios itens.

## Gatilhos automaticos

Webhook e agenda so ficam publicados quando o fluxo esta **ativo** — o
interruptor `active` e o que decide isso. Fluxos inativos continuam executaveis
manualmente.

O agendador roda a cada 30 segundos e guarda o ultimo disparo de cada gatilho em
memoria, para nao repetir a mesma ocorrencia entre dois ticks. Subir o daemon
nao dispara execucoes atrasadas em lote: na primeira vez que um gatilho e visto,
apenas o relogio e marcado.

O `trigger.webhook` aceita `response_mode`:

- `last_node` (padrao): responde com a saida do fluxo.
- `immediate`: responde `202` na hora e executa em segundo plano.

## Armazenamento

Um arquivo JSON por fluxo em `<config>/flows/` e um por execucao em
`<config>/flow-runs/`, onde `<config>` e o diretorio de configuracao do MLX
Pilot. A escrita e atomica (arquivo temporario + rename). O historico mantem as
200 execucoes mais recentes.

As amostras de saida guardadas no historico sao truncadas em 20 itens por no e
8000 caracteres por item, para o historico nao virar um dump.

## Migracao de workflows do n8n

`POST /flows/import/n8n` converte um workflow exportado do n8n. E uma conversao
de arquivo: nao contata instancia nenhuma. Na UI, o botao **Importar** detecta o
formato automaticamente.

A conversao cobre `manualTrigger`, `webhook`, `scheduleTrigger`, `cron`,
`httpRequest`, `set`, `if`, `filter`, `merge`, `noOp` e `respondToWebhook`. Um
`httpRequest` apontado para `/agent/run` vira o no nativo `agent.run`.

Tipos sem equivalente viram `debug.log` com uma nota preservando o tipo e os
parametros originais: o desenho do grafo sobrevive e a lacuna fica visivel no
editor. A resposta sempre traz `warnings` e `unsupported_kinds`.

Limites conhecidos, reportados como aviso:

- Do `if`, so a primeira condicao e convertida.
- Nos com mais de uma saida (fora o `if`) tem todas as saidas ligadas na
  principal.
- O `Code` node nao tem equivalente: nao ha motor JavaScript neste motor.

## Testes

```bash
cargo test -p mlx-flow
```

```bash
npm --prefix apps/desktop-ui run test:e2e:agent-workspace-ui
```
