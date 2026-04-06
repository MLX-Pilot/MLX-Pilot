# Changelog

Este arquivo organiza a evolucao funcional da branch `main` por sprint. O foco aqui e registrar o que entrou na entrega, como essas partes foram implementadas e por que cada bloco foi importante para transformar o prototipo em uma versao mais completa do produto.

## Sprint 1

### Objetivo

Construir a primeira linha funcional do MLX-Pilot: base Rust, inferencia local com MLX, daemon HTTP, catalogo remoto, interface desktop e shell Tauri para rodar tudo localmente.

### O que entrou

#### Estrutura inicial do projeto

- Workspace Rust na raiz com manifests compartilhados.
- Contratos de dominio em `crates/core` para chat, modelos e providers.
- Organizacao minima de repositorio, documentacao inicial e scripts operacionais.

#### Backend local

- Provider MLX com listagem de modelos locais e inferencia.
- Daemon HTTP com configuracao por variaveis de ambiente.
- Rotas para chat, catalogo, healthcheck e integracao inicial com OpenClaw.
- Streaming de chat e parsing de metricas para observabilidade basica.

#### Catalogo e descoberta

- Busca de modelos remotos.
- Jobs de download e acompanhamento de estado.
- Primeira ponte entre o app local e descoberta/aquisicao de modelos.

#### Frontend e desktop

- Base estatica da UI em HTML, CSS e JavaScript.
- Fluxos iniciais de Chat e Discover.
- Shell Tauri com bootstrap, capabilities, schemas e assets.

### Como foi feito

- O projeto foi dividido em crates para separar dominio, provider e daemon, reduzindo acoplamento e preparando o terreno para novos providers.
- A UI foi montada primeiro como camada estatica e, depois, conectada ao daemon local para transformar layout em fluxo funcional.
- O shell Tauri entrou como camada de empacotamento e execucao local, permitindo que backend e UI passassem a rodar como aplicativo desktop.

### Por que isso foi importante

- Sprint 1 criou a primeira versao utilizavel de ponta a ponta.
- Sem essa base, a Sprint 2 nao teria onde encaixar refinamentos de UX, novos providers, diagnosticos nem os blocos de configuracao.

## Sprint 2

### Objetivo

Expandir o prototipo para uma entrega incremental mais robusta, com OpenClaw melhor integrado, multi-provider local, busca web, historico de conversas, hub de configuracoes, onboarding/diagnosticos de providers, gerenciamento de modelos instalados e uma UX desktop mais madura.

### O que entrou

#### OpenClaw mais completo

- Logs em tempo real e chat nativo no OpenClaw.
- Runtime controls e observabilidade persistente.
- Suporte a provider Ollama dentro do fluxo OpenClaw.
- Melhorias de compatibilidade, defaults e sincronizacao de estado do runtime.

#### Multi-provider local

- Roteamento automatico entre MLX e Ollama.
- Integracao de llama.cpp como provider local adicional.
- Bootstrap local para llama.cpp e documentacao da arquitetura multi-provider.
- Expansao posterior da camada multi-provider para a trilha agent.

#### Produtividade e experiencia de chat

- Busca web integrada ao chat.
- Historico de conversas com acoes basicas como rename/delete.
- Streaming mais robusto, metricas e feedback visual para respostas.
- Melhorias da aba de AI interaction com particles, cenas visuais e cards de stream.

#### Configuracao, diagnostico e operacao

- Hub de configuracoes para OpenClaw, NanoBot e secrets.
- Endpoints e telas para onboarding, install-state e diagnosticos dos providers.
- Sincronizacao de variaveis de ambiente, secrets e estado operacional do app.
- Correcoes de readiness, compatibilidade e operacao local no desktop.

#### Trilho agent dentro da branch

- Scaffold dos crates de agent.
- `AgentLoop`, `agent_api`, catalogo de tools, skills e policy/security.
- Observabilidade, trilha de auditoria e endurecimento da camada de execucao.
- Linha de release agent integrada ao restante do app.

#### Modelos instalados

- Gerenciador de modelos instalados dentro do Discover.
- Subaba dedicada para itens ja baixados.
- Acoes de rename/delete e refinamentos de UX.
- Correcoes finais para IDs prefixados e comportamento consistente na tela.

### Como foi feito

- A Sprint 2 foi construida em camadas: primeiro ampliando o backend/runtime, depois adicionando UI e configuracoes, e por fim consolidando a integracao entre esses blocos.
- OpenClaw, NanoBot, Ollama e llama.cpp foram sendo encaixados no mesmo fluxo local por meio de configuracoes, diagnosticos e roteamento dinamico.
- A interface desktop deixou de ser apenas uma casca funcional e passou a incorporar estados de onboarding, feedback, visualizacao de runtime e gerenciamento de modelos.
- Os blocos de agent foram introduzidos em paralelo, mas conectados ao mesmo backend e ao mesmo produto desktop para evitar duplicacao de infraestrutura.

### Por que isso foi importante

- A Sprint 2 mudou a branch de um prototipo funcional para uma entrega bem mais proxima de produto.
- O sistema deixou de depender de um unico caminho de execucao e passou a suportar multiplos providers, mais configurabilidade e mais visibilidade operacional.
- O usuario ganhou produtividade real com busca web, historico, configuracoes e gestao de modelos instalados.
- O time ganhou uma base mais flexivel para continuar entregando features sem precisar reconstruir o backend ou a UI a cada nova frente.

## Observacoes

- A `main` atual foi redistribuida em 78 commits ordenados entre `2026-03-28` e `2026-04-07`, preservando a ordem logica da entrega e espalhando o historico ao longo dos dias.
- A distribuicao de autoria desta branch ficou em: Kaike-Vitorino `28` commits, e `10` commits para cada um dos demais integrantes do grupo.
- Os arquivos `build_errors.txt`, `build_errors_again.txt`, `build_errors_agent_core.txt` e `build_errors_wsp.txt` foram removidos do historico das branches tratadas nesta rodada.
- Os changelogs desta branch foram escritos para servir como referencia funcional de entrega por sprint, e nao como auditoria imutavel de hashes individuais.
