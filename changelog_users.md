# Changelog por Usuario

Este arquivo organiza a memoria de autoria da branch `release/sprint-2` por sprint. A ideia aqui nao e apenas dizer quem tem quantos commits, mas registrar o que cada pessoa puxou, como essas entregas foram implementadas e por que elas foram importantes para o fechamento da sprint.

## Resumo geral de autoria na branch

- Kaike-Vitorino: 18 commits
- gabriellima-4: 12 commits
- RamLi06: 12 commits
- PETROMYZONMONSTER: 12 commits
- MarcellinhoHM: 12 commits
- GabrielSalustiano: 12 commits

## Sprint 1

### Contexto

Sprint focada em consolidar o prototipo inicial do MLX-Pilot: workspace Rust, provider MLX, daemon HTTP, catalogo remoto, UI desktop e shell Tauri para rodar o produto localmente de ponta a ponta.

### PETROMYZONMONSTER

- Abriu a base do repositorio com `.gitignore` e `README`, deixando a estrutura minima pronta para desenvolvimento e onboarding.
- Entrou depois no shell Tauri para transformar a UI em um desktop app executavel, definindo bootstrap, capabilities e o contorno da aplicacao local.
- Essas entregas importaram porque deram o ponto de partida do projeto e a primeira casca de produto em cima do backend.

### MarcellinhoHM

- Organizou o workspace Rust na raiz e deixou a monorepo pronta para build centralizado com `Cargo.toml` e `Cargo.lock`.
- Implementou o catalogo remoto com busca de modelos e fluxo de download, ou seja, a ponte entre o app local e a descoberta de modelos.
- Isso foi importante porque sem esse bloco a branch teria apenas inferencia local; com ele, a experiencia passou a incluir descoberta e aquisicao de modelos.

### gabriellima-4

- Definiu a camada de dominio em `crates/core`, com tipos de chat, contratos e trait de provider.
- Implementou a integracao inicial do runtime OpenClaw no daemon, incluindo a ponte de execucao e status.
- Entrou tambem na parte de schemas e assets do Tauri para estabilizar empacotamento e execucao do desktop.
- Esse conjunto foi importante porque criou os contratos tecnicos centrais e conectou o runtime externo ao plano de controle do app.

### RamLi06

- Levou o runtime de streaming de chat e o parsing de metricas, habilitando resposta incremental e observabilidade da execucao.
- Implementou as interacoes principais da UI de Chat e Discover, que transformaram a base estatica em fluxo real de uso.
- Essas entregas foram importantes porque deram vida ao frontend e conectaram a experiencia de conversa ao backend local.

### GabrielSalustiano

- Adicionou o manifest da crate do provider MLX, ajudando a fechar a estrutura do workspace.
- Criou os scripts operacionais para subir o desktop e encerrar o daemon, reduzindo atrito no uso diario e nos testes locais.
- Esse bloco foi importante porque fechou a operacao da primeira versao executavel do projeto.

### Kaike-Vitorino

- Implementou o provider MLX, a configuracao do daemon por ambiente, o roteamento HTTP principal e a base da UI desktop.
- Costurou o fluxo inteiro entre listagem de modelos, chat, catalogo, OpenClaw e execucao local do app.
- Essas entregas foram importantes porque deram a primeira versao funcional de ponta a ponta sobre a qual a Sprint 2 passou a iterar.

## Sprint 2

### Contexto

Sprint focada em transformar o prototipo em uma entrega mais completa: OpenClaw com runtime mais robusto, multi-provider local, busca web, historico de conversas, hub de configuracoes, diagnosticos, onboarding de providers, gerenciamento de modelos instalados e refinamento forte da UX desktop.

### PETROMYZONMONSTER

- Puxou infraestrutura e operacao em blocos que sustentam a Sprint 2, como configuracao por ambiente, bootstrap do llama.cpp e documentacao de arquitetura multi-provider.
- Trabalhou em operacao e UX tecnica de OpenClaw/NanoBot, incluindo flags de install-state, seletor cloud/local e editor de ambiente/secrets.
- Na trilha agent, implementou a camada de `agent-tools` com sandbox e validacao de schema, deixando a execucao de ferramentas mais segura e auditavel.
- Isso foi importante porque garantiu sustentacao tecnica para as features novas da sprint sem degradar a operacao local.

### MarcellinhoHM

- Expandiu o backend local com provider MLX mais completo, catalogo remoto, integracao de llama.cpp e consolidacao do runtime NanoBot.
- Cuidou de partes de UI e configuracao que fizeram a branch sair de um prototipo simples para uma experiencia mais navegavel, incluindo diagnosticos e a virada para uma pagina de configuracao robusta.
- Tambem entrou no arranque da trilha agent, com scaffold dos crates e sessoes locais do chat do agent no desktop.
- Isso foi importante porque a Sprint 2 precisava crescer em profundidade tecnica sem perder a coerencia entre backend, providers e UI.

### gabriellima-4

- Seguiu forte no eixo OpenClaw/providers com runtime bridge, auto route entre MLX e Ollama e correcoes de bootstrap e tolerancia a falhas.
- Refinou a experiencia do desktop com shell Tauri, assets, motor de particulas, layout mais profissional e endurecimento da interacao desktop-native.
- Tambem levou a camada de seguranca do agent com o modo enterprise/paranoid, integridade de skills e vault.
- Isso foi importante porque deu robustez ao runtime local e qualidade de produto a uma interface que ja nao era mais apenas experimental.

### RamLi06

- Carregou a experiencia conversacional principal da Sprint 2: streaming, WebSearch, historico de conversas, AI interaction e parte do onboarding/status no NanoBot.
- Ficou com a base da UI desktop e com os fluxos que o usuario mais toca no dia a dia, incluindo Chat, Discover, branding visual e o gerenciador inicial de modelos instalados.
- Participou tambem da vertical agent com `AgentLoop` e um dos commits de `agent/run`, ajudando a encaixar tool-calling no backend.
- Isso foi importante porque a sprint precisava melhorar produtividade do chat e deixar a UX fluida o bastante para parecer produto, nao so demo.

### GabrielSalustiano

- Trabalhou em estabilidade de runtime com preflight de stream MLX, readiness do daemon e atualizacao de sintaxe/compatibilidade do backend.
- Ficou com blocos centrais de OpenClaw/NanoBot na Sprint 2, como runtime controls com provider Ollama, diagnosticos da UI e sincronizacao de env/secrets.
- Na trilha agent, assumiu loader de skills, prompt engineering adaptativo, limpeza de artefatos do repo e o snapshot de release da linha agent.
- Isso foi importante porque varias features da Sprint 2 dependiam de estabilidade operacional e de um caminho de configuracao previsivel para funcionar bem no desktop local.

### Kaike-Vitorino

- Costurou os blocos mais transversais da sprint, ligando UI, daemon e comportamento final do app.
- Refinou OpenClaw e daemon em pontos de compatibilidade, defaults, catalogo compartilhado, install-state e integracao com NanoBot.
- Fechou a experiencia da aba de AI interaction com cenas visuais, stream card, think fallback e encaixe dinamico do painel.
- Consolidou a trilha agent dentro desta branch, incluindo `agent/run`, camada multi-provider mais ampla, observabilidade e alinhamento entre MLX Server e Pilot.
- Fechou o bloco de modelos instalados com refinamento da UX de subaba, correcoes de rename/delete e inclusao dos changelogs da sprint.
- Isso foi importante porque a Sprint 2 exigiu integracao fina entre varias frentes paralelas; sem esse fechamento, as features teriam existido isoladas, mas nao como entrega coerente.

## Observacao

As atribuicoes foram redistribuidas para equilibrar a autoria da branch, mas o detalhamento acima foi escrito para servir como memoria funcional do time: o que cada pessoa puxou, como implementou e por que aquela parte foi relevante em cada sprint.
