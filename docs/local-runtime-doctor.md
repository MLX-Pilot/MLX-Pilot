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

## Compatibilidade obrigatória do Ollama

O MLX-Pilot instala o Ollama **0.30.7** e aceita servidores estáveis no
intervalo **>= 0.30.0 e < 0.31.0**. A versão mais recente publicada não é
necessariamente compatível. A política fica em
`crates/providers/ollama/src/runtime.rs`: ao atualizar a versão aprovada,
revise também os limites de compatibilidade e os testes do provedor.

A validação usa `GET /api/version` antes de listar ou excluir modelos e
antes de gerar respostas, incluindo streaming, URLs configuradas manualmente
e conexões que já estavam prontas. Versões incompatíveis, pré-lançamentos
e respostas de versão inválidas são recusados antes da operação. Servidores
configurados manualmente devem ser atualizados pelo responsável pelo endpoint.

Com `APP_OLLAMA_AUTO_INSTALL=true` e `APP_OLLAMA_AUTO_START=true` (padrões),
o runtime gerenciado substitui uma instalação de sistema incompatível pelo
pacote aprovado. O pacote depende do sistema e da arquitetura: TGZ no macOS,
TAR.ZST no Linux e ZIP no Windows, com binários para ARM64 e x86-64.
Bibliotecas e permissões de execução são preservadas. O pacote extraído só
é ativado depois de confirmar a versão exata 0.30.7; uma instalação
interrompida preserva o manifesto ativo anterior.

O runtime fica em `APP_OLLAMA_RUNTIME_ROOT` ou na pasta padrão de dados do
MLX-Pilot (`~/.local/share/mlx-pilot/runtime/ollama` no macOS/Linux).
`OLLAMA_MODELS` continua controlando a pasta dos modelos. A instalação
gerenciada não altera automaticamente o executável global no PATH nem
encerra servidores externos para liberar portas.

O endpoint `/runtime/startup` informa a versão detectada, a prontidão do
provedor e o motivo de uma falha. O arquivo `runtime-state.json`, dentro da
pasta do runtime, registra também `required_version` e `installed_version`.
Desativar a instalação automática não desativa a exigência de compatibilidade.

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
