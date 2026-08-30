# n8n Source Integration

MLX Pilot keeps the n8n source tree in this repository as a Git submodule at
`vendor/n8n`.

The integration boundary is still direct control over a local n8n instance
through the n8n Public API. MLX Pilot does not expose a dedicated n8n webhook
endpoint anymore. When a workflow needs to call MLX Pilot, it should use an n8n
`HTTP Request` node pointed at `POST /agent/run`.

## What is included

- `.gitmodules` points `vendor/n8n` to the official n8n repository.
- `GET /integrations/n8n/status` checks n8n health and reports local source
  metadata from `vendor/n8n/package.json`.
- `POST /integrations/n8n/workflows/list` lists workflows through the n8n Public
  API.
- `POST /integrations/n8n/workflows/generate` turns a prompt into n8n workflow
  JSON through the MLX Pilot agent, validates the shape, and creates the
  workflow through the n8n Public API.
- The desktop app exposes these controls and embeds the n8n editor in the
  `Workflows` tab.

## Source setup

Initialize the source tree after cloning MLX Pilot:

```powershell
git submodule update --init --recursive vendor/n8n
```

Check local requirements:

```powershell
.\scripts\n8n-source-status.ps1
```

If PowerShell blocks `.ps1` execution, run scripts like this:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\n8n-source-status.ps1
```

The vendored n8n package currently declares:

- Node `>=24.0.0`
- pnpm `>=11.22.0`
- package manager `pnpm@11.22.0`

Bootstrap n8n from source:

```powershell
.\scripts\n8n-source-bootstrap.ps1
```

This prepares the pnpm version declared by n8n into `.cache/corepack`, installs
n8n dependencies, and builds the local n8n runtime with:

```powershell
pnpm turbo run build --filter=n8n... --output-logs=full --concurrency=1
```

It needs internet access the first time.

Do not run `corepack.cmd enable` on Windows unless the terminal is elevated as
administrator. The helper scripts intentionally avoid it because it tries to
write `pnpm.ps1` under `C:\Program Files\nodejs`. Instead, they set
`COREPACK_HOME` to `.cache/corepack` inside this repository.

The upstream production bundle command is still available when you have
Git Bash/WSL or a POSIX-compatible shell:

```powershell
.\scripts\n8n-source-bootstrap.ps1 -ProductionBundle
```

On plain Windows PowerShell, prefer the default bootstrap command above.

Start n8n from the vendored source:

```powershell
.\scripts\n8n-source-start.ps1
```

By default it serves the editor at:

```text
http://127.0.0.1:5678
```

The start script sets `N8N_PREVIEW_MODE=true` so n8n does not send the
`X-Frame-Options: sameorigin` header that blocks the embedded editor in the
desktop app.

## n8n API key

Open n8n and create an API key:

```text
Settings > n8n API > Create API key
```

Keep MLX Pilot running on:

```text
http://127.0.0.1:11435
```

## Direct endpoints

Status:

```powershell
Invoke-RestMethod `
  -Method Get `
  -Uri "http://127.0.0.1:11435/integrations/n8n/status?base_url=http://127.0.0.1:5678"
```

List workflows:

```powershell
$body = @{
  base_url = "http://127.0.0.1:5678"
  api_key = "N8N_API_KEY"
} | ConvertTo-Json

Invoke-RestMethod `
  -Method Post `
  -Uri "http://127.0.0.1:11435/integrations/n8n/workflows/list" `
  -ContentType "application/json" `
  -Body $body
```

Generate and create a workflow from a prompt:

```powershell
$body = @{
  base_url = "http://127.0.0.1:5678"
  api_key = "N8N_API_KEY"
  name = "Resumo via MLX Pilot"
  mlx_base_url = "http://127.0.0.1:11435"
  ollama_base_url = "http://127.0.0.1:11434"
  workflow_model_id = "qwen3.5:9b"
  prompt = "Crie um workflow manual que mande um texto para o MLX Pilot, resuma em uma frase e deixe a resposta disponivel no output."
} | ConvertTo-Json

Invoke-RestMethod `
  -Method Post `
  -Uri "http://127.0.0.1:11435/integrations/n8n/workflows/generate" `
  -ContentType "application/json" `
  -Body $body
```

The generator asks the active MLX Pilot Agent provider/model to return only
valid n8n workflow JSON. MLX Pilot then normalizes the workflow before sending
it to n8n. If generation returns `provider_error`, test the Agent tab with a
simple message first; the n8n workflow creation depends on that same provider
being reachable. When the active Agent provider is Ollama, the desktop UI sends
the `Ollama URL` field as the generation `agent_base_url`; for a regular
Ollama Desktop install this should be `http://127.0.0.1:11434`.

For workflows that call MLX Pilot, the generated node should use:

- `Manual Trigger` or another trigger requested in the prompt
- `HTTP Request` to `POST /agent/run`

The generated workflow is inactive by default; open it in n8n and run or adjust
it from the editor.

## Desktop UI

In the desktop app, open the `Workflows` tab:

1. Set `Base URL` to `http://127.0.0.1:5678`.
2. Paste the API key generated in n8n.
3. Click `Status` to check both the running n8n instance and `vendor/n8n`.
4. Use the embedded `Editor n8n` panel at the top of the tab for the visual
   workflow editor.
5. In `Gerador de workflow`, write what the workflow should do.
6. Click `Gerar workflow`. MLX Pilot generates the JSON, creates the workflow in
   n8n, shows the JSON preview, and opens the created workflow in the embedded
   editor when n8n returns its id.

If the embedded editor stays blank, click `Abrir fora`. Some local n8n/editor
settings can reject iframe embedding; the direct API integration still works in
that case.

## License note

n8n is not under a permissive OSI license; the vendored source carries n8n's
Sustainable Use License and related notices. Keep `vendor/n8n/LICENSE.md` with
the source tree and review the current upstream license before distributing a
fork or binary that includes n8n.
