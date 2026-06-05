# Runtime Hardening Cycle — 2026-04-18

## What Changed

Codigo alterado neste ciclo:
- `crates/agent-tools/src/tools/exec.rs`
- `crates/agent-tools/src/checkpoints.rs`
- `crates/agent-tools/src/scheduler.rs`
- `crates/agent-tools/src/tools/{write_file,edit_file}.rs`
- `crates/agent-tools/src/{lib.rs,types.rs}`
- `crates/agent-core/src/{tool_catalog.rs,prompt_builder.rs}`
- `crates/daemon/src/{agent_runtime_tools.rs,config.rs}`

Tambem permanecem neste branch as melhorias anteriores de runtime/provider:
- `crates/daemon/src/runtime_doctor.rs`
- `crates/daemon/src/lib.rs`
- `crates/providers/{mlx,llamacpp,ollama}/src/lib.rs`
- `docs/local-runtime-doctor.md`

## Improvements

### Added

- fila local de execucao com prioridade
- limites por dominio e limite total de concorrencia
- rollback local de mutacoes de arquivo
- novos tools: `checkpoints_list`, `checkpoint_restore`
- metadados de checkpoint anexados aos resultados de `write_file` e `edit_file`

### Hardened

- `exec` sem shell generico
- bloqueio explicito de pipes, redirects e chaining
- metadados operacionais de `exec`: prioridade, dominio, espera na fila, truncation flags
- prompt do agente alinhado com a nova restricao operacional

## What Was Intentionally Not Added

- rollback de comandos arbitrarios executados por `exec`
- sandbox por container/worktree por sessao
- registry unico de modelos com metadata GGUF/MLX/Ollama
- streaming agentico ponta a ponta

Motivo:
- o ganho imediato mais alto estava em reduzir superficie de risco do host e tornar mutacoes revertiveis
- rollback de `exec` exige isolamento muito mais forte; fazer isso pela metade seria fragil

## Performance Impact

Esperado:
- leve aumento de latencia no primeiro `exec` por causa da fila/scheduler
- estabilidade melhor sob concorrencia porque o host deixa de receber spawns paralelos descontrolados
- custo pequeno de I/O para gravar checkpoints em `write_file` e `edit_file`

Trade-off:
- comandos que dependiam de shell operators deixam de funcionar por design

## Risk Analysis

Riscos aceitos:
- agentes/prompt antigos que tentem `cmd1 && cmd2`, `cmd | other`, `> file` vao falhar
- o scheduler ainda so esta conectado diretamente ao dominio `system`

Riscos mitigados:
- shell injection acidental
- pipelines arbitrarios fora do escopo do workspace
- perda de alteracao em `write_file`/`edit_file` sem caminho de undo

## Validation

Comandos executados:
- `cargo test -p mlx-agent-tools --lib`
- `cargo test -p mlx-agent-core --lib`
- `cargo test -p mlx-ollama-daemon --lib`

Resultado:
- todos passaram neste workspace em `2026-04-18`
