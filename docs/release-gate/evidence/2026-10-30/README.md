# Evidências da auditoria de plugins — 30/10/2026

Execução local na revisão `47d941d398f44d4a99d89c3c3dc87de5313c6e62`, em Windows
x64 com Node 22.19.0. Os arquivos de código estavam commitados; `dirty: true`
registra os arquivos locais não rastreados gerados pelo graphify.

```sh
cargo build --locked -p mlx-ollama-daemon
npm ci --prefix apps/desktop-ui
npm ci --prefix packages/plugin-sdk
node scripts/release-gate/run-all.mjs --only plugin_security_smoke --output-dir temp/plugin-security-evidence
```

O bloco de plugins passou nos **11 casos obrigatórios**: nove cenários com HTTP
e WASM reais, mais os contratos da interface (8 testes com jsdom) e do SDK
(10 testes Node). O encerramento foi seguido de varredura de 17 arquivos de
estado/logs, sem vazamento da credencial de teste. O estado temporário foi
removido após preservar os logs sanitizados.

O comando retornou **1**, com decisão global **NO-GO**, porque os outros seis
blocos foram explicitamente marcados `not_run`. Essa execução não aprova o
release completo nem comprova navegação em navegador real; a automação completa
de navegador permanece no escopo da issue #59.

- [Relatório do bloco](plugin-security-report.json): respostas sanitizadas,
  controles positivos, revisão, ambiente e hashes do daemon e da fixture.
- [Contrato da interface](bridge_ui_contract.tap) e
  [contrato do SDK](sdk_bridge_contract.tap): saída TAP das suítes executadas.
- `daemon-1.log`, `daemon-2.log`, `daemon-3.log`: logs dos dois workspaces e da
  reabertura do workspace original.
- `plugin-security-fixture.json`: bytes WASM gerados a partir do WAT versionado.
- `artifact-sha256.json`: hashes dos logs e da fixture arquivados.

Caminhos absolutos de artefatos, executáveis e estado temporário foram
normalizados e os finais de linha padronizados em LF para permitir a leitura no
repositório e manter os hashes entre plataformas. Resultados, revisões,
horários e hashes não foram alterados. Configurações e vault do daemon não são
publicados. Logs vazios de stderr e da compilação silenciosa também são
preservados.

A execução negativa com binário ausente retornou 1, registrou os nove cenários
HTTP/WASM como não executados/falhos e ainda executou as duas suítes da ponte.
As 33 regressões do gate e do verificador de TAP passaram separadamente.
