<p align="center">
  <img src="apps/desktop-ui/ui/assets/mlxpilot-wordmark.png" alt="MLX Pilot" />
</p>

# MLX Pilot (Arquitetura Multi-Provider)

Projeto em Rust para execucao local de LLMs com roteamento multi-provider:
- MLX (Apple Silicon)
- llama.cpp embutido (cross-platform via `llama-server` gerenciado pelo daemon)
- Ollama (compatibilidade)

Tambem oferece descoberta/download de modelos e interface desktop (Tauri).

---

## Conceitos principais

### O que e um daemon?

Um daemon e um programa que roda em segundo plano (sem janela), esperando requisicoes.
Neste projeto, o daemon e um servidor HTTP local (por padrao `127.0.0.1:11435`) que expoe endpoints como `/health`, `/models`, `/chat` e `/catalog/...`.

### Camadas do projeto

1. Backend (Rust)
- Servidor HTTP com endpoints de saude, modelos, chat e catalogo.
- Roteamento dinamico para providers de inferencia.

2. Catalogo remoto
- Integracao com Hugging Face para busca, detalhes e downloads.

3. Interface desktop (Tauri)
- App nativo com abas de Chat e Descobrir Modelos.

### Workspace Cargo

O repositorio e um workspace Rust com multiplas crates (core, providers e daemon), alem do app desktop.

---

## O que esta fase entrega

- Daemon HTTP em Rust com endpoints:
- `GET /health`
- `GET /models`
- `POST /chat`
- `POST /chat/stream`
- `GET /catalog/sources`
- `GET /catalog/models`
- `POST /catalog/downloads`
- `GET /catalog/downloads`
- `GET /catalog/downloads/{job_id}`
- Provider MLX para modelos locais via CLI (`python3 -m mlx_lm.generate`, por padrao).
- Provider llama.cpp embutido com `llama-server` gerenciado pelo daemon.
- Provider Ollama para compatibilidade.
- UI desktop (Tauri + frontend estatico).

---

## Agent

### Recursos

- Agent loop completo em Rust com iteracao multi-turn e tool-calling.
- Loader de skills compativel com `SKILL.md` (sem injetar corpo integral no prompt).
- Prompt engineering adaptativo para modelos locais/remotos.
- API dedicada do agente:
- `POST /agent/run`
- `POST /agent/stream` (stub para streaming de eventos)
- `GET /agent/providers`
- `GET/POST /agent/config`
- `GET /agent/skills`
- `POST /agent/skills/reload`
- `GET /agent/tools`
- `GET /agent/audit`
- `POST /agent/approve`

### Multi-provider

- Providers locais: `mlx`, `llamacpp`, `ollama`.
- Providers remotos: `openai`, `anthropic`, `groq`, `openrouter`, `deepseek`.
- Endpoint customizavel (`custom`) com `base_url` e headers.
- Fallback opcional entre provider primario e secundario por configuracao.

### Seguranca

- `PolicyEngine` com allow/deny por glob, bloqueio de paths sensiveis e controle de egress.
- `ApprovalService` com modos `auto`, `ask` e `deny`.
- `AuditLog` estruturado em JSONL para trilha de execucao.
- Modo enterprise/paranoid com:
- capabilities declarativas por skill (`fs_read`, `fs_write`, `network`, `exec`, `secrets_access`)
- integridade de skill (SHA256 + pin opcional)
- cofre local criptografado para API keys
- airgapped mode e owner-only mode

### UI

- Aba **Agent** no desktop com configuracao de provider, modelo, execucao e seguranca.
- Controle de skills/tools ativos direto na UI.
- Chat do agente integrado ao fluxo principal do MLX-Pilot.

---

## Estrutura do repositorio

```text
mlx-ollama-pilot/
|-- Cargo.toml
|-- crates/
|   |-- core/
|   |-- agent-core/
|   |-- agent-tools/
|   |-- agent-skills/
|   |-- providers/
|   |   |-- mlx/
|   |   |-- llamacpp/
|   |   |-- ollama/
|   |   '-- http_llm_provider/
|   |-- bench_agent/
|   '-- daemon/
|-- apps/
|   '-- desktop-ui/
|       |-- ui/
|       '-- src-tauri/
'-- scripts/
```

| Pasta | Papel |
|---|---|
| `crates/core` | Contratos de dominio (tipos, erros, trait `ModelProvider`). |
| `crates/agent-core` | Agent loop, prompt builder, policy/approval/audit e runtime de skills. |
| `crates/agent-tools` | Ferramentas (read/write/edit/list/exec) e sandbox de IO. |
| `crates/agent-skills` | Parser/loader de skills e metadados de compatibilidade. |
| `crates/providers/mlx` | Provider MLX. |
| `crates/providers/llamacpp` | Provider llama.cpp embutido. |
| `crates/providers/ollama` | Provider Ollama. |
| `crates/providers/http_llm_provider` | Provider HTTP generico (OpenAI-compatible/Anthropic). |
| `crates/bench_agent` | Benchmark comparativo automatizado entre OpenClaw/NanoBot/Rust Agent. |
| `crates/daemon` | Servidor HTTP principal. |
| `apps/desktop-ui` | App desktop Tauri e frontend. |
| `scripts` | Scripts de conveniencia (`run-desktop.sh`, `stop-daemon.sh`) para macOS/Linux. |

---

## Requisitos

### Requisitos gerais

- Rust (toolchain estavel via `rustup`)
- Python com `mlx-lm` no ambiente usado pelo daemon (quando for usar MLX)
- Modelos locais

### Windows

Instale Rust e Build Tools:

```powershell
winget install -e --id Rustlang.Rustup
winget install -e --id Microsoft.VisualStudio.2022.BuildTools
```

No instalador do Visual Studio Build Tools, selecione:
- `Desktop development with C++`
- `MSVC v143`
- `Windows 10/11 SDK`

Validacao:

```powershell
rustup --version
cargo --version
```

Se `cargo` nao estiver no PATH:

```powershell
$env:Path += ";$env:USERPROFILE\.cargo\bin"
cargo --version
```

Instale a CLI do Tauri:

```powershell
cargo install tauri-cli --locked
```

### macOS/Linux

- Rust (`rustup`)
- Ferramentas de build nativas do sistema
- (Opcional) `scripts/run-desktop.sh` e `scripts/stop-daemon.sh` para fluxo rapido

---

## Como rodar (desenvolvimento)

### Opcao A: somente daemon (API)

```bash
cargo run -p mlx-ollama-daemon
```

### Opcao B: daemon + desktop

#### Windows (PowerShell)

Terminal 1:

```powershell
cd g:\ai\mlx-ollama-pilot
cargo run -p mlx-ollama-daemon
```

Terminal 2:

```powershell
cd g:\ai\mlx-ollama-pilot\apps\desktop-ui\src-tauri
cargo tauri dev
```

Se estiver no **Developer Command Prompt for VS 2022** (`cmd`), use:

```cmd
cd /d g:\ai\mlx-ollama-pilot
cargo run -p mlx-ollama-daemon
```

```cmd
cd /d g:\ai\mlx-ollama-pilot\apps\desktop-ui\src-tauri
cargo tauri dev
```

No `cmd`, `cd g:\...` pode nao trocar a unidade. Use `cd /d ...` (ou rode `g:` antes).

#### macOS/Linux

```bash
./scripts/run-desktop.sh
```

Para parar o daemon:

```bash
./scripts/stop-daemon.sh
```

---

## Build de release

### Daemon

```bash
cargo build -p mlx-ollama-daemon --release
```

### Desktop (Tauri)

```bash
cd apps/desktop-ui/src-tauri
cargo tauri build
```

Artefatos esperados:
- Daemon (Windows): `target\release\mlx-ollama-daemon.exe`
- Daemon (Unix): `target/release/mlx-ollama-daemon`
- Bundle desktop: `apps/desktop-ui/src-tauri/target/release/bundle/...` (ex.: `.msi`, `.exe`, `.dmg`, `.deb`, `.AppImage`)

---

## Distribuicao para usuario final (instalador pronto)

Para o usuario nao precisar instalar Rust/Cargo:

1. Gere os artefatos de release (`cargo build --release` e `cargo tauri build`).
2. Publique os instaladores por plataforma (Windows/macOS/Linux).
3. Distribua em GitHub Releases com versionamento semantico.
4. Recomenda-se assinatura de codigo dos instaladores.

Fluxo de produto recomendado:
- O app desktop inicia o daemon automaticamente (sidecar/processo filho).
- O instalador entrega app ja compilado.
- Atualizacoes podem ser manuais (nova release) ou automaticas via updater do Tauri.

---

## Configuracao via variaveis de ambiente (daemon)

| Variavel | Padrao | Descricao |
|---|---|---|
| `APP_BIND_ADDR` | `127.0.0.1:11435` | Endereco e porta do daemon |
| `APP_LOCAL_PROVIDER` | `auto` | `auto`, `mlx`, `llamacpp` ou `ollama` |
| `APP_MODELS_DIR` | `/Users/kaike/models` | Pasta raiz de modelos locais |
| `APP_MLX_COMMAND` | `python3` | Comando base para inferencia |
| `APP_MLX_PREFIX_ARGS` | `-m mlx_lm.generate` | Args antes do modelo/prompt |
| `APP_MLX_SUFFIX_ARGS` | vazio | Args apos o prompt |
| `APP_MLX_TIMEOUT_SECS` | `900` | Timeout da inferencia |
| `APP_MLX_AIRLLM_ENABLED` | `true` | Ativa fallback de memoria para modelos grandes (orquestrado no Rust) |
| `APP_MLX_AIRLLM_THRESHOLD_PERCENT` | `70` | Percentual RAM fisica para ativar o fallback |
| `APP_MLX_AIRLLM_PYTHON_COMMAND` | `~/mlx-env/bin/python` (`python` no Windows) | Python usado pelo bridge do fallback |
| `APP_MLX_AIRLLM_RUNNER` | `scripts/mlx_airllm_bridge.py` | Script bridge executado no fallback |
| `APP_MLX_AIRLLM_BACKEND` | `auto` | Backend do bridge: `auto`, `original` (AirLLM) ou `legacy` (mlx_lm) |
| `APP_LLAMACPP_SERVER_BINARY` | `llama-server` | Binario do llama.cpp |
| `APP_LLAMACPP_BASE_URL` | `http://127.0.0.1:11439` | URL do servidor llama.cpp |
| `APP_LLAMACPP_AUTO_START` | `true` | Sobe `llama-server` automaticamente |
| `APP_LLAMACPP_AUTO_INSTALL` | `true` | Tenta instalar llama.cpp automaticamente |
| `APP_LLAMACPP_CONTEXT_SIZE` | `16384` | Context window |
| `APP_LLAMACPP_GPU_LAYERS` | `999` | Camadas na GPU |
| `APP_OLLAMA_BASE_URL` | `http://127.0.0.1:11434` | URL do Ollama |
| `APP_REMOTE_DOWNLOADS_DIR` | `/Users/kaike/models` | Destino dos downloads do catalogo |
| `APP_HF_API_BASE` | `https://huggingface.co` | Base da API Hugging Face |
| `APP_HF_PYTHON` | venv ou `python3` | Python para ferramentas HF |
| `APP_HF_TOKEN` | vazio | Token HF (modelos privados/gated) |
| `APP_CATALOG_SEARCH_LIMIT` | `18` | Limite da busca |
| `APP_CATALOG_DOWNLOAD_TIMEOUT_SECS` | `21600` | Timeout de download |

---

## Testar API rapidamente

Com o daemon rodando:

```bash
curl http://127.0.0.1:11435/health
curl http://127.0.0.1:11435/models
```

Exemplo de chat:

```bash
curl -X POST http://127.0.0.1:11435/chat \
  -H 'Content-Type: application/json' \
  -d '{
    "model_id": "Qwen3-Coder-30B-A3B-Instruct-MLX-4bit",
    "messages": [{"role":"user", "content":"Explique recursao em uma frase."}],
    "options": {"temperature":0.2, "max_tokens":128}
  }'
```

---

## Resumo rapido

- Backend: `crates/daemon`
- Providers: `crates/providers/*`
- UI desktop: `apps/desktop-ui`
- Execucao dev no Windows: dois terminais (`cargo run` + `cargo tauri dev`)
- Distribuicao final: `cargo tauri build` + publicacao dos instaladores
