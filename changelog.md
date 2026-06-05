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

## Sprint 3

### Objetivo

Fechar a camada de observabilidade e runtime local mais avancado: fallback AIRLLM para modelos pesados, console de observabilidade do agente, dashboard do daemon e sincronizacao tecnica da `agent-config-ui` em `main_2`.

### O que entrou

#### Fallback AIRLLM

- Ponte Rust/Python para AIRLLM em modelos de alta memoria.
- Flags de uso/obrigatoriedade do AIRLLM no stream do daemon e nos metadados de chat.
- Controles de backend/device, safe mode, recuperacao de OOM e diagnosticos em tempo real.
- Badge e metricas na UI para indicar quando AIRLLM foi requerido ou usado.

#### Observabilidade do agente e daemon

- Console nativo de agente com feed de auditoria, progresso e eventos.
- Dashboard/estado operacional para runtime, tools, skills e readiness de modelos.
- Persistencia de logs/sync operacional para telemetria.
- Smoke coverage inicial para workspace de agente.

#### Integracao seletiva da `agent-config-ui`

- Aplicacao do estado final de codigo da `agent-config-ui` sobre `main_2`.
- Preservacao de `changelog.md` e `changelog_users.md` no padrao da `main`.
- Manutencao do historico antigo da `main`, sem squash geral e sem substituir a branch por uma arvore divergente.
- Nao reintroducao do `CHANGELOG.md` legado nem de artefatos de build removidos anteriormente.

### Como foi feito

- Como `origin/main` e `origin/agent-config-ui` nao tinham ancestral comum, a integracao foi feita por diferenca de arvore, nao por merge cego.
- O resultado da `agent-config-ui` foi aplicado sobre `main_2`, excluindo os changelogs para preservar a memoria documental da `main`.
- Os commits da `agent-config-ui` que ja apareciam como patches equivalentes na `main` foram tratados como representados e nao foram importados de novo.
- As novas alteracoes foram organizadas em commits por area, com autoria redistribuida no mesmo estilo da `main`.

### Por que isso foi importante

- A Sprint 3 melhorou a capacidade do app de operar modelos pesados e de mostrar o que o agente/daemon estao fazendo.
- `main_2` fica com o codigo mais atual sem destruir o trabalho de curadoria historica ja feito na `main`.
- A branch passa a representar a sprint mais recente com autoria visivel para todos os desenvolvedores, mas mantendo Kaike-Vitorino como principal responsavel pela integracao.

## Sprint 4

### Objetivo

Consolidar o agente local como uma camada funcional de automacao: agent core, ferramentas sandboxed, skills, API integrada ao daemon, politicas, secrets vault, memoria local e hardening de seguranca.

### O que entrou

#### Runtime nativo de agentes

- Fundacoes do runtime Hermes/native agent.
- Novos modulos de capacidade, memoria, state store, recall de sessoes e catalogo de tools.
- Endurecimento de execucao local, runtime doctor, backend health checks e remocao dos artefatos legados de OpenClaw.
- Ajustes nos providers MLX, Ollama, llama.cpp e HTTP para roteamento local mais uniforme.

#### Tools, skills e operacao

- Skills compativeis com ecossistemas Claude/Codex.
- Ferramentas locais de busca, grep/glob, checkpoints, scheduler e execucao com politica auditavel.
- Smoke tests para skills, canais e workspace de agente.
- Carregamento de `SKILL.md`, metadados de compatibilidade, limites e elegibilidade de skills.
- Contratos de execucao local com validacao de schema e bloqueios para paths/comandos perigosos.

#### API, memoria e contexto

- Endpoints dedicados no daemon para execucao do agente e acompanhamento de estado.
- Sessao ativa, memoria local, state store e recall para continuidade de interacoes.
- Catalogo de tools, policy resolver e aplicacao de overrides por sessao.
- Inventario de capacidades respondido pelo runtime, incluindo readiness de tools e modelos.

#### Politicas, secrets e hardening

- Politicas de filesystem, processo, rede, allow/deny lists e perfis de tools.
- Vault local para secrets e uso controlado de variaveis sensiveis.
- Modo enterprise/paranoid herdado da linha agent e reforcado por checks de integridade.
- Hardening de chat/runtime com sanitizacao, isolamento e release-gate de seguranca.

#### Interface de agentes e configuracao

- Workspace desktop de agente com sidebar, console nativo, progresso em tempo real e renderizacao rica.
- Fluxos de configuracao para modelos, tools, skills, politicas e readiness do agente.
- Reorganizacao do control plane, tela compacta de agente e nova experiencia visual para o app desktop.
- Inclusao do frontend orbital/desktop atualizado usado como base visual da branch mais recente.

### Como foi feito

- A camada de agent core foi fechada em cima dos crates `agent-core`, `agent-tools` e `agent-skills`.
- O daemon passou a atuar como fronteira entre UI, runtime local, providers e politicas de execucao.
- A UI recebeu controles para transformar configuracao de tools/skills/modelos em fluxo operacional.
- O hardening foi tratado junto da execucao, nao como documento separado: policies, secrets, sandbox e smoke tests passaram a fazer parte do caminho normal.

### Por que isso foi importante

- A Sprint 4 transforma o agente de uma promessa arquitetural em uma camada local usavel e auditavel.
- O produto passa a ter memoria, contexto, ferramentas e politicas suficientes para automacao multi-etapa com risco controlado.
- O backend e a UI agora compartilham uma mesma visao de capacidades, readiness e configuracao.

## Sprint 5 parcial

### Objetivo

Iniciar a evolucao pos-MVP: canais oficiais, release gate final, empacotamento multiplataforma, extensibilidade por plugins e fundacoes para automacao autonoma/workflows.

### O que entrou

#### Canais e integracoes externas

- Runtime multi-account para canais.
- Control plane oficial para canais no desktop.
- Ponte WhatsApp local com renderer de QR e documentacao de protocolo/transporte.
- Ajustes de UI para controle de canais e persistencia operacional.

#### Release gate e smoke tests finais

- Scripts de release gate para validacao final, migracao, rollback, carga, crash recovery e auditoria.
- Relatorio final de release gate e evidencias tecnicas em `docs/release-gate-report.*`.
- Smoke tests para control plane, workspace de agente, canais e skills.
- Checklist de dry run para preparar revisao de release.

#### Empacotamento multiplataforma

- Assets e icones multiplataforma no frontend/Tauri atualizado.
- Estrutura de app desktop adicional em `frontend-new` para validar empacotamento e distribuicao.
- Ajustes de configuracao Tauri e lockfiles para builds desktop mais previsiveis.

#### Extensibilidade e automacao futura

- Modulo inicial de plugins no daemon e registro local de estado.
- Adaptador unificado de providers em TypeScript como fundacao para providers/tools externos.
- Runtime com estado, limites e execucao por tools que prepara o terreno para ciclo OODA.
- A integracao n8n embedded segue como backlog aberto, sem implementacao completa nesta branch.

#### Documentacao tecnica e pesquisa

- `README_DEV.md` para orientacao de desenvolvimento.
- Revisoes de arquitetura, runtime hardening, runtime doctor e assimilacao de ecossistemas locais.
- Documentos de integracao Hermes, matriz de paridade de tools e relatorios de release gate.
- Adaptador TypeScript unificado para providers externos.

### Como foi feito

- As frentes de canais, release gate e empacotamento foram trazidas como blocos ja presentes na `agent-config-ui`.
- Os itens de S5 ainda abertos no backlog foram documentados como fundacoes ou inicio de trilha, nao como entrega completa.
- O control plane e os smoke tests ajudam a transformar essas trilhas em algo revisavel antes da substituicao da `main`.

### Por que isso foi importante

- A Sprint 5 parcial mostra que a branch ja passou do agent core e comecou a preparar produto para integracoes externas, release e distribuicao.
- O changelog separa claramente o que entrou de fato do que ainda permanece como backlog aberto.
- Os changelogs continuam servindo como memoria funcional da sprint, agora incluindo a ponte entre a `main` organizada, a `agent-config-ui` mais atualizada e o kanban do projeto.

## Observacoes

- A `main` original foi redistribuida em 78 commits ordenados entre `2026-03-28` e `2026-04-07`, preservando a ordem logica da entrega e espalhando o historico ao longo dos dias.
- A `main_2` preserva esses 78 commits e adiciona 7 commits de integracao/documentacao cobrindo Sprint 3, Sprint 4 e parte da Sprint 5.
- A distribuicao de autoria da `main_2` ficou em: Kaike-Vitorino `30` commits, e `11` commits para cada um dos demais integrantes do grupo.
- Os arquivos `build_errors.txt`, `build_errors_again.txt`, `build_errors_agent_core.txt` e `build_errors_wsp.txt` foram removidos do historico das branches tratadas nesta rodada.
- Os changelogs desta branch foram escritos para servir como referencia funcional de entrega por sprint, e nao como auditoria imutavel de hashes individuais.
