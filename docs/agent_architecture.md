# Agent Architecture (MLX-Pilot v0.1.0 preview)

## 1. Objetivo

O Agent do MLX-Pilot foi desenhado para executar raciocinio com tool-calling de forma:
- local-first
- multi-provider
- segura por padrao
- integrada na UI desktop

## 2. Crates e responsabilidades

| Componente | Papel |
|---|---|
| `crates/agent-core` | Loop do agente, prompt builder, policy/approval/audit, orquestracao de tools e provider. |
| `crates/agent-tools` | Ferramentas builtin (`read_file`, `write_file`, `edit_file`, `list_dir`, `exec`) + sandbox. |
| `crates/agent-skills` | Parse e load de skills no formato `SKILL.md`, capacidades e hash de integridade. |
| `crates/providers/*` | Implementacoes de provider (MLX, llama.cpp, Ollama, HTTP LLM). |
| `crates/daemon` | API HTTP, configuracao persistente, roteamento de providers e integracao com UI. |
| `apps/desktop-ui` | Interface do usuario para chat e configuracao do agente. |

## 3. Fluxo de execucao

1. UI envia `POST /agent/run` com mensagem + configuracao efetiva.
2. Daemon resolve provider/modelo e constroi `PolicyConfig`.
3. Agent runtime carrega skills do workspace e aplica checks de integridade/pin.
4. `AgentLoop` monta prompt otimizado (identidade, regras, skills compactadas, tools ativas, janela de conversa).
5. Provider retorna resposta:
- resposta final direta, ou
- `tool_calls` estruturados.
6. Cada tool_call passa por:
- policy enforcement
- approval flow (quando aplicavel)
- execucao via `ToolRegistry`
- auditoria JSONL
7. Resultado da tool retorna para o loop ate resposta final ou limite de iteracoes.

## 4. Camada de Prompt Engineering

O prompt e dividido em blocos pequenos para modelos locais e remotos:
- identity fixa e curta
- runtime rules curtas
- skill summaries (1 linha por skill)
- tool schema minimo
- historico com sliding window/truncation inteligente

Profiles por modelo controlam:
- max prompt tokens
- max history messages
- max tools no prompt
- temperatura default

## 5. Skill System e Capabilities

Cada skill pode declarar capacidades explicitas:
- `fs_read`
- `fs_write`
- `network`
- `exec`
- `secrets_access`

No load, o `SKILL.md` recebe hash SHA256.

Integridade:
- pin opcional por skill (`skill_sha256_pins`)
- estado conhecido salvo localmente para detectar alteracoes
- mudanca sem pin gera aviso
- mismatch com pin gera bloqueio

## 6. Seguranca

### 6.1 PolicyEngine

Aplicado antes de qualquer execucao de tool:
- allow/deny por glob de nome de tool
- deny de comandos sensiveis
- bloqueio de paths sensiveis
- egress allowlist por dominio
- bloqueio de IP direto
- owner-only mode
- airgapped mode

### 6.2 CapabilityAuthority

Quando `agent.security.require_capabilities = true`, o runtime passa a exigir um manifest explicito
para tools sensiveis, inspirado no modelo de capabilities do Tauri v2.

Capacidades suportadas neste ciclo:
- `fs:read`
- `fs:write`
- `process:spawn`

Escopos suportados:
- `scopes.fs.allow/deny`
- `scopes.process.allow/deny`

Comportamento:
- o daemon pode carregar manifests por `agent.security.capability_manifest_paths`
- se nenhum path for configurado, o backend deriva um manifest default do workspace e dos tools habilitados
- a checagem acontece dentro de `ToolRegistry::dispatch`, antes da tool ser executada
- o binding atual cobre o contexto `agent`; automations/plugins podem reutilizar a mesma autoridade no futuro

### 6.3 ApprovalService

Modos:
- `auto`: aprova automaticamente
- `ask`: exige aprovacao do usuario
- `deny`: nega por padrao

### 6.4 AuditLog

Registro JSONL estruturado por evento:
- tool name
- params hash/sumario
- duracao
- decisao/politica
- erro resumido

### 6.5 Secrets Vault

API keys sao armazenadas localmente de forma criptografada:
- chave local dedicada
- payload cifrado
- configuracao persiste apenas referencia (`api_key_ref`) quando vault ativo
- logs nao devem expor segredo em claro

### 6.6 Execution Queue e Rollback Local

O runtime agora assume que execucao de tool mutavel precisa de controle operacional:
- `exec` roda como invocacao direta de processo, sem pipes, redirects ou encadeamento de shell
- chamadas de `exec` entram em fila local com prioridade (`low` / `normal` / `high`)
- a fila respeita limites por dominio (`inference`, `tools`, `memory`, `system`) e um limite total
- `write_file` e `edit_file` gravam checkpoints locais em `.mlx-pilot/checkpoints/`
- checkpoints podem ser listados e restaurados com `checkpoints_list` e `checkpoint_restore`
- metadados de checkpoint incluem `session_id`, caminho relativo, hash do estado novo e, quando houver, `git HEAD`

## 7. Multi-provider

Providers suportados:
- locais: MLX, llama.cpp, Ollama
- remotos: OpenAI-compatible (OpenRouter/Groq/custom), Anthropic, DeepSeek

Recursos:
- `base_url` customizado
- headers customizados
- fallback opcional provider primario -> fallback

Tambem existe uma camada TypeScript standalone em `providers/unified_adapter/` para normalizar
requests/responses de chat, tool-calling e roteamento entre:
- Anthropic Messages
- OpenAI Chat Completions
- Ollama
- MLX
- llama.cpp

Ela ainda nao e o caminho padrao do daemon, mas ja possui adapters, testes e exemplo E2E para futura
integracao com frontends ou bridges JS locais.

## 8. Integracao UI

A aba Agent concentra:
- selecao de provider/modelo
- credenciais e endpoint
- execution mode
- toggle de skills/tools
- controles de seguranca
- chat do agente

Configuracao e persistida localmente e aplicada no backend a cada run.

## 9. Observabilidade e Operacao

- endpoint de audit (`GET /agent/audit`)
- metadados de skills (`GET /agent/skills`)
- politica efetiva por tool (`GET /agent/tools`)
- configuracao do agente (`GET/POST /agent/config`)

## 10. Session Management

Para manter um historico rico e local da atividade do agente:
- **SessionStore**: persiste sessoes e eventos estruturados em SQLite local (`settings/agent/state.sqlite`), com import de legado JSONL quando necessario.
- Gerenciamento via UI: endpoints `GET/POST/PATCH/DELETE /agent/sessions` gerenciam as sessoes ativas.
- As mensagens, tool calls, tool results e snapshots relevantes originados de chamadas `/agent/run` podem ser anexados a sessao fornecida (`session_id`).
- Opcao nativa de exportacao via `GET /agent/sessions/:id/export` para revisar as traces completas em JSON normalizado.

## 11. Hermes-Inspired Runtime

O backend agora suporta dois modos:

- `classic`: caminho legado, mantendo o `AgentLoop` atual.
- `hermes_inspired`: runtime opt-in com:
  - hidratacao de memoria local antes do turno
  - snapshots e resumos persistidos por sessao
  - busca em sessoes anteriores (`session_search`)
  - persistencia estruturada de `tool_call` / `tool_result`
  - escrita explicita de memoria duravel (`memory_write`)
  - delegacao sincronizada de sessao filha (`delegate_session`)
  - toolsets nomeados para restringir o subconjunto de tools por request
  - provider profiles para separar configuracao global de provider/modelo da selecao por sessao
  - contexto local de gateway (`source_channel`, `thread_id`, `sender_id`, `correlation_id`)

Configuracao:

- `agent.runtime_variant`
- `agent.persist_tool_events`
- `agent.memory_profile`
- `agent.memory_snapshot_mode`
- `agent.session_search_enabled`
- `agent.default_toolset_id`
- `agent.provider_profile_id`
- `agent.provider_profiles`
- `agent.gateway_mode`

Endpoints operacionais novos:

- `GET /agent/toolsets`
- `GET /agent/provider-profiles`

## 12. Limites atuais (preview)

- `POST /agent/stream` ainda em modo stub para stream full de eventos.
- coverage de ferramentas e policy deve continuar evoluindo para cenario production-grade.
- ainda nao existe um registry unificado de modelos locais com metadata rica de GGUF/MLX/Ollama.
- a fila de execucao hoje cobre de forma concreta o dominio `system`; os outros dominios estao preparados no scheduler, mas ainda nao foram conectados a todos os fluxos do runtime.

