<p align="center">
  <img src="apps/desktop-ui/ui/assets/mlxpilot-wordmark.png" alt="MLX Pilot" />
</p>

# MLX Pilot (Arquitetura Multi-Provider)

Projeto em Rust para execucao local de LLMs com roteamento multi-provider:
- MLX (Apple Silicon)
- llama.cpp embutido (cross-platform via `llama-server` gerenciado pelo daemon)
- Ollama (compatibilidade)

Tambem oferece descoberta/download de modelos e interface desktop (Tauri).

## Presentation Studio e plugins WASM

A aba **Plugins** é a central de ferramentas: cada plugin pode ter backend WASM,
frontend HTML/CSS/JS e ferramentas disponíveis para a IA. O **Presentation Studio**
vem incluído e abre na aba **Apresentações**.

### Adicionar, ativar, desativar e remover

| Ação | Como fazer na interface |
| --- | --- |
| Adicionar | Abra **Plugins → Adicionar plugin**, selecione um arquivo `.mlxplugin.json`, revise as permissões, marque as que deseja conceder e clique em **Instalar plugin**. Para ativar, todas as permissões declaradas são necessárias. |
| Abrir | Clique em **Abrir** no cartão do plugin. O frontend próprio abre na central; o Presentation Studio abre em **Apresentações**. |
| Desativar | Clique em **Desativar**. A interface é fechada/bloqueada e novas chamadas às ferramentas são recusadas. A instalação e os documentos permanecem salvos. |
| Ativar | Clique em **Ativar** para liberar novamente a interface e as ferramentas autorizadas. |
| Remover | Clique em **Remover** e confirme. O pacote é desinstalado; documentos e revisões são preservados. A remoção continua valendo após reiniciar o aplicativo. |
| Reinstalar o Studio | Use **Reinstalar Presentation Studio**, disponível na central após removê-lo. Os projetos anteriores voltam a ficar acessíveis. |

Instale somente pacotes cuja origem você conhece. A revisão de permissões acontece
antes da instalação. Plugins de terceiros executam ferramentas em WASM; seus
frontends ficam em um iframe isolado, sem acesso ao DOM principal, credenciais ou
requisições de rede pelo SDK. A comunicação com o backend passa pelo host e só
pode chamar ferramentas do próprio plugin, respeitando os grants e o estado ativo.

### Criar apresentações e exportar PDF/PPTX

1. Abra **Plugins → Presentation Studio → Abrir** e clique em **+ Nova**.
2. Escolha layouts, defina o briefing e a paleta em **Design**. Use **＋ Imagens**
   para enviar PNG/JPEG/WebP; em **Imagens**, descreva cada referência, insira-a no
   slide ou extraia uma cor com **Paleta**. Também é possível importar uma paleta.
3. Em **Chat**, selecione um modelo instalado e **Criar apresentação completa**.
   Basta escrever “Uma apresentação sobre jardins urbanos”. A IA define público,
   roteiro e direção de arte, escreve cada slide e faz uma revisão editorial.
   São 5 slides por padrão, ou até 12 por pedido. O progresso mostra cada etapa;
   a criação pode levar alguns minutos e pode ser cancelada.
   **Acrescentar slides** preserva o projeto atual. **Refinar apresentação atual**
   permite pedir ajustes específicos. Criar em um projeto já preenchido salva uma
   nova apresentação, preservando a anterior.
4. Edite com o mouse (arrastar, redimensionar, texto e propriedades) ou abra
   **Código e validação** para editar HTML, CSS, JavaScript ou o documento JSON.
   Há desfazer/refazer, salvamento automático e histórico de revisões.
5. Escolha **PDF**, **PPTX editável**, **HTML animado** ou **Projeto JSON** e clique
   em **Exportar**. Use **Apresentar** para testar a reprodução.

A direção visual combina sete composições (capa, divisão em colunas, cartões,
processo, afirmação, encerramento e gráfico) com estilos editorial, minimalista ou
marcante. A paleta escolhida é mantida, salvo pedido explícito para alterá-la. Peça,
por exemplo, “estilo editorial, animações suaves, sem estatísticas inventadas”.
Imagens enviadas são usadas pelos IDs e descrições; sem imagens, o editor compõe
com tipografia e formas. As premissas inferidas ficam nas notas do apresentador.

Antes de salvar, todos os slides são renderizados e verificados quanto a cortes,
sobreposição de textos e contraste em fundos sólidos. O modelo recebe uma rodada
de reparo para problemas visuais. Se o problema persistir, o projeto anterior é
preservado. PDF/PPTX bloqueiam exportações com erros detectados; avisos de densidade
ou contraste não verificável aparecem em **Código e validação → Validar**.
CSS personalizado e imagens de fundo ainda exigem conferência visual humana.
Essas verificações reduzem defeitos mensuráveis; não substituem revisão editorial.

O PDF é exportado em 2560 × 1440 pixels por página e preserva o visual como imagens.
O PPTX contém elementos nativos editáveis
(textos, imagens, gráficos e tabelas); a opção **PPTX como imagens** preserva CSS
personalizado. Animações e transições são reproduzidas no HTML, enquanto PDF e
PPTX são estáticos. A exportação usa o documento estruturado original; não depende
de extrair elementos de um PDF já rasterizado.

### Modelo local recomendado para Mac M4 com 24 GB

O perfil usado nesta demonstração é **Qwen 3.5 9B**, com contexto de **8192 tokens**
e raciocínio estendido desativado no chat do Studio. O modelo padrão ocupa cerca de
6,6 GB, deixando espaço para macOS, editor e exportação. O consumo total varia
com contexto, imagens e concorrência. Consulte o [modelo oficial](https://ollama.com/library/qwen3.5).

Com o MLX-Pilot aberto e seu Ollama gerenciado em execução:

```bash
OLLAMA_HOST=http://127.0.0.1:11438 ollama pull qwen3.5:9b
```

Depois clique em **Chat → Atualizar modelos** no Studio. O aplicativo valida a
compatibilidade do Ollama e instala a versão aprovada quando necessário.

### Desenvolver um plugin com backend e frontend

Um plugin reúne ferramentas de computação em WASM e, opcionalmente, uma interface
HTML/CSS/JavaScript. O mesmo backend atende aos botões da interface e às chamadas
da IA. O SDK gera o manifesto e empacota tudo em um `.mlxplugin.json`; instalar
esse arquivo conecta o plugin ao MLX-Pilot, sem um servidor separado por plugin.

#### 1. Gerar o projeto

Para desenvolver, use Node.js 20+, npm,
[Extism JavaScript PDK](https://github.com/extism/js-pdk) (`extism-js`) e
[Binaryen](https://github.com/WebAssembly/binaryen) (`wasm-opt` e `wasm-merge`) no
PATH. Quem apenas instala um pacote pronto não precisa dessas ferramentas.

Execute a partir da raiz deste repositório:

```bash
npm ci --prefix packages/plugin-sdk
node packages/plugin-sdk/src/cli.mjs init meu_plugin
cd meu_plugin
npm install
```

O scaffold já vem com uma ferramenta funcional de soma e uma tela para usá-la:

```text
meu_plugin/
├── package.json       # scripts de build/pack e dependência do SDK local
└── src/
    ├── index.ts       # definição do plugin e ferramentas compiladas para WASM
    ├── index.html     # estrutura da interface
    ├── frontend.css   # aparência da interface
    └── frontend.ts    # eventos da tela e chamadas ao backend
```

O SDK é referenciado por uma dependência `file:` deste checkout. Não é necessário
buscar `@mlx-pilot/plugin-sdk` em um registro npm; ao mover o projeto para outro
computador, ajuste essa dependência para o checkout correspondente.

#### 2. Definir ferramentas, entradas e permissões

Edite `src/index.ts`. Este exemplo mantém os nomes usados pelo scaffold:

```ts
import { definePlugin, defineTool } from '@mlx-pilot/plugin-sdk';

export default definePlugin({
  id: 'meu_plugin',
  name: 'Meu plugin',
  version: '1.0.0',
  permissions: ['compute'],
  frontend: {
    html: 'src/index.html',
    script: 'src/frontend.ts',
    style: 'src/frontend.css',
  },
  tools: [defineTool({
    name: 'summarize_numbers',
    description: 'Calcule a quantidade e a soma dos números fornecidos pelo usuário.',
    parameters: {
      type: 'object',
      properties: {
        values: { type: 'array', items: { type: 'number' }, maxItems: 1000 },
      },
      required: ['values'],
      additionalProperties: false,
    },
    mutating: false,
    execute({ values }: { values: number[] }) {
      return { count: values.length, sum: values.reduce((a, b) => a + b, 0) };
    },
  })],
});
```

O `id` do plugin e os nomes das ferramentas usam `snake_case`, com 1–30 caracteres.
A `description` explica à IA quando usar a ferramenta; `parameters` declara as
entradas em JSON Schema, validadas pelo host. `execute` recebe esse objeto e retorna
JSON de forma síncrona. Marque `mutating: true` em ferramentas que alterem estado;
o agente usa essa informação para bloquear mutações nos modos ReadOnly/DryRun.

Use `compute` para funções de cálculo/transformação. A API v1 também reconhece
`documents:read` e `documents:write`, usadas pelo broker do Presentation Studio;
declará-las em um plugin de terceiros não concede acesso a arquivos ou aos
documentos do Studio. O guest WASM não recebe Node.js, DOM, `fetch`, shell ou
credenciais. A API atual de terceiros também não oferece uma ponte para serviços
HTTP externos.

#### 3. Conectar a interface ao backend

O `frontend` acima faz o build incluir os três assets no pacote. O HTML contém os
elementos da tela; o CSS define o visual; o TypeScript trata eventos e usa a ponte
do SDK. Dentro de um evento, por exemplo:

```ts
import { createFrontendClient } from '@mlx-pilot/plugin-sdk';

const host = createFrontendClient();
document.getElementById('calculate')!.onclick = async () => {
  const output = document.getElementById('result')!;
  try {
    const result = await host.invoke('summarize_numbers', { values: [10, 20, 30] });
    output.textContent = JSON.stringify(result); // {"count":3,"sum":60}
  } catch (error) {
    output.textContent = String(error);
  }
};
```

Os IDs `calculate` e `result` já existem no HTML gerado. O scaffold também mostra
como ler os valores do campo de entrada. `host.invoke` retorna uma Promise; o host
confere ativação, permissões e schema antes de executar o WASM. Use `host.dispose()`
ao encerrar um cliente que você gerencie para liberar listeners e chamadas pendentes.

O fluxo é **interface → ponte do SDK → daemon → ferramenta WASM → resultado**.
O frontend roda em iframe isolado e só pode chamar ferramentas do próprio plugin.
Não configure uma URL de backend na tela: o aplicativo encaminha a chamada para
seu daemon local. Inclua os assets no pacote; CDNs e requisições de rede ficam
bloqueadas nesse iframe. Abrir `index.html` diretamente no navegador não fornece
a ponte do host; teste a conexão abrindo o plugin dentro do MLX-Pilot.

#### 4. Testar, compilar e empacotar

Na pasta `meu_plugin`, crie `fixtures.json` com entradas e resultados esperados:

```json
[
  {
    "tool": "summarize_numbers",
    "input": { "values": [10, 20, 30] },
    "expected": { "count": 3, "sum": 60 }
  }
]
```

Depois execute, ainda nessa pasta:

```bash
npx --no-install mlx-plugin test fixtures.json
npm run build
npm run pack
```

O teste de fixtures verifica a lógica JavaScript local. O build gera
`dist/plugin.json` e `dist/plugin.wasm`, incluindo o frontend no manifesto; o pack
gera **`meu_plugin-1.0.0.mlxplugin.json` na raiz do plugin**. Para conferir também
a execução WASM e a integração da tela, instale o pacote e teste pelo aplicativo.

#### 5. Instalar, conectar à IA e atualizar

1. Com o MLX-Pilot aberto, vá a **Plugins → Adicionar plugin** e selecione
   `meu_plugin-1.0.0.mlxplugin.json`.
2. Revise e conceda as permissões declaradas; clique em **Instalar plugin**.
   A instalação valida o pacote e o deixa ativo.
3. Clique em **Abrir** no cartão do plugin e teste **Calcular**. Para os números
   `10, 20, 30`, o resultado esperado é `{"count":3,"sum":60}`.
4. Para a IA usar a ferramenta, abra o **Agent**, selecione um modelo com suporte
   a ferramentas, use o toolset `general` ou `full` e o perfil de ferramentas
   `coding` ou `full`. O catálogo da execução registra
   automaticamente **`meu_plugin__summarize_numbers`**, com a descrição e o schema
   definidos no código. Se houver seleção explícita de ferramentas, inclua esse
   nome. As políticas e o modo de execução do agente continuam valendo.
5. Peça, por exemplo: “Use `meu_plugin__summarize_numbers` para calcular a quantidade
   e a soma de 10, 20 e 30”. Não é preciso manter a interface do plugin aberta para
   o agente chamar suas ferramentas; o plugin precisa estar instalado, ativo e
   autorizado. A disponibilidade no agente não adiciona chamadas de ferramentas
   automaticamente a todo chat comum.

Para atualizar, mantenha o mesmo `id`, aumente `version`, repita build/pack e
instale o novo arquivo pelo mesmo fluxo de permissões. Não há recarga automática
dos fontes: alterações em `.ts`, `.html` ou `.css` precisam ser recompiladas e
reinstaladas. Os botões **Desativar**, **Ativar** e **Remover** controlam o ciclo de
vida descrito acima. O ID `presentation` é reservado para o Studio embarcado.

Para integrar outro cliente local diretamente ao daemon, uma ferramenta instalada
também pode ser chamada por HTTP. Exemplo com o endereço padrão:

```bash
curl --fail-with-body http://127.0.0.1:11435/agent/wasm-plugins/meu_plugin/tools/summarize_numbers \
  -H 'Content-Type: application/json' \
  -d '{"values":[10,20,30]}'
```

O corpo é a entrada da ferramenta, sem um campo `input` adicional. Use o endereço
configurado do daemon caso tenha alterado a porta; a rota revalida ativação,
grants e schema. No frontend de um plugin, use a ponte do SDK em vez dessa rota.

O exemplo completo em [`plugins/number-lab`](plugins/number-lab) reúne backend e
frontend. O [`Presentation Studio`](plugins/presentation-studio) mostra uma
integração embarcada mais ampla, com broker de documentos e exportadores; essas
APIs adicionais não fazem parte do SDK genérico de terceiros.

Consulte o [guia de arquitetura, SDK, permissões e API](docs/plugin-platform.md).

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
- Compatibility Matrix automatizada para o agente Hermes.
- API dedicada do agente:
- `POST /agent/run`
- `POST /agent/gateway/events`
- `POST /agent/stream` (stub para streaming de eventos)
- `GET /integrations/n8n/status`
- `POST /integrations/n8n/workflows/list`
- `POST /integrations/n8n/workflows/generate`
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
- Integracao direta com n8n local via API publica, editor visual nativo na aba Workflows e fonte vendorizado em `vendor/n8n`, documentada em `docs/n8n-integration.md`.

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
