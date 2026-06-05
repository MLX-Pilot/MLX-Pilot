# MLX-Pilot Review — 2026-04-18

## Escopo analisado

Workspace inspecionado:
- `crates/core`
- `crates/providers/{mlx,llamacpp,ollama,http_llm_provider}`
- `crates/agent-core`
- `crates/agent-tools`
- `crates/agent-skills`
- `crates/daemon`
- `apps/desktop-ui`

## Architecture Map

| Camada | Modulos | Responsabilidade principal |
|---|---|---|
| UI | `apps/desktop-ui` | Configuracao, chat, controle operacional do daemon |
| API local | `crates/daemon` | HTTP API, config persistente, roteamento de provider, integracoes de canal, doctor/runtime tools |
| Runtime agentico | `crates/agent-core` | `AgentLoop`, prompt builder, policy, approval, audit, sessions, memory |
| Tooling local | `crates/agent-tools` | filesystem tools, exec, sandbox, fila local de execucao, checkpoints |
| Skills | `crates/agent-skills` | discovery, parse de `SKILL.md`, capabilities, integridade |
| Providers | `crates/providers/*` | adaptadores locais/remotos para MLX, llama.cpp, Ollama e OpenAI-style |
| Core types | `crates/core` | tipos compartilhados de request/response, providers e chat |

Fluxo dominante hoje:
1. UI ou cliente local chama o daemon.
2. O daemon resolve provider/modelo e politica efetiva.
3. `agent-core` monta prompt, skills e tools ativos.
4. O provider produz texto ou `tool_calls`.
5. `ToolRegistry` executa tools locais com policy + approval + audit.
6. Sessao, memoria e artefatos locais sao persistidos em disco.

## Dependency Graph

Grafo funcional do workspace:
- `apps/desktop-ui -> crates/daemon`
- `crates/daemon -> crates/agent-core`
- `crates/daemon -> crates/agent-tools`
- `crates/daemon -> crates/agent-skills`
- `crates/daemon -> crates/providers/*`
- `crates/agent-core -> crates/agent-tools`
- `crates/agent-core -> crates/agent-skills`
- `crates/providers/* -> crates/core`

Caracteristica importante:
- a maior parte da logica operacional relevante vive no daemon + `agent-core`
- `agent-tools` e o ponto mais sensivel de seguranca local
- os providers ainda sao adaptadores relativamente finos; a inteligencia operacional esta acima deles

## Performance Bottlenecks

1. `SessionStore` e `AuditLog` usam JSON/JSONL e releitura completa de arquivos. Isso e simples e robusto, mas piora com sessoes longas e auditoria pesada.
2. O runtime ainda nao possui streaming de eventos ponta a ponta maduro; isso esconde latencia e dificulta UX responsiva.
3. Falta um registry unificado de modelos locais com metadata normalizada; discovery e compatibilidade ainda dependem demais do provider individual.
4. O custo de prompt ainda cresce com historico e numero de tools; existem budgets, mas ainda nao ha cache de prefixo/KV ou precomputo de manifestos.
5. Antes deste ciclo, `exec` dependia de shell generico. Isso era um problema de seguranca e tambem um problema operacional porque a concorrencia era nao controlada.

## Code Smells / Weaknesses

1. Persistencia local dispersa em JSONL/JSON e sem indice unico para sessoes, memoria, checkpoints e auditoria.
2. Tooling mutavel e altamente sensivel (`exec`, `write_file`, `edit_file`) sem rollback nativo antes deste ciclo.
3. Sem lifecycle manager interno para modelos com estados `downloaded / loaded / running / unhealthy`.
4. Falta de metadata de modelo mais rica: quantizacao, context length, backend affinity, embedding/reranker/chat.
5. Convergencia incompleta entre runtime local, control plane da UI e health/runtime doctor.

## Missing Features vs Modern Local LLM Tooling

### Must address

- streaming agentico real com eventos de tool-call e tool-result
- registry local de modelos com inventario unico por backend
- scheduler interno para execucao sensivel
- rollback local para mutacoes de arquivo
- preflight operacional por backend/modelo

### Important next

- parser de metadata GGUF
- visibilidade de contexto efetivo por modelo/backend
- task queue mais ampla que cubra inference, memory e automations
- busca/local recall mais forte para memoria longa

### Useful but later

- cache de prefixo/KV
- prewarming de modelos
- manifests de empacotamento por hardware
- benchmark automatizado por backend/modelo
