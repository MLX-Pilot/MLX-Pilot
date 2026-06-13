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
- App nativo com abas de Chat, Modelos/Descobrir, Agent, IA Visual, Console, Historico, Memoria, Comparar, Deep Research, Hardware e Settings.
- Frontend em ES Modules nativos (`ui/js/core`, `ui/js/features`, `ui/css`), servido estaticamente pelo Tauri, sem bundler/passo de build de frontend.

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
- Compatibility Matrix automatizada para o agente Hermes.
- API dedicada do agente:
- `POST /agent/run`
- `POST /agent/stream` (stub para streaming de eventos)
- `GET /agent/providers`
- `GET/POST /agent/config`
- `GET /agent/skills`
- `POST /agent/skills/reload`
- `GET /agent/tools`
- `GET /agent/compat/report`
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
- Control Plane completo para channels, plugins, skills, tools/policies, context/memory e runtime/health.
- Chat do agente integrado ao fluxo principal do MLX-Pilot.

### Validacao do Agente

- Estado atual validado por `GET /agent/compat/report`.
- UI smoke validations:

```bash
cd apps/desktop-ui
npm run test:e2e:channels-smoke
npm run test:e2e:skills-smoke
npm run test:e2e:agent-control-plane
npm run test:e2e:agent-workspace-ui
```

---

## Estrutura do repositorio

```text
mlx-pilot/
|-- Cargo.toml                  # workspace Rust
|-- crates/
|   |-- core/                   # contratos de dominio (tipos, trait ModelProvider)
|   |-- daemon/                 # servidor HTTP (o "daemon")
|   |-- agent-core/             # agent loop, policy, approval, audit, skills runtime
|   |-- agent-tools/            # ferramentas (read/write/edit/exec) + sandbox de IO
|   |-- agent-skills/           # parser/loader de skills
|   |-- research/               # deep research (Wave 5)
|   |-- hardware-fit/           # analise de hardware (Wave 5)
|   |-- model-fit/              # recomendacao de modelos (Wave 5)
|   '-- providers/
|       |-- mlx/                # Apple Silicon
|       |-- llamacpp/           # llama.cpp embutido
|       |-- ollama/             # compatibilidade Ollama
|       '-- http_llm_provider/  # remoto (OpenAI/Anthropic/...)
|-- apps/
|   '-- desktop-ui/
|       |-- ui/                 # frontend estatico (ESM: js/core, js/features, css/)
|       |-- e2e/                # testes e2e (JSDOM)
|       '-- src-tauri/          # app Tauri (embute e sobe o daemon)
'-- scripts/                    # conveniencia (run-desktop.sh, fetch-llama-engine.ps1, ...)
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
| `crates/research` | Deep Research (Wave 5). |
| `crates/hardware-fit` | Analise de hardware e fit de modelos (Wave 5). |
| `crates/model-fit` | Heuristicas de recomendacao de modelos (Wave 5). |
| `crates/daemon` | Servidor HTTP principal (o daemon). |
| `apps/desktop-ui` | App desktop Tauri + frontend estatico (ESM). O app embute e sobe o daemon. |
| `scripts` | Scripts de conveniencia (`run-desktop.sh`, `stop-daemon.sh`, `fetch-llama-engine.ps1`). |

---

## Requisitos

Para rodar em **modo desenvolvimento** (direto do codigo, sem gerar instalador), o
app desktop **ja sobe o daemon embutido sozinho** — voce nao precisa de um
terminal separado para o daemon nem da CLI do Tauri.

### Comum a todos os sistemas

- Rust estavel via [rustup](https://rustup.rs)
- Git
- (Opcional) Node.js >= 18 — apenas para os atalhos `npm run ...` e os testes e2e
- (Opcional) Python com `mlx-lm` — apenas para inferencia MLX em Apple Silicon

> A CLI do Tauri (`tauri-cli`) NAO e necessaria para rodar em dev. Ela so e usada
> para gerar instalador (`cargo tauri build`, ver "Build de release").

### Windows

```powershell
winget install -e --id Rustlang.Rustup
winget install -e --id Microsoft.VisualStudio.2022.BuildTools
```

No instalador do Build Tools selecione: `Desktop development with C++`,
`MSVC v143` e `Windows 10/11 SDK`.

A WebView (WebView2) ja vem no Windows 11. No Windows 10, instale o
"Evergreen WebView2 Runtime" da Microsoft, se ainda nao tiver.

Reabra o terminal e valide (se `cargo` nao estiver no PATH, rode antes
`$env:Path += ";$env:USERPROFILE\.cargo\bin"`):

```powershell
rustc --version
cargo --version
```

### macOS

```bash
xcode-select --install                                    # toolchain de build
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

A WebView (WKWebView) ja faz parte do macOS — nada extra a instalar.
Em Apple Silicon, para usar MLX, tenha Python + `mlx-lm` no ambiente do daemon.

### Linux

Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Dependencias de sistema do Tauri 2 (WebKitGTK e afins):

```bash
# Debian / Ubuntu
sudo apt update
sudo apt install -y build-essential curl wget file pkg-config libssl-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev librsvg2-dev \
  libayatana-appindicator3-dev libsoup-3.0-dev
```

```bash
# Fedora
sudo dnf install gtk3-devel webkit2gtk4.1-devel libsoup3-devel \
  librsvg2-devel openssl-devel curl wget file

# Arch
sudo pacman -S --needed base-devel webkit2gtk-4.1 gtk3 libsoup3 librsvg openssl
```

---

## Como rodar (desenvolvimento, sem gerar executavel)

Clone o repositorio e entre nele:

```bash
git clone https://github.com/MLX-Pilot/MLX-Pilot.git mlx-pilot
cd mlx-pilot
```

### App desktop completo (recomendado) — um unico comando

Compila e abre a janela do app. O **daemon HTTP sobe junto, embutido** (porta
padrao `127.0.0.1:11435`; se ela estiver ocupada, o app escolhe outra porta livre
automaticamente). O mesmo comando funciona em **Windows, macOS e Linux**, a
partir da raiz do repo:

```bash
cargo run -p mlx-ollama-desktop
```

Equivalente, usando os atalhos npm (de dentro de `apps/desktop-ui`):

```bash
cd apps/desktop-ui
npm run desktop:dev
```

> A primeira compilacao baixa/compila o workspace e pode levar alguns minutos;
> as execucoes seguintes sao rapidas.
> Na primeira vez o daemon tenta provisionar o motor `llama.cpp` automaticamente
> (`APP_LLAMACPP_AUTO_INSTALL=true`). Para desativar, exporte
> `APP_LLAMACPP_AUTO_INSTALL=false` antes de rodar.

### Somente o daemon (API HTTP, sem janela)

Util para testar a API ou plugar outro frontend:

```bash
cargo run -p mlx-ollama-daemon
```

Sobe em `http://127.0.0.1:11435` — veja "Testar API rapidamente".

### Atalho macOS/Linux (opcional)

`scripts/run-desktop.sh` e `scripts/stop-daemon.sh` automatizam o fluxo no
macOS/Linux (variaveis de ambiente, checagem de porta e logs em arquivo). Ajuste
a variavel `ROOT_DIR` no topo do script para o caminho do seu clone antes de usar.

---

## Build de release

### Daemon

```bash
cargo build -p mlx-ollama-daemon --release
```

### Desktop (Tauri)

Apenas para empacotar (nao e preciso para rodar em dev), instale a CLI do Tauri
uma vez:

```bash
cargo install tauri-cli --locked
```

Depois gere o bundle:

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

- Backend (daemon HTTP): `crates/daemon`
- Providers: `crates/providers/*`
- UI desktop (embute o daemon): `apps/desktop-ui`
- Rodar em dev (Windows/macOS/Linux): `cargo run -p mlx-ollama-desktop` (ou `npm run desktop:dev`) — o daemon sobe junto, embutido, sem terminal separado
- So a API HTTP: `cargo run -p mlx-ollama-daemon`
- Distribuicao final: `cargo tauri build` + publicacao dos instaladores
