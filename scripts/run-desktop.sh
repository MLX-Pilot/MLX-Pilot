#!/usr/bin/env zsh
set -euo pipefail

ROOT_DIR="/Users/kaike/mlx-ollama-pilot"
DAEMON_LOG="/tmp/mlx-ollama-daemon.log"
SERVICE_LABEL="com.kaike.mlx-ollama-daemon"
USER_ID="$(id -u)"

source "$HOME/.cargo/env"

export APP_LOCAL_PROVIDER="${APP_LOCAL_PROVIDER:-auto}"
export APP_LLAMACPP_AUTO_INSTALL="${APP_LLAMACPP_AUTO_INSTALL:-true}"
export APP_LLAMACPP_AUTO_START="${APP_LLAMACPP_AUTO_START:-true}"

if [ -x "$ROOT_DIR/bin/llama-server" ]; then
  export APP_LLAMACPP_SERVER_BINARY="$ROOT_DIR/bin/llama-server"
fi

if [ -x "/Users/kaike/mlx-env/bin/mlx_lm.generate" ]; then
  export APP_MLX_COMMAND="/Users/kaike/mlx-env/bin/mlx_lm.generate"
  export PATH="/Users/kaike/mlx-env/bin:$PATH"
fi

if [ "$APP_LOCAL_PROVIDER" = "auto" ] || [ "$APP_LOCAL_PROVIDER" = "llamacpp" ]; then
  LLAMA_BIN="${APP_LLAMACPP_SERVER_BINARY:-llama-server}"
  if ! command -v "$LLAMA_BIN" >/dev/null 2>&1 && [ "$APP_LLAMACPP_AUTO_INSTALL" = "true" ]; then
    if command -v brew >/dev/null 2>&1; then
      echo "llama-server nao encontrado. Tentando instalar llama.cpp via Homebrew..."
      brew list llama.cpp >/dev/null 2>&1 || brew install llama.cpp || true
    fi
  fi
fi

cd "$ROOT_DIR"

if launchctl print "gui/${USER_ID}/${SERVICE_LABEL}" >/dev/null 2>&1; then
  echo "Desativando LaunchAgent ${SERVICE_LABEL} para evitar conflito de porta..."
  launchctl bootout "gui/${USER_ID}/${SERVICE_LABEL}" >/dev/null 2>&1 || true
  sleep 1
fi

RUNNING_PIDS=$(lsof -t -nP -iTCP:11435 -sTCP:LISTEN 2>/dev/null || true)
if [ -n "$RUNNING_PIDS" ]; then
  echo "Reiniciando daemon em 127.0.0.1:11435..."
  kill $RUNNING_PIDS >/dev/null 2>&1 || true
  sleep 1
fi

echo "Iniciando daemon..."
nohup cargo run -p mlx-ollama-daemon > "$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!
echo "$DAEMON_PID" > /tmp/mlx-ollama-daemon.pid
sleep 1

if ! kill -0 "$DAEMON_PID" >/dev/null 2>&1; then
  echo "Processo do daemon encerrou logo apos iniciar. Veja: $DAEMON_LOG"
  tail -n 60 "$DAEMON_LOG" || true
  exit 1
fi

READY=0
for _ in {1..30}; do
  if ! kill -0 "$DAEMON_PID" >/dev/null 2>&1; then
    echo "Processo do daemon encerrou durante a inicializacao. Veja: $DAEMON_LOG"
    tail -n 80 "$DAEMON_LOG" || true
    exit 1
  fi
  if curl -sSf http://127.0.0.1:11435/health >/dev/null 2>&1; then
    READY=1
    break
  fi
  sleep 1
done

if [ "$READY" -ne 1 ]; then
  echo "Falha ao iniciar daemon dentro do timeout (30s). Veja: $DAEMON_LOG"
  tail -n 80 "$DAEMON_LOG" || true
  exit 1
fi

STREAM_STATUS=$(curl -sS -o /tmp/mlx-ollama-chat-stream-check.out -w "%{http_code}" \
  -X POST http://127.0.0.1:11435/chat/stream \
  -H "Content-Type: application/json" \
  -d '{"model_id":"","messages":[{"role":"user","content":"ping"}],"options":{}}' || true)

if [ "$STREAM_STATUS" = "404" ]; then
  echo "Servico em 127.0.0.1:11435 nao tem /chat/stream. Porta pode estar com daemon antigo."
  echo "Body recebido:"
  cat /tmp/mlx-ollama-chat-stream-check.out || true
  exit 1
fi

echo "Abrindo app desktop (Tauri)..."
cd "$ROOT_DIR/apps/desktop-ui/src-tauri"
cargo tauri dev
