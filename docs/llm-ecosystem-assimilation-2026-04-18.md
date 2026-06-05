# LLM Ecosystem Assimilation — 2026-04-18

## Primary Sources

- `llama.cpp`: https://github.com/ggml-org/llama.cpp
- `llama-server` dev docs: https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README-dev.md
- `MLX`: https://github.com/ml-explore/mlx
- `MLC-LLM`: https://github.com/mlc-ai/mlc-llm
- `MLC docs`: https://llm.mlc.ai/docs/get_started/introduction.html
- `Ollama`: https://github.com/ollama/ollama
- `Ollama tool calling`: https://docs.ollama.com/capabilities/tool-calling
- `Ollama streaming`: https://docs.ollama.com/capabilities/streaming

## External Patterns Worth Absorbing

### llama.cpp

Observed patterns:
- `server_queue` + `server_response` desacoplando HTTP de inferencia
- `server_slot` para concorrencia previsivel por sequencia
- router mode para multi-modelo
- checkpoints de prompt/KV e reuse de prefixo
- OpenAI-compatible server com foco em throughput e baixo acoplamento

Assimilacao para MLX-Pilot:
- MUST HAVE: fila local explicita para execucao sensivel e runtime controlado
- MUST HAVE: separar melhor plano HTTP/API de plano de trabalho pesado
- NICE TO HAVE: registry/roteador multi-modelo nativo
- EXPERIMENTAL: reuse de prefixo/KV

### MLX

Observed patterns:
- lazy computation
- multi-device awareness
- foco em Apple Silicon como first-class backend
- design simples e extensivel

Assimilacao para MLX-Pilot:
- MUST HAVE: tratar MLX como backend otimizado mas nao presumir estabilidade
- NICE TO HAVE: preflight detalhado por plataforma/accelerator
- EXPERIMENTAL: cache e precompile de kernels/exec paths expostos ao runtime do MLX-Pilot

### MLC-LLM

Observed patterns:
- modelo compilado por alvo de hardware
- package config e cache local de build/JIT
- mesmo engine em varias superficies com API compativel OpenAI

Assimilacao para MLX-Pilot:
- MUST HAVE: metadata operacional por modelo/backend, nao so nome de modelo
- NICE TO HAVE: manifests locais por backend com artefatos compilados e compatibilidade
- EXPERIMENTAL: pipeline interno de packaging/compilacao

### Ollama

Observed patterns:
- lifecycle local forte: `pull`, `ls`, `ps`, `stop`, `Modelfile`
- endpoint local consistente e simples
- tool calling e streaming bem documentados
- onboarding/integration UX voltado para apps locais

Assimilacao para MLX-Pilot:
- MUST HAVE: provider local de primeira classe com health e fallback reais
- MUST HAVE: doctor/preflight de runtime
- MUST HAVE: inventario de modelos locais mais claro
- NICE TO HAVE: camada de model lifecycle interna inspirada em `ls/ps/stop/show`

## Classification

### MUST HAVE

- fila local com concorrencia controlada e prioridade
- bloqueio de operadores de shell em `exec`
- rollback/checkpoints para mutacoes de arquivo
- runtime doctor + fallback real entre backends
- inventario local de modelos mais rico que o nome bruto do provider

### NICE TO HAVE

- parser GGUF nativo
- registry local de modelos com contexto, quantizacao e afinidade de backend
- UX de lifecycle (`list`, `running`, `load`, `unload`, `inspect`)
- streaming end-to-end com deltas de tools

### EXPERIMENTAL

- prefix/KV checkpoint reuse
- packaging/compilacao local estilo MLC
- router multi-modelo com scheduling mais sofisticado
- worktree/sandbox efemero por sessao

## Implemented In This Cycle

1. `exec` foi refeito como invocacao direta de processo, sem shell piping/chaining/redirection.
2. `exec` agora passa por fila local com prioridade e limites por dominio.
3. `write_file` e `edit_file` agora criam checkpoints locais recuperaveis.
4. `checkpoints_list` e `checkpoint_restore` entraram no runtime do agente.
5. O prompt do runtime agora deixa explicito que `exec` nao aceita operadores de shell.
6. O work already present neste branch para provider health/fallback e `runtime_doctor` permanece alinhado com a assimilacao inspirada em Ollama e MLX.

## Intentionally Not Added Yet

- dependencia de runtime em projetos terceiros
- parser GGUF completo neste ciclo
- packaging/compilacao estilo MLC dentro do produto
- worktree isolation automatica para toda sessao
- speculative decoding, prefix cache e router multi-modelo complexo

Motivo:
- todos esses itens tem custo de manutencao e risco arquitetural maior que o ganho imediato neste ciclo
- a base correta primeiro era endurecer execucao local, fallback de backend e rollback
