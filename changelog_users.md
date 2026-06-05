# Changelog por Usuario

Este arquivo organiza a memoria de autoria da branch `main` por sprint. A ideia aqui nao e apenas dizer quem tem quantos commits, mas registrar o que cada pessoa puxou, como essas entregas foram implementadas e por que elas foram importantes para o fechamento da sprint.

## Resumo geral de autoria na branch

- Kaike-Vitorino: 30 commits
- gabriellima-4: 11 commits
- RamLi06: 11 commits
- PETROMYZONMONSTER: 11 commits
- MarcellinhoHM: 11 commits
- GabrielSalustiano: 11 commits

## Sprint 1

### Contexto

Sprint focada em consolidar o prototipo inicial do MLX-Pilot: workspace Rust, provider MLX, daemon HTTP, catalogo remoto, UI desktop e shell Tauri para rodar o produto localmente de ponta a ponta.

### PETROMYZONMONSTER

- Abriu a base do repositorio com `.gitignore` e `README`, deixando a estrutura minima pronta para desenvolvimento e onboarding.
- Entrou depois no shell Tauri para transformar a UI em um desktop app executavel, definindo bootstrap, capabilities e o contorno da aplicacao local.
- Tambem ficou associado a blocos de observabilidade operacional no OpenClaw e de consolidacao de configuracao/local provider, que aparecem mais a frente na evolucao da `main`.
- Essas entregas importaram porque deram o ponto de partida do projeto e a primeira casca de produto em cima do backend.

### MarcellinhoHM

- Organizou o workspace Rust na raiz e deixou a monorepo pronta para build centralizado com `Cargo.toml` e `Cargo.lock`.
- Implementou o catalogo remoto com busca de modelos e fluxo de download, ou seja, a ponte entre o app local e a descoberta de modelos.
- Na fotografia final da `main`, tambem aparece puxando partes de provider MLX, daemon HTTP e consolidacao da camada agent/UI, ajudando a fechar o miolo tecnico da branch.
- Isso foi importante porque sem esse bloco a branch teria apenas inferencia local; com ele, a experiencia passou a incluir descoberta e aquisicao de modelos.

### gabriellima-4

- Definiu a camada de dominio em `crates/core`, com tipos de chat, contratos e trait de provider.
- Implementou a integracao inicial do runtime OpenClaw no daemon, incluindo a ponte de execucao e status.
- Entrou tambem na parte de schemas e assets do Tauri para estabilizar empacotamento e execucao do desktop.
- No estado final da `main`, continua concentrando blocos de runtime OpenClaw/providers, acabamento do desktop e parte da trilha de seguranca do agent.
- Esse conjunto foi importante porque criou os contratos tecnicos centrais e conectou o runtime externo ao plano de controle do app.

### RamLi06

- Levou o runtime de streaming de chat e o parsing de metricas, habilitando resposta incremental e observabilidade da execucao.
- Implementou as interacoes principais da UI de Chat e Discover, que transformaram a base estatica em fluxo real de uso.
- Na `main` final, tambem concentra boa parte da experiencia conversacional, branding visual e os blocos iniciais do gerenciador de modelos instalados.
- Essas entregas foram importantes porque deram vida ao frontend e conectaram a experiencia de conversa ao backend local.

### GabrielSalustiano

- Adicionou o manifest da crate do provider MLX, ajudando a fechar a estrutura do workspace.
- Criou os scripts operacionais para subir o desktop e encerrar o daemon, reduzindo atrito no uso diario e nos testes locais.
- No recorte final da `main`, tambem aparece ligado a estabilidade de runtime, diagnosticos, sync de ambiente e partes da linha agent/release.
- Esse bloco foi importante porque fechou a operacao da primeira versao executavel do projeto.

### Kaike-Vitorino

- Implementou o provider MLX, a configuracao do daemon por ambiente, o roteamento HTTP principal e a base da UI desktop.
- Costurou o fluxo inteiro entre listagem de modelos, chat, catalogo, OpenClaw e execucao local do app.
- Na `main` atual, ficou com a maior parte dos commits de integracao e fechamento, especialmente nos pontos em que varias frentes precisaram ser consolidadas num fluxo unico de produto.
- Essas entregas foram importantes porque deram a primeira versao funcional de ponta a ponta sobre a qual a Sprint 2 passou a iterar.

## Sprint 2

### Contexto

Sprint focada em transformar o prototipo em uma entrega mais completa: OpenClaw com runtime mais robusto, multi-provider local, busca web, historico de conversas, hub de configuracoes, diagnosticos, onboarding de providers, gerenciamento de modelos instalados e refinamento forte da UX desktop.

### PETROMYZONMONSTER

- Puxou infraestrutura e operacao em blocos que sustentam a Sprint 2, como configuracao por ambiente, bootstrap do llama.cpp e documentacao de arquitetura multi-provider.
- Trabalhou em operacao e UX tecnica de OpenClaw/NanoBot, incluindo flags de install-state, seletor cloud/local e editor de ambiente/secrets.
- Na trilha agent, implementou a camada de `agent-tools` com sandbox e validacao de schema, deixando a execucao de ferramentas mais segura e auditavel.
- Na `main` final, os commits atribuidos a ele se concentram em fundacao de repositorio, shell desktop, operacao de providers locais e parte do acabamento de configuracao/instalacao.
- Isso foi importante porque garantiu sustentacao tecnica para as features novas da sprint sem degradar a operacao local.

### MarcellinhoHM

- Expandiu o backend local com provider MLX mais completo, catalogo remoto, integracao de llama.cpp e consolidacao do runtime NanoBot.
- Cuidou de partes de UI e configuracao que fizeram a branch sair de um prototipo simples para uma experiencia mais navegavel, incluindo diagnosticos e a virada para uma pagina de configuracao robusta.
- Tambem entrou no arranque da trilha agent, com scaffold dos crates e sessoes locais do chat do agent no desktop.
- Na `main` consolidada, os commits dele ficam especialmente associados a provider/local runtime, catalogo, daemon principal e parte da subida de features agent/UI.
- Isso foi importante porque a Sprint 2 precisava crescer em profundidade tecnica sem perder a coerencia entre backend, providers e UI.

### gabriellima-4

- Seguiu forte no eixo OpenClaw/providers com runtime bridge, auto route entre MLX e Ollama e correcoes de bootstrap e tolerancia a falhas.
- Refinou a experiencia do desktop com shell Tauri, assets, motor de particulas, layout mais profissional e endurecimento da interacao desktop-native.
- Tambem levou a camada de seguranca do agent com o modo enterprise/paranoid, integridade de skills e vault.
- Na `main` atual, os commits associados a ele ajudam a puxar a espinha dorsal de OpenClaw e a estabilizacao do app como produto desktop.
- Isso foi importante porque deu robustez ao runtime local e qualidade de produto a uma interface que ja nao era mais apenas experimental.

### RamLi06

- Carregou a experiencia conversacional principal da Sprint 2: streaming, WebSearch, historico de conversas, AI interaction e parte do onboarding/status no NanoBot.
- Ficou com a base da UI desktop e com os fluxos que o usuario mais toca no dia a dia, incluindo Chat, Discover, branding visual e o gerenciador inicial de modelos instalados.
- Participou tambem da vertical agent com `AgentLoop` e um dos commits de `agent/run`, ajudando a encaixar tool-calling no backend.
- Na `main` final, os commits dele aparecem muito ligados aos blocos de chat, discover, UI e usabilidade de modelos instalados.
- Isso foi importante porque a sprint precisava melhorar produtividade do chat e deixar a UX fluida o bastante para parecer produto, nao so demo.

### GabrielSalustiano

- Trabalhou em estabilidade de runtime com preflight de stream MLX, readiness do daemon e atualizacao de sintaxe/compatibilidade do backend.
- Ficou com blocos centrais de OpenClaw/NanoBot na Sprint 2, como runtime controls com provider Ollama, diagnosticos da UI e sincronizacao de env/secrets.
- Na trilha agent, assumiu loader de skills, prompt engineering adaptativo, limpeza de artefatos do repo e o snapshot de release da linha agent.
- Na `main` consolidada, isso o deixa associado principalmente aos pontos de configuracao, release e estabilidade operacional do produto.
- Isso foi importante porque varias features da Sprint 2 dependiam de estabilidade operacional e de um caminho de configuracao previsivel para funcionar bem no desktop local.

### Kaike-Vitorino

- Costurou os blocos mais transversais da sprint, ligando UI, daemon e comportamento final do app.
- Refinou OpenClaw e daemon em pontos de compatibilidade, defaults, catalogo compartilhado, install-state e integracao com NanoBot.
- Fechou a experiencia da aba de AI interaction com cenas visuais, stream card, think fallback e encaixe dinamico do painel.
- Consolidou a trilha agent dentro desta branch, incluindo `agent/run`, camada multi-provider mais ampla, observabilidade e alinhamento entre MLX Server e Pilot.
- Fechou o bloco de modelos instalados com refinamento da UX de subaba, correcoes de rename/delete e inclusao dos changelogs da sprint.
- Na `main` final, isso se traduz em maior volume de commits de integracao e fechamento, mantendo boa parte da branch com voce sem concentrar tudo exclusivamente em um autor.
- Isso foi importante porque a Sprint 2 exigiu integracao fina entre varias frentes paralelas; sem esse fechamento, as features teriam existido isoladas, mas nao como entrega coerente.

## Sprint 3

### Contexto

Sprint focada em fallback AIRLLM, observabilidade do agente, dashboard do daemon e sincronizacao tecnica da `agent-config-ui` em `main_2`, sem apagar o historico curado da `main`.

### PETROMYZONMONSTER

- Ficou associado a parte da validacao tecnica do agente, especialmente smoke coverage e documentacao de paridade de tools.
- Essa atribuicao preserva o padrao da `main`, em que PETROMYZONMONSTER ja estava ligado a tools, seguranca e documentacao tecnica de sustentacao.

### MarcellinhoHM

- Ficou com a documentacao tecnica de runtime e pesquisa que explica o salto da branch atualizada.
- Essa entrega foi importante para registrar a arquitetura atualizada da `agent-config-ui` sem transformar a integracao em uma mudanca sem memoria tecnica.

### gabriellima-4

- Assumiu parte da UI de observabilidade e configuracao do workspace de agente, incluindo shell desktop atualizado e experiencia visual mais recente.
- O bloco cobre sidebar, console nativo, fluxo compacto e readiness de modelos/tools.
- Essa atribuicao segue o historico da `main`, onde gabriellima-4 ja aparece em UI/config e integracao de telas de configuracao.

### RamLi06

- Ficou com smoke coverage do workspace de agente e validacoes que ajudam a provar a integracao da Sprint 3.
- Essa contribuicao deixa visivel a frente de QA/validacao, essencial para transformar a integracao em branch revisavel.

### GabrielSalustiano

- Ficou associado aos ajustes de integracao operacional entre daemon, runtime e UI durante a sincronizacao da branch.
- Isso mantem GabrielSalustiano no eixo de estabilidade/integracao em que ele ja aparece na `main`.

### Kaike-Vitorino

- Continuou com a maior parte da responsabilidade de integracao, criando `main_2`, preservando os commits da `main` e aplicando seletivamente o estado final da `agent-config-ui`.
- Ficou com o commit estrutural de runtime/backend que inclui AIRLLM, runtime doctor, providers locais e remocao de artefatos legados.
- Tambem fechou os changelogs, registrando a estrategia usada, a nao duplicacao de patches equivalentes e a nova distribuicao de autoria.
- Essa distribuicao mantem Kaike-Vitorino como principal responsavel tecnica pela consolidacao, mas deixa a participacao minima dos demais desenvolvedores visivel no historico.

## Sprint 4

### Contexto

Sprint focada em consolidar a base de agente local: agent core, ferramentas sandboxed, skills, API do agente no daemon, politicas, secrets vault, memoria local, contexto e hardening de seguranca.

### PETROMYZONMONSTER

- Ficou associado ao bloco de `agent-tools` e skills, incluindo carregamento de skills, ferramentas locais de busca, checkpoints, scheduler e smoke de validacao.
- Tambem aparece nos documentos de paridade de tools e no fechamento da camada operacional de execucao auditavel.
- Essa atribuicao preserva o padrao da `main`, em que PETROMYZONMONSTER ja estava ligado a tools, seguranca e documentacao tecnica de sustentacao.

### MarcellinhoHM

- Ficou com a documentacao tecnica de runtime, pesquisa Hermes, hardening, runtime doctor e revisao local do ecossistema.
- Tambem ficou associado ao adaptador TypeScript unificado de providers, alinhado ao papel que ja tinha na `main` em provider/local runtime.
- Essa entrega foi importante para registrar a arquitetura da Sprint 4 e explicar como o agente local passou a se apoiar em providers e runtime mais extensiveis.

### gabriellima-4

- Assumiu o commit de UI/configuracao do workspace de agente, incluindo control plane, agent skills UI, configuracao de modelos/tools e o frontend orbital.
- O bloco cobre a experiencia visual da Sprint 4, com telas para transformar agent core, skills e policies em fluxo operavel.
- Isso foi importante porque a Sprint 4 nao era apenas backend; o usuario precisava enxergar e operar as capacidades do agente.

### RamLi06

- Ficou associado a smoke tests do desktop e validacoes que cobrem workspace, skills e readiness.
- Essa contribuicao amarra a entrega da Sprint 4 a evidencias tecnicas, evitando que agent core/tools/skills ficassem apenas como codigo nao exercitado.

### GabrielSalustiano

- Ficou ligado aos pontos de integracao daemon/UI que conectam a API do agente ao control plane.
- Essa participacao ajuda a manter coerencia com o historico da `main`, onde GabrielSalustiano aparece em rotas, runtime e estabilidade operacional.

### Kaike-Vitorino

- Liderou a integracao estrutural da Sprint 4: runtime nativo, state store, memoria local, API do agente, policies, secrets vault, providers e hardening.
- Tambem manteve a regra de preservar a `main` organizada, trazendo a Sprint 4 da `agent-config-ui` sem importar commits duplicados.
- Isso foi importante porque a Sprint 4 e a espinha dorsal tecnica do agente local.

## Sprint 5 parcial

### Contexto

Sprint iniciada para canais oficiais, release gate final, empacotamento multiplataforma, extensibilidade por plugins e fundacoes para automacao autonoma/workflows. Parte do backlog segue aberta, mas alguns blocos ja aparecem nesta branch.

### PETROMYZONMONSTER

- Aparece na sustentacao de tools/skills que serve de base para extensibilidade futura e automacao por ferramenta.
- Essa frente ajuda a preparar o terreno para plugins e agentes autonomos sem afirmar que o sistema WASM completo ja esteja fechado.

### MarcellinhoHM

- Ficou com documentacao e adaptador de providers que servem como fundacao para extensibilidade externa.
- Tambem ajuda a registrar limites da Sprint 5 parcial, separando entregas reais de itens ainda em backlog.

### gabriellima-4

- Contribuiu com o frontend orbital/desktop e fluxos de control plane que sustentam a experiencia de canais e configuracao avancada.
- Essa entrega se conecta ao empacotamento multiplataforma e ao uso do app como produto desktop.

### RamLi06

- Ficou com release gate, smoke tests finais, relatorios de validacao e scripts de dry run.
- Essa participacao torna visivel a frente de QA e evidencia tecnica do MVP final.

### GabrielSalustiano

- Assumiu a frente de canais oficiais/control plane, incluindo runtime multi-account, WhatsApp bridge, QR local e documentacao de protocolo/transporte.
- Essa e uma das partes de Sprint 5 que ja tem implementacao concreta na branch.

### Kaike-Vitorino

- Coordenou a classificacao da Sprint 5 parcial dentro da integracao, deixando claro o que entrou e o que permanece aberto.
- Ficou tambem com as fundacoes de runtime, estado e limites que preparam a trilha de loop OODA, sem marcar n8n embedded ou plugins WASM como completos.

## Observacao

As atribuicoes foram redistribuidas para equilibrar a autoria da `main` entre `2026-03-28` e `2026-04-07`, e a `main_2` manteve esse criterio ao adicionar a integracao da `agent-config-ui`. O detalhamento acima foi escrito para servir como memoria funcional do time: o que cada pessoa puxou, como implementou e por que aquela parte foi relevante em cada sprint, inclusive quando a Sprint 5 ainda esta parcialmente aberta no backlog.
