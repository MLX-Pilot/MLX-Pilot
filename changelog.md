# Changelog

Este arquivo descreve o que foi feito em cada commit da branch `mvp-functional-a6f0f23` apos a divisao em commits menores e redistribuicao de autoria.

## Commits

- `de3c1ab` - `chore: add base ignore rules and project overview`
  - Adiciona regras iniciais de ignorar arquivos (`.gitignore`).
  - Adiciona documentacao inicial do projeto (`README.md`).

- `8ed43c5` - `chore(workspace): add root cargo workspace manifests`
  - Cria os manifests raiz do workspace Rust (`Cargo.toml` e `Cargo.lock`).

- `15bc889` - `feat(core): define chat domain types and provider contract`
  - Implementa a base de dominio em `crates/core` (tipos de chat, contratos e trait de provider).

- `47cb17b` - `chore(provider-mlx): add crate manifest`
  - Adiciona o manifesto da crate do provider MLX.

- `a41a8da` - `feat(provider-mlx): implement local model listing and inference`
  - Implementa listagem de modelos locais e inferencia no provider MLX.

- `0d3eab2` - `chore(daemon): add daemon crate dependencies`
  - Define dependencias e configuracao da crate `daemon`.

- `6c887c2` - `feat(daemon): add environment-driven runtime configuration`
  - Adiciona configuracao por variaveis de ambiente para o daemon.

- `5d2a85c` - `feat(catalog): add remote model search and download jobs`
  - Implementa catalogo remoto com busca de modelos e gerenciamento de jobs de download.

- `75af033` - `feat(chat): add streaming runtime and metrics parsing`
  - Adiciona fluxo de streaming de chat e parsing de metricas de execucao.

- `821d9d9` - `feat(openclaw): add runtime bridge and status integration`
  - Integra runtime/bridge do OpenClaw e endpoints de status no daemon.

- `75a26e6` - `feat(daemon): wire http routes for chat, catalog and openclaw`
  - Conecta as rotas HTTP principais (chat, catalogo e openclaw) no `main` do daemon.

- `5193882` - `feat(desktop-ui): add base static shell and styling`
  - Cria base da UI desktop (estrutura HTML, estilos e README da UI).

- `36d70aa` - `feat(desktop-ui): implement chat and discover interactions`
  - Implementa interacoes de chat e descoberta de modelos na UI (`app.js`).

- `d05dee7` - `feat(tauri): add desktop shell bootstrap and capabilities`
  - Configura shell Tauri, bootstrap, capabilities e arquivos de configuracao de execucao.

- `bebd1f9` - `chore(tauri): add generated schemas and app icon assets`
  - Adiciona schemas gerados do Tauri e assets de icone do app.

- `1d6b0e9` - `chore(scripts): add desktop run and daemon stop helpers`
  - Adiciona scripts utilitarios para subir desktop e parar daemon.

- `8344bb6` - `docs: add commit-level and user-level changelogs`
  - Adiciona documentacao de changelog por commit e por usuario.
