# RFC da ABI 1 de providers de inferência

Um provider carregável oferece modelos e responde a conversas. O daemon continua responsável por escolher o modelo, autorizar o envio do histórico, executar ferramentas e apresentar resultados. Esta proposta define a fronteira entre ambos. A prova de carregamento roda no teste `provider_abi`. O aplicativo distribuído atualmente roteia conversas pelos providers nativos.

## Descoberta e configuração

O SDK de autoria poderá empacotar `manifest` e `wasm_base64` no mesmo envelope de transporte dos plugins atuais. O manifesto de um provider declara `kind: "inference_provider"`, `api_version: 2` e `provider_abi: 1`. O primeiro número identifica o formato do pacote e o segundo identifica os exports e mensagens de inferência. O manifesto de ferramentas usa `api_version: 1`. O instalador atual rejeita a versão 2, inclusive quando recebe um pacote misto com ferramentas. [Manifesto completo da prova](../crates/plugin-runtime/tests/fixtures/provider-v1.json).

`id`, `name`, `version`, `permissions`, serviços e credenciais seguem as restrições do manifesto atual. `capabilities` declara os booleanos obrigatórios `streaming` e `tool_calls`. `config_schema` descreve um objeto JSON com chaves públicas, tipos e valores aceitos. O host valida o schema localmente, com os limites de tamanho e a proibição de referências externas do SDK existente. Os destinos de rede e referências de credencial ficam nas declarações de serviço. A configuração preenchida e os grants pertencem ao par workspace/plugin.

Na instalação, o host valida metadados, tamanho, hash do módulo e versões suportadas antes de instanciar o WASM. Confere a assinatura dos exports, inclusive os exigidos pelas capacidades anunciadas. Uma atualização é preparada antes de substituir a versão ativa. Concessões adicionais e mudança de destino exigem revisão. A interface apresenta o nome do plugin, os dados enviados e o destino antes da ativação.

Ao ativar, o host valida a configuração e chama `provider_list_models`. Cada `ModelDescriptor` usa um ID local único e um `provider` igual a `plugin.<id>`; o `path` é um identificador opaco. No catálogo, o host qualifica o modelo como `plugin.<id>::<model_id>` e rejeita colisões. O SDK deve passar o ID local ao guest em cada requisição. O host publica o catálogo somente depois de validar a resposta completa. Falha de descoberta mantém o provider indisponível e permite uma nova tentativa após corrigir a configuração.

Reconfigurar ou atualizar abre uma nova geração de instâncias. Desativação, desinstalação e revogação retiram o catálogo, cancelam as chamadas afetadas e descartam suas instâncias. A requisição captura a versão e o workspace. O broker verifica os grants vigentes a cada operação. O guest deve funcionar com uma instância nova por chamada e tratar seu estado de memória como transitório.

## Exports e mensagens

Cada export Extism recebe bytes JSON UTF-8 pela entrada do PDK, escreve um objeto JSON na saída e retorna `i32` zero. O retorno de negócio tem exatamente uma chave `result` ou `error`. Um trap ou status Extism diferente de zero é falha do runtime. Campos opcionais novos em resultados podem ser ignorados. Uma mudança que altera um tipo, remove um campo obrigatório ou muda a semântica de um evento exige nova versão de `provider_abi`.

| Export | Entrada JSON | Resultado |
| --- | --- | --- |
| `provider_list_models` | `{"config":{}}` | Array de `ModelDescriptor`. Catálogo vazio é válido. |
| `provider_chat` | `{"config":{},"request":ChatRequest,"tools":[FunctionDef]}` | `ChatResponse` completa, com `message.role` igual a `assistant`. |
| `provider_chat_stream` | Mesma entrada de `provider_chat` | Deltas durante a execução e uma `ChatResponse` final completa. Obrigatório quando `streaming` é `true`. |

`ChatRequest`, `ChatResponse`, `ChatMessage`, `FunctionDef`, `TokenUsage` e `GenerationOptions` usam os campos serializados do core. `request` leva `model_id`, `messages` e `options`. O guest preserva a ordem dos turnos e mantém `reasoning` separado de `content`. O host verifica o modelo e o provider de cada resposta, valida os tipos e contabiliza a latência observada. [Tipos do core](../crates/core/src/lib.rs).

Um `tools` não vazio exige capacidade `tool_calls`. O modelo pode solicitar chamadas em `message.tool_calls`, cada qual com `id` único por turno, `name` de uma função oferecida e `arguments` contendo um objeto JSON serializado. O host valida os argumentos contra o schema, aplica a política e as aprovações da sessão e só então executa a função. O próximo turno pode incluir `role: "tool"` e `tool_call_id` correspondente. `raw_output`, quando presente, é dado do modelo e recebe os mesmos controles de exposição e redação do texto.

Durante `provider_chat_stream`, o host liga a importação Extism `mlx_provider_emit` a **uma chamada**. A assinatura é `(param i64) (result i32)`, com o ponteiro para um bloco UTF-8 alocado pelo PDK. Um evento contém `seq` consecutivo a partir de zero, `kind` igual a `content_delta` ou `reasoning_delta`, e `text`. O host aceita o evento com retorno zero e aplica contrapressão antes de aceitar outro. Retorno diferente de zero encerra a produção de deltas. A resposta final agrega os textos emitidos e traz uso e chamadas de ferramenta completos. O consumidor substitui o texto parcial pelo texto final. O daemon emite um único terminal ao cliente após validar o resultado ou o erro. Um salto de sequência, evento desconhecido, limite excedido ou chamada de ferramenta incompleta produz `invalid_response`.

## Cancelamento, erros e permissões

O host mantém um identificador por requisição, um prazo monotônico e um handle de cancelamento do Extism. Ao cancelar, interrompe o guest e as operações do broker daquela chamada, descarta deltas tardios e libera a instância. O primeiro terminal vence a corrida entre conclusão e cancelamento. Repetir o cancelamento é idempotente. A aplicação pode oferecer repetição antes da primeira saída visível. Após um delta, encerra o turno com o erro para que o usuário decida sobre uma nova tentativa.

Um erro de negócio usa `{"error":{"code":"model_not_found","message":"Modelo indisponível"}}`. O host converte `invalid_request`, `model_not_found`, `unavailable` e `timeout` nos erros correspondentes de `ModelProvider`. `unsupported_capability` orienta a seleção de outro modelo e `permission_denied` leva à configuração do workspace. `cancelled` encerra a requisição sem fallback. O adaptador deve carregar esses três estados até a camada HTTP/agente, pois o enum atual não os distingue. Traps, JSON incorreto, violação de contrato e códigos desconhecidos resultam em erro local `invalid_response`. O log registra provider, modelo, duração e código, com mensagens redigidas, sem incluir prompts, segredos nem cabeçalhos brutos.

O provider requer `compute`. Para inferência remota, o host exige autorização explícita do workspace para enviar histórico ao serviço e aplica `network:request` e, quando necessário, `credentials:use`. O guest referencia o serviço pelo ID. O broker valida destino, método, limites e injeta a credencial. `RuntimeProviderConfig.api_key`, cabeçalhos arbitrários, shell, caminhos locais e handles de modelo nativos ficam no host. Uma instalação de provider concede apenas capacidades de inferência e os serviços aprovados. A prova usa WASI desligado e nenhuma importação de usuário. No adaptador futuro, somente o broker restrito e, quando cabível, `mlx_provider_emit` poderão ser ligados.

## Integração com o roteamento atual

| Caminho atual | Consequência para o adaptador |
| --- | --- |
| MLX executa o runtime local; a rota de streaming usa `spawn_chat_stream`. | Manter o acesso ao processo e aos pesos no backend nativo. O provider WASM precisa de uma capacidade explícita do host para usar um motor local. |
| Ollama usa HTTP, oferece `chat_with_tools_with_runtime` e o daemon lê o fluxo com `begin_chat_stream`. | Mapear definições de ferramenta e deltas preservando os limites do broker. O broker de plugins atual expõe respostas HTTP completas e não transmite um fluxo HTTP incremental. |
| llama.cpp chama o binário local. O daemon adapta a resposta completa em `spawn_provider_compat_stream`. | Apresentar o resultado final quando o provider declara `streaming: false`. A capacidade de streaming descreve deltas produzidos pelo guest. |
| O agente resolve IDs conhecidos em `AgentProviderRegistry`; o core exige `provider_id() -> &'static str`. | Introduzir registro por workspace e identidade com vida útil adequada para IDs carregados antes de ligar a ABI ao roteamento. Resolver a mesma identidade no chat e no agente. |
| `Package::invoke` valida uma ferramenta e cria uma instância por invocação. O SDK 2 publica ferramentas e frontends. | Acrescentar um adaptador próprio com limites de tokens, bytes, combustível, tempo e operações concorrentes. Usar os limites atuais do runtime como teto. SDK e instalador precisarão distinguir os dois tipos de pacote. |

[Daemon e rotas de chat](../crates/daemon/src/lib.rs) · [Registro do agente](../crates/daemon/src/agent_api.rs) · [Loader de ferramentas](../crates/plugin-runtime/src/lib.rs) · [Plataforma de plugins](plugin-platform.md).

## Executar a prova

```sh
cargo test --locked -p mlx-plugin-runtime --test provider_abi
```

O teste compila o guest WAT em WASM, carrega o módulo com Extism, verifica os exports e lê o catálogo. Envia um `ChatRequest` do core e decodifica a resposta fixa `pong` como `ChatResponse`. Também verifica configuração inválida, versão de ABI incompatível, export obrigatório ausente e oferta de streaming sem export. A fixture usa identificadores e resultados determinísticos para que a prova rode offline. Streaming, cancelamento e integração com o registro do daemon são contratos para a implementação posterior. [Teste executável](../crates/plugin-runtime/tests/provider_abi.rs) · [Guest da prova](../crates/plugin-runtime/tests/fixtures/provider-v1.wat).
