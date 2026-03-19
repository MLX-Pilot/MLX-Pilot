# Changelog

Este arquivo descreve o que foi feito em cada commit da branch `mvp-functional-a6f0f23` apos a divisao em commits menores.

## Commits

- `8eb55d2` - `chore: add base ignore rules and project overview`
  - Adiciona regras iniciais de ignorar arquivos (`.gitignore`).
  - Adiciona documentacao inicial do projeto (`README.md`).

- `7a19142` - `chore(workspace): add root cargo workspace manifests`
  - Cria os manifests raiz do workspace Rust (`Cargo.toml` e `Cargo.lock`).

- `567eb69` - `feat(core): define chat domain types and provider contract`
  - Implementa a base de dominio em `crates/core` (tipos de chat, contratos e trait de provider).

- `f52e631` - `chore(provider-mlx): add crate manifest`
  - Adiciona o manifesto da crate do provider MLX.

- `88fa72e` - `feat(provider-mlx): implement local model listing and inference`
  - Implementa listagem de modelos locais e inferencia no provider MLX.

- `f7db3ea` - `chore(daemon): add daemon crate dependencies`
  - Define dependencias e configuracao da crate `daemon`.

- `1095f06` - `feat(daemon): add environment-driven runtime configuration`
  - Adiciona configuracao por variaveis de ambiente para o daemon.

- `94f8d83` - `feat(catalog): add remote model search and download jobs`
  - Implementa catalogo remoto com busca de modelos e gerenciamento de jobs de download.

- `1d8e45d` - `feat(chat): add streaming runtime and metrics parsing`
  - Adiciona fluxo de streaming de chat e parsing de metricas de execucao.

- `1a9042c` - `feat(openclaw): add runtime bridge and status integration`
  - Integra runtime/bridge do OpenClaw e endpoints de status no daemon.

- `dfa6c4e` - `feat(daemon): wire http routes for chat, catalog and openclaw`
  - Conecta as rotas HTTP principais (chat, catalogo e openclaw) no `main` do daemon.

- `f92b7f2` - `feat(desktop-ui): add base static shell and styling`
  - Cria base da UI desktop (estrutura HTML, estilos e README da UI).

- `0426124` - `feat(desktop-ui): implement chat and discover interactions`
  - Implementa interacoes de chat e descoberta de modelos na UI (`app.js`).

- `c438c52` - `feat(tauri): add desktop shell bootstrap and capabilities`
  - Configura shell Tauri, bootstrap, capabilities e arquivos de configuracao de execucao.

- `1657368` - `chore(tauri): add generated schemas and app icon assets`
  - Adiciona schemas gerados do Tauri e assets de icone do app.

- `4b34fc7` - `chore(scripts): add desktop run and daemon stop helpers`
  - Adiciona scripts utilitarios para subir desktop e parar daemon.
