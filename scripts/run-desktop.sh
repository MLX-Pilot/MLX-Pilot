#!/usr/bin/env zsh
set -euo pipefail

ROOT_DIR="/Users/kaike/mlx-pilot"
DAEMON_LOG="/tmp/mlx-ollama-daemon.log"

source "$HOME/.cargo/env"

if [ -x "/Users/kaike/mlx-env/bin/mlx_lm.generate" ]; then
  export APP_MLX_COMMAND="/Users/kaike/mlx-env/bin/mlx_lm.generate"
  export PATH="/Users/kaike/mlx-env/bin:$PATH"
fi

cd "$ROOT_DIR"

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
sleep 2

if ! kill -0 "$DAEMON_PID" >/dev/null 2>&1; then
  echo "Processo do daemon encerrou logo apos iniciar. Veja: $DAEMON_LOG"
  tail -n 60 "$DAEMON_LOG" || true
  exit 1
fi

if ! curl -sSf http://127.0.0.1:11435/health >/dev/null; then
  echo "Falha ao iniciar daemon. Veja: $DAEMON_LOG"
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
