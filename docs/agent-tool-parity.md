# Agent Tool Parity

Atualizado em 16 de abril de 2026.

Este documento compara as tools/capacidades padrao de ecossistemas de referencia com o que o MLX-Pilot Agent implementa hoje.

## Fontes oficiais

- Claude Code tools reference: https://code.claude.com/docs/en/tools-reference
- OpenCode tools: https://opencode.ai/docs/tools/
- OpenAI Codex CLI: https://developers.openai.com/codex/cli
- OpenAI Codex app: https://openai.com/index/introducing-the-codex-app/
- OpenAI shell tool guide: https://developers.openai.com/api/docs/guides/tools-local-shell

## Claude Code

Lista oficial publicada pela Anthropic:

- `Agent`
- `AskUserQuestion`
- `Bash`
- `CronCreate`
- `CronDelete`
- `CronList`
- `Edit`
- `EnterPlanMode`
- `EnterWorktree`
- `ExitPlanMode`
- `ExitWorktree`
- `Glob`
- `Grep`
- `ListMcpResourcesTool`
- `LSP`
- `Monitor`
- `NotebookEdit`
- `PowerShell`
- `Read`
- `ReadMcpResourceTool`
- `SendMessage`
- `Skill`
- `TaskCreate`
- `TaskGet`
- `TaskList`
- `TaskOutput` (deprecated)
- `TaskStop`
- `TaskUpdate`
- `TeamCreate`
- `TeamDelete`
- `TodoWrite`
- `ToolSearch`
- `WebFetch`
- `WebSearch`
- `Write`

## OpenCode

Lista oficial publicada pelo OpenCode:

- `bash`
- `edit`
- `write`
- `read`
- `grep`
- `glob`
- `lsp` (experimental)
- `apply_patch`
- `skill`
- `todowrite`
- `webfetch`
- `websearch`
- `question`

## OpenAI Codex

A OpenAI publica capacidades e fluxos, mas nao uma tabela publica unica com todos os nomes internos de tools do produto no mesmo formato do Claude Code/OpenCode.

Capacidades publicas documentadas:

- ler arquivos
- editar arquivos
- executar comandos
- usar shell local
- fazer web search
- usar subagents
- usar skills
- usar MCP
- trabalhar com worktrees
- usar automations
- revisar codigo

Observacao:
- O shell/local shell e documentado oficialmente.
- Web search, subagents, skills, MCP, approval modes, worktrees e automations aparecem na documentacao e nas paginas oficiais do Codex CLI/app.
- Esta secao e uma inferencia consolidada a partir dessas fontes, nao uma lista oficial de nomes internos de tools da OpenAI.

## Mapeamento para a linguagem do MLX-Pilot

Implementado hoje:

- `read_file`: ler arquivo
- `list_dir`: listar diretorio
- `glob`: buscar arquivos por padrao
- `grep`: pesquisar conteudo/regex
- `write_file`: criar ou sobrescrever arquivo
- `edit_file`: editar texto com precisao
- `exec`: executar comando de shell ou Python
- `sessions_list`: listar sessoes
- `sessions_history`: ler historico da sessao
- `sessions_spawn`: criar subagente/sessao
- `sessions_send`: enviar mensagem para sessao
- `sessions_status`: ver status de sessao
- `memory_search`: pesquisar memoria local
- `memory_get`: ler memoria local por id
- `message`: enviar mensagem por canal configurado

Plugins/compatibilidade operacional atuais:

- `memory`
- `voice-call`
- `diffs`
- `device-pair`
- `auth`
- `automation-helpers`

## Equivalencias rapidas

- Claude/OpenCode `Read` ou `read` -> MLX `read_file`
- Claude `LS` comportamento / OpenCode navegacao de arquivos -> MLX `list_dir`
- Claude/OpenCode `Glob` ou `glob` -> MLX `glob`
- Claude/OpenCode `Grep` ou `grep` -> MLX `grep`
- Claude `Edit` / OpenCode `edit` / `apply_patch` -> MLX `edit_file`
- Claude `Write` / OpenCode `write` -> MLX `write_file`
- Claude `Bash` ou `PowerShell` / OpenCode `bash` / Codex shell -> MLX `exec`
- Claude `Agent` / Codex subagents -> MLX `sessions_spawn`
- Claude task tools / OpenCode `todowrite` -> MLX ainda sem checklist nativo dedicado
- Claude `WebFetch` / OpenCode `webfetch` / Codex web search stack -> MLX ainda nao implementado no runtime do agent
- Claude `WebSearch` / OpenCode `websearch` / Codex web search -> MLX ainda nao implementado no runtime do agent
- Claude `AskUserQuestion` / OpenCode `question` -> MLX ainda sem tool interativa dedicada no runtime do agent
- Claude `LSP` / OpenCode `lsp` -> MLX ainda nao implementado
- Claude `Monitor` -> MLX ainda nao implementado
- Claude MCP resource tools / Codex MCP -> MLX UI conhece MCP por ecossistema, mas o agent runtime ainda nao expoe essas tools
- Claude worktree tools / Codex worktrees -> MLX ainda nao tem tool runtime dedicada; hoje isso fica coberto por skill e fluxo git

## Gap atual mais importante

Para aproximar de Claude Code / Codex, a proxima leva de runtime tools deveria ser:

1. `web_search`
2. `web_fetch`
3. `ask_user`
4. `todo_write`
5. `lsp`
6. `monitor`
7. `worktree_create`
8. `worktree_exit`
9. `mcp_resources_list`
10. `mcp_resource_read`

## Recomendacao de nomenclatura

Para manter nossa linguagem consistente:

- `read_file` -> "Ler arquivo"
- `list_dir` -> "Listar pasta"
- `glob` -> "Buscar arquivos"
- `grep` -> "Pesquisar texto"
- `write_file` -> "Escrever arquivo"
- `edit_file` -> "Editar arquivo"
- `exec` -> "Executar comando"
- `sessions_spawn` -> "Abrir subagente"
- `memory_search` -> "Pesquisar memoria"
- `message` -> "Enviar mensagem"
