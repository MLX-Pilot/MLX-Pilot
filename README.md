# MLX Pilot (Arquitetura Multi-Provider)

Projeto em Rust com foco em arquitetura de TCC para execução local de LLMs com **roteamento multi-provider**:
- **MLX** (Apple Silicon)
- **llama.cpp embutido** (cross-platform via `llama-server` gerenciado pelo daemon)
- **Ollama** (compatibilidade)

Também oferece descoberta/download de modelos e interface desktop.

---

## Conceitos que ajudam a entender o projeto

Se você não tem familiaridade com Rust, daemons ou APIs, esta seção resume o que você precisa saber.

### O que é um daemon?

Um **daemon** é um programa que fica rodando em segundo plano (sem janela), esperando pedidos. Aqui, o daemon é um **servidor HTTP**: ele escuta em um endereço (por exemplo `127.0.0.1:11435`) e responde a requisições como “liste os modelos” ou “gere uma resposta de chat”. A interface desktop e qualquer outro cliente conversam com o sistema **só** através desse servidor.

### O que é este projeto em termos de “camadas”?

1. **Backend (Rust)**  
   - Um servidor HTTP (daemon) que expõe endpoints como `/health`, `/models`, `/chat`, `/catalog/...`.  
   - Esse servidor usa **providers plugáveis** para listar modelos e executar inferência (MLX, llama.cpp embutido e Ollama).

2. **Catálogo remoto**  
   - Integração com Hugging Face: buscar modelos, ver detalhes, enfileirar downloads para uma pasta local e acompanhar o status.

3. **Interface desktop (Tauri)**  
   - App nativo (janela) que consome a API do daemon. Abas: **Chat** (conversar com um modelo) e **Descobrir Modelos** (buscar e baixar do catálogo).

### Workspace Rust (Cargo)

O projeto é um **workspace Cargo**: várias “crates” (bibliotecas/executáveis) em um mesmo repositório. O `Cargo.toml` na raiz declara os membros; cada pasta tem seu próprio `Cargo.toml`. Isso organiza o código em partes reutilizáveis e deixa a compilação e os testes mais simples.

---

## O que esta fase entrega

- **Daemon HTTP** em Rust com endpoints:
  - `GET /health` — saúde do serviço
  - `GET /models` — lista de modelos locais
  - `POST /chat` — chat (e `POST /chat/stream` para streaming)
  - `GET /catalog/sources` — fontes do catálogo
  - `GET /catalog/models` — busca de modelos no catálogo
  - `POST /catalog/downloads` — enfileirar download
  - `GET /catalog/downloads` e `GET /catalog/downloads/{job_id}` — status dos downloads
- **Provider MLX**: lista modelos em uma pasta local e executa inferência via CLI (`python3 -m mlx_lm.generate` por padrão).
- **Catálogo remoto (Hugging Face)**: busca por nome, task, likes, downloads e tamanho; download em 1 clique para pasta local, com fila de jobs e status.
- **UI desktop (Tauri + frontend estático)**: abas **Chat** e **Descobrir Modelos**.

---

## Arquitetura do repositório: o que é cada pasta

```
mlx-ollama-pilot/
├── Cargo.toml              # Workspace: define crates (core, providers, daemon)
├── crates/
│   ├── core/               # Contratos de domínio (tipos, erros, trait do provider)
│   ├── providers/
│   │   ├── mlx/            # Provider MLX
│   │   ├── llamacpp/       # Provider llama.cpp embutido (gerencia llama-server)
│   │   └── ollama/         # Provider Ollama (compatibilidade)
│   └── daemon/             # Servidor HTTP (Axum) e orquestração (chat, catálogo, etc.)
├── apps/
│   └── desktop-ui/         # App desktop Tauri + frontend (HTML/JS/CSS)
├── scripts/                # Scripts para subir/parar daemon e rodar o app desktop
└── target/                 # Gerado pelo Cargo (compilados, dependências) — pode ignorar
```

### Descrição de cada parte

| Pasta / arquivo | O que é | Para que serve |
|-----------------|---------|----------------|
| **`crates/core`** | Biblioteca Rust de **domínio** | Define contratos (tipos de chat, erros e trait `ModelProvider`) usados por qualquer backend de inferência. |
| **`crates/providers/mlx`** | Provider **MLX** | Executa modelos MLX locais via CLI (`mlx_lm.generate`). |
| **`crates/providers/llamacpp`** | Provider **llama.cpp embutido** | Descobre modelos `.gguf`, sobe/gerencia `llama-server` automaticamente por modelo e chama API OpenAI-compatible local (`/v1/chat/completions`). |
| **`crates/providers/ollama`** | Provider **Ollama** | Compatibilidade/fallback cross-platform via API local Ollama. |
| **`crates/daemon`** | **Servidor HTTP** (executável) | Expõe `/health`, `/models`, `/chat`, `/chat/stream`, `/web/brave/search`, `/openclaw/...`, `/catalog/...` e roteia dinamicamente para o provider correto. |
| **`apps/desktop-ui`** | **App desktop** (Tauri) | `src-tauri/`: código Rust do shell Tauri (janela, permissões). `ui/`: frontend estático (HTML, JS, CSS) que chama a API do daemon em `127.0.0.1:11435`. Abas Chat e Descobrir Modelos. |
| **`scripts/`** | Scripts de conveniência | `run-desktop.sh`: sobe o daemon e abre o app desktop. `stop-daemon.sh`: encerra o processo do daemon. Úteis para quem não quer rodar daemon e Tauri manualmente. |
| **`target/`** | Saída do Cargo | Compilados e artefatos. Não versionar; pode ser apagado (o Cargo recria). |

### Fluxo resumido

- **Frontend (desktop-ui)** → HTTP → **Daemon** → **Provider MLX** (lista modelos, chama Python para inferência) e **Catálogo** (Hugging Face, downloads).
- O **core** não depende do daemon nem do provider; o daemon e o provider dependem do core.

---

## Requisitos para desenvolver e rodar (ex.: Mac mini M4)

- **Rust**: toolchain instalado (ex.: `rustup`). Usado para compilar o daemon e o app Tauri.
- **Python** com `mlx-lm` no ambiente que o daemon usa (o daemon chama `python3 -m mlx_lm.generate` ou comando configurado).
- **Modelos locais**: por padrão esperados em `/Users/kaike/models` (configurável por variável de ambiente).

Não é necessário ter um executável pré-compilado: o projeto é rodado a partir do código-fonte com `cargo run` e `cargo tauri dev`.

---

## Como rodar o projeto (sem executável pré-compilado)

O projeto **não** é distribuído como um único executável; você compila e roda a partir do código. Duas formas típicas:

### Opção 1: Só o daemon (API)

No diretório do projeto:

```bash
cd /Users/kaike/mlx-ollama-pilot
cargo run -p mlx-ollama-daemon
```

- `cargo run` compila (se precisar) e executa.
- `-p mlx-ollama-daemon` indica o pacote do daemon (em `crates/daemon`).
- O daemon sobe em `127.0.0.1:11435` (por padrão). Você pode testar com `curl` (veja “Testar API” mais abaixo).

### Opção 2: Daemon + app desktop (recomendado para uso diário)

Use o script que sobe o daemon e abre a interface:

```bash
cd /Users/kaike/mlx-ollama-pilot
./scripts/run-desktop.sh
```

O script:

1. Garante que não há outro daemon na porta 11435 (reinicia se precisar).
2. Sobe o daemon em background com `cargo run -p mlx-ollama-daemon`.
3. Espera o daemon ficar saudável e abre o app Tauri com `cargo tauri dev` (em `apps/desktop-ui/src-tauri`).

Para parar o daemon depois:

```bash
cd /Users/kaike/mlx-ollama-pilot
./scripts/stop-daemon.sh
```

**Resumo:** se ainda não está compilado em um executável, use `cargo run -p mlx-ollama-daemon` para só a API, ou `./scripts/run-desktop.sh` para API + interface desktop.

---

## Configuração do daemon (variáveis de ambiente)

| Variável | Default | Descrição |
|----------|---------|-----------|
| `APP_BIND_ADDR` | `127.0.0.1:11435` | Endereço e porta em que o daemon escuta |
| `APP_LOCAL_PROVIDER` | `auto` | `auto`, `mlx`, `llamacpp` ou `ollama` |
| `APP_MODELS_DIR` | `/Users/kaike/models` | Pasta raiz de modelos locais |
| `APP_MLX_COMMAND` | `python3` | Comando base para inferência |
| `APP_MLX_PREFIX_ARGS` | `-m mlx_lm.generate` | Argumentos antes do modelo/prompt |
| `APP_MLX_SUFFIX_ARGS` | (vazio) | Argumentos após |
| `APP_MLX_TIMEOUT_SECS` | `900` | Timeout da inferência em segundos |
| `APP_LLAMACPP_SERVER_BINARY` | `llama-server` | Binário do servidor llama.cpp |
| `APP_LLAMACPP_BASE_URL` | `http://127.0.0.1:11439` | URL do servidor embutido llama.cpp |
| `APP_LLAMACPP_AUTO_START` | `true` | Sobe o `llama-server` automaticamente |
| `APP_LLAMACPP_AUTO_INSTALL` | `true` | Tenta instalar llama.cpp automaticamente (best-effort) |
| `APP_LLAMACPP_CONTEXT_SIZE` | `16384` | Context window do `llama-server` |
| `APP_LLAMACPP_GPU_LAYERS` | `999` | Camadas na GPU para llama.cpp |
| `APP_OLLAMA_BASE_URL` | `http://127.0.0.1:11434` | URL Ollama |
| `APP_REMOTE_DOWNLOADS_DIR` | `/Users/kaike/models` | Pasta onde os downloads do catálogo vão |
| `APP_HF_API_BASE` | `https://huggingface.co` | Base da API Hugging Face |
| `APP_HF_PYTHON` | (venv se existir, senão `python3`) | Python para ferramentas HF |
| `APP_HF_TOKEN` | (vazio) | Token HF para modelos gated/privados |
| `APP_CATALOG_SEARCH_LIMIT` | `18` | Limite de resultados na busca do catálogo |
| `APP_CATALOG_DOWNLOAD_TIMEOUT_SECS` | `21600` | Timeout de download do catálogo |

---

## Testar a API

Com o daemon rodando:

```bash
curl http://127.0.0.1:11435/models
```

```bash
curl -X POST http://127.0.0.1:11435/chat \
  -H 'Content-Type: application/json' \
  -d '{
    "model_id": "Qwen3-Coder-30B-A3B-Instruct-MLX-4bit",
    "messages": [{"role":"user", "content":"Explique recursao em uma frase."}],
    "options": {"temperature":0.2, "max_tokens":128}
  }'
```

```bash
curl 'http://127.0.0.1:11435/catalog/models?source=huggingface&query=mlx%20qwen&limit=5'
```

```bash
curl -X POST http://127.0.0.1:11435/catalog/downloads \
  -H 'Content-Type: application/json' \
  -d '{
    "source": "huggingface",
    "model_id": "hf-internal-testing/tiny-random-gpt2",
    "allow_patterns": ["*.json", "*.safetensors"]
  }'
```

```bash
curl http://127.0.0.1:11435/catalog/downloads
```

---

## Resumo para o time

- **O que é cada pasta:** `crates/core` = domínio; `crates/providers/*` = backends de inferência; `crates/daemon` = servidor HTTP; `apps/desktop-ui` = app desktop; `scripts/` = atalhos para rodar/parar.
- **Como rodar sem executável pronto:** `cargo run -p mlx-ollama-daemon` (só API) ou `./scripts/run-desktop.sh` (API + app desktop).
- **Parar o daemon:** `./scripts/stop-daemon.sh`.

Com isso, qualquer pessoa do time consegue entender a arquitetura e rodar o projeto a partir do código-fonte.
