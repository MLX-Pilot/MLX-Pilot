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

### 6.2 ApprovalService

Modos:
- `auto`: aprova automaticamente
- `ask`: exige aprovacao do usuario
- `deny`: nega por padrao

### 6.3 AuditLog

Registro JSONL estruturado por evento:
- tool name
- params hash/sumario
- duracao
- decisao/politica
- erro resumido

### 6.4 Secrets Vault

API keys sao armazenadas localmente de forma criptografada:
- chave local dedicada
- payload cifrado
- configuracao persiste apenas referencia (`api_key_ref`) quando vault ativo
- logs nao devem expor segredo em claro

## 7. Multi-provider

Providers suportados:
- locais: MLX, llama.cpp, Ollama
- remotos: OpenAI-compatible (OpenRouter/Groq/custom), Anthropic, DeepSeek

Recursos:
- `base_url` customizado
- headers customizados
- fallback opcional provider primario -> fallback

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

## 10. Limites atuais (preview)

- `POST /agent/stream` ainda em modo stub para stream full de eventos.
- coverage de ferramentas e policy deve continuar evoluindo para cenario production-grade.

