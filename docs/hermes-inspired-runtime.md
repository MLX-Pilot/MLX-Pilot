# Hermes-Inspired Runtime

O MLX-Pilot agora possui um runtime opt-in `hermes_inspired` para aproximar o fluxo nativo do modelo operacional do Hermes sem depender do runtime Python.

## O que ele adiciona

- loop multi-turn persistente com session state estruturado
- memoria local hidratada antes do turno
- snapshots de contexto e resumos persistidos por sessao
- recall de sessoes anteriores (`session_search`)
- escrita de memoria duravel (`memory_write`)
- delegacao sincronizada para sessao filha (`delegate_session`) com handoff explicito
- toolsets nomeados para restringir o subconjunto de tools por sessao
- provider profiles para separar selecao de provider/modelo da config global
- contexto de gateway local (`source_channel`, `thread_id`, `sender_id`, `correlation_id`)
- Ollama tratado como provider local de primeira classe no mesmo fluxo do runtime

## Configuracao

Campos em `agent`:

- `runtime_variant`: `classic` ou `hermes_inspired`
- `persist_tool_events`: persiste `tool_call` / `tool_result` na sessao
- `memory_profile`: `minimal`, `balanced`, `full`
- `memory_snapshot_mode`: `off`, `session`, `turn`
- `session_search_enabled`: habilita recall entre sessoes
- `default_toolset_id`: toolset padrao para requests sem override
- `provider_profile_id`: profile padrao de provider/modelo
- `gateway_mode`: modo atual de gateway/contexto local
- `provider_profiles`: lista de perfis persistidos para providers locais/remotos

## Storage local

O runtime usa SQLite local em:

- `settings/agent/state.sqlite`

Esse banco concentra:

- sessoes
- eventos de sessao
- resumos de sessao
- snapshots de contexto
- memoria duravel
- busca FTS quando suportada pela build do SQLite, com fallback para busca textual simples

## Exemplo minimo

```json
POST /agent/run
{
  "provider_profile_id": "ollama-local",
  "message": "Use session_search se houver contexto anterior e depois responda.",
  "runtime_variant": "hermes_inspired",
  "persist_tool_events": true,
  "session_search_enabled": true,
  "memory_profile": "balanced",
  "memory_snapshot_mode": "session",
  "toolset_id": "general",
  "gateway_context": {
    "source_channel": "desktop",
    "thread_id": "local-chat",
    "sender_id": "operator"
  }
}
```

Fluxo esperado:

1. o runtime carrega o snapshot anterior e hidrata contexto com memoria + sessoes relevantes
2. o modelo pode chamar `session_search`, `memory_search` ou uma tool local normal
3. o subconjunto de tools disponiveis e filtrado pelo `toolset_id`
4. o backend persiste `tool_call` / `tool_result` quando `persist_tool_events=true`
5. o agente pode gravar memoria duravel via `memory_write`
6. ao final do turno, o runtime grava resumo + snapshot local da sessao
7. uma segunda sessao pode recuperar isso via `memory_search` ou `session_search`

Se houver skills em `skills/`, `.claude/skills/`, `.hermes/skills/` ou `.codex/skills/`, o runtime continua carregando essas skills no mesmo fluxo do `AgentLoop`, agora com metadata adicional de formato, rotinas, workflows e diretorios auxiliares indexados.

## Tools relevantes

- `session_search`
- `memory_search`
- `memory_get`
- `memory_write`
- `delegate_session`
- `toolsets_list`

## Toolsets e provider profiles

Toolsets disponiveis neste ciclo:

- `general`
- `messaging`
- `full`
- `safe_readonly`

O daemon tambem expoe provider profiles via:

- `GET /agent/provider-profiles`

e toolsets via:

- `GET /agent/toolsets`

## Limites deste ciclo

- delegacao paralela ainda nao foi adicionada
- o gateway/messaging ainda e apenas um shape local de contexto, nao um sistema completo de canais do Hermes
- a compatibilidade de skills e conceitual/metadata, nao importacao direta de implementacoes Python

