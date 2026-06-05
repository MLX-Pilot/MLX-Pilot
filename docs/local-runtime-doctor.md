# Local Runtime Doctor

O daemon agora expõe um diagnóstico nativo do runtime local em:

- `GET /runtime/doctor`
- `POST /runtime/doctor`

## Objetivo

O endpoint detecta e consolida:

- sistema operacional, arquitetura e GPU
- saúde e versão de `ollama`, `mlx`, `llama.cpp` e Python
- modelos locais disponíveis
- backend ativo recomendado
- fallback chain
- smoke test do backend ativo

Além da resposta JSON, um snapshot do relatório é salvo em:

- `%TEMP%/mlx-pilot-runtime-doctor-report.json` no Windows
- `${TMPDIR:-/tmp}/mlx-pilot-runtime-doctor-report.json` em macOS/Linux

## Comportamento de fallback

### macOS Apple Silicon

Prioridade:

1. `mlx`
2. `llama.cpp`
3. `ollama`
4. `cpu`

### Windows e Linux não-Apple

Prioridade:

1. `llama.cpp`
2. `ollama`
3. `cpu`

`MLX` não deve ser escolhido implicitamente em Windows.

## Request opcional

`POST /runtime/doctor` aceita:

```json
{
  "apply_fixes": true,
  "allow_updates": false,
  "run_validation": true,
  "validation_model": "qwen3.5:9b"
}
```

## Auto-fixes seguros deste ciclo

- bootstrap do Ollama quando instalado mas offline
- normalização do Python do AIRLLM para `py` em Windows
- remoção do fallback implícito incorreto para `MLX` em hosts não suportados
- correção da auto-instalação do `llama.cpp` no provider Windows

## Comandos manuais úteis

### Ollama

```powershell
ollama --version
ollama list
ollama run qwen3.5:9b "Reply with exactly OK."
```

### llama.cpp no Windows

```powershell
winget install --id ggml.llama.cpp -e
```

### Python alias do Windows

Use `py` em vez de `python` enquanto o alias da Microsoft Store estiver interceptando o comando.

### MLX dylib issue no macOS

Se o doctor reportar sinais de `dyld` ou `dlopen`, valide o Python ativo e reinstale:

```bash
python3 -m pip install --upgrade --force-reinstall mlx mlx-lm
```
