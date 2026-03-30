#!/usr/bin/env zsh
set -euo pipefail

if [ -f /tmp/mlx-ollama-daemon.pid ]; then
  PID=$(cat /tmp/mlx-ollama-daemon.pid)
  if ps -p "$PID" >/dev/null 2>&1; then
    kill "$PID"
    echo "Daemon finalizado (PID $PID)."
  else
    echo "PID registrado nao esta ativo."
  fi
  rm -f /tmp/mlx-ollama-daemon.pid
else
  echo "Nenhum PID de daemon encontrado."
fi
