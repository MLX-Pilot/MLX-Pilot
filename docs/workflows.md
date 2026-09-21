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
| `mcp` | Cliente do Model Context Protocol (JSON-RPC sobre stdio). |

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
| `mcp.call` | Roda uma ferramenta de um servidor Model Context Protocol. |

`agent.run` e `tool.call` sao o motivo de o motor ser embutido: nao ha viagem de
ida e volta por HTTP, nao ha credencial para configurar, e `tool.call` reaproveita
o `ToolRegistry` e o sandbox que o agente ja usa.

O catalogo — rotulos, cores, parametros padrao e os campos do formulario — e
publicado em `GET /flows/node-types`. A paleta e o inspetor da UI sao montados a
partir dele, entao registrar um no novo em Rust o faz aparecer na interface sem
mexer no JavaScript.

### Campos herdados do app

Um `FieldSpec` pode declarar `options_source`, e o daemon publica so o nome da
fonte; quem resolve e a UI, que ja mantem essas listas para as outras abas:

| Fonte | Preenchida com |
| --- | --- |
| `agent_providers` | Provedores em dois grupos: **Local (MLX Pilot)** e **Cloud**. |
| `agent_models` | Modelos nos mesmos grupos: os instalados localmente, em ordem, e os das nuvens configuradas. |
| `flow_tools` | Ferramentas registradas, para o `tool.call`. |
| `mcp_servers` | Servidores MCP configurados e habilitados. |
| `mcp_tools` | Ferramentas do servidor MCP escolhido no proprio no. |

Um no `agent.run` recem-criado ja nasce apontando para o mesmo provedor e modelo
que estao ativos no resto do MLX Pilot. Escolher um modelo ajusta o provedor
junto (e o perfil, no caso de nuvem); trocar de provedor repõe o modelo quando o
atual nao pertence mais a ele.

Campos marcados como `advanced` ficam recolhidos em **Avancado** no inspetor.
Sao os que tem um padrao herdado do app e so precisam aparecer para sobrescrever
— `base_url` e `temperature` no `agent.run`, `workspace_root` no `tool.call`.
Deixar qualquer um deles vazio significa "usar o que o agente ja usa".

### Interacoes do editor

| Acao | Como |
| --- | --- |
| Conectar | Arrastar da porta de saida ate o no de destino, ou clicar na saida e depois na entrada. |
| Encadear | Adicionar um no pela paleta com outro selecionado ja cria a conexao. |
| Duplicar | `Ctrl+D`. |
| Desfazer / refazer | `Ctrl+Z` / `Ctrl+Shift+Z` ou `Ctrl+Y`. |
| Remover no | `Delete`. |
| Remover conexao | Clicar na linha. |
| Cancelar ligacao | `Esc`. |

Depois de executar, cada no fica colorido pelo resultado e cada conexao mostra
quantos itens passaram por ela.

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

## Servidores MCP

O no `mcp.call` fala Model Context Protocol com servidores que voce configura na
aba **Workflows**, no card *Servidores MCP*. A implementacao vive em
`crates/flow/src/mcp.rs` e cobre o subconjunto que um workflow precisa:
`initialize`, `notifications/initialized`, `tools/list` e `tools/call`.

O transporte e stdio: o daemon sobe o processo do servidor, conversa por
stdin/stdout em JSON-RPC 2.0 linha a linha e encerra ao terminar. Uma conexao
por chamada e mais lenta que manter o processo vivo, mas nao deixa processo
orfao se o daemon cair — e um no de workflow nao e caminho quente.

O protocolo esta separado do transporte pelo trait `McpTransport`, o que
permite testar o cliente inteiro sem subir processo nenhum.

### Configurar

| Campo | Exemplo |
| --- | --- |
| Nome | `arquivos` |
| Comando | `npx` |
| Argumentos (um por linha) | `-y` / `@modelcontextprotocol/server-filesystem` / `G:/TCC` |
| Ambiente (`CHAVE=valor`, um por linha) | `GITHUB_TOKEN=...` |

Depois de salvar, clique em **Testar**: e essa sondagem que sobe o servidor, le
`tools/list` e popula o seletor de ferramentas do no. Sem sondar, o no fica sem
lista.

A configuracao vai para `<config>/mcp-servers.json`. Um servidor desativado nao
aparece no seletor e recusa chamadas antes mesmo de subir o processo.

### Rotas

| Rota | Efeito |
| --- | --- |
| `GET /flows/mcp/servers` | Servidores configurados e as ferramentas em cache. |
| `POST /flows/mcp/servers` | Cria ou substitui um servidor pelo nome. |
| `DELETE /flows/mcp/servers/{nome}` | Remove. |
| `POST /flows/mcp/servers/{nome}/probe` | Conecta, lista as ferramentas e atualiza o cache. |

### Verificar sem depender da rede

`scripts/mcp-echo-server.mjs` e um servidor MCP minimo por stdio com duas
ferramentas (`eco` e `somar`). Serve para conferir a integracao ponta a ponta
sem baixar pacote nenhum:

```bash
curl -s -X POST http://127.0.0.1:11435/flows/mcp/servers -H 'content-type: application/json' -d '{"name":"eco-teste","command":"node","args":["./scripts/mcp-echo-server.mjs"],"enabled":true}'
```

```bash
curl -s -X POST http://127.0.0.1:11435/flows/mcp/servers/eco-teste/probe
```

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

## Demonstracao gravada

`docs/media/workflow-demo.mp4` mostra um fluxo executando do inicio ao fim na
interface do MLX Pilot: um chamado entra pelo gatilho manual, o no `agent.run`
pede ao modelo local que classifique como urgente ou normal, o `flow.if`
decide a fila pela resposta da IA, e o painel mostra o resultado por no — com
o ramo nao escolhido marcado como pulado e o contador de itens nas conexoes.

A execucao do video e real: o registro traz `qwen2.5:7b-instruct-q4_K_M` via
ollama, 144 tokens, e a classificacao `URGENTE` que motivou o desvio.

Para gravar de novo, com o daemon no ar e a UI servida estaticamente:

```bash
node scripts/static-server.mjs apps/desktop-ui/ui 5610
```

```bash
node scripts/record-workflow-demo.mjs http://127.0.0.1:5610 http://127.0.0.1:11500 "Triagem de chamados com IA"
```

O script usa Playwright, desenha um cursor sintetico (a gravacao nao mostra o
ponteiro real) e salva um `.webm` em `docs/media/`. Para converter:

```bash
ffmpeg -i docs/media/workflow-demo.webm -c:v libx264 -crf 23 -pix_fmt yuv420p -movflags +faststart docs/media/workflow-demo.mp4
```

## Testes

```bash
cargo test -p mlx-flow
```

```bash
npm --prefix apps/desktop-ui run test:e2e:agent-workspace-ui
```
