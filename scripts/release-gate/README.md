# Auditoria de segurança de plugins

A auditoria da issue #60 usa um daemon real com estado temporário, dois plugins
de teste e um servidor HTTP local. As fixtures WASM são compiladas a partir de
WAT pelo exemplo Rust `plugin_security_fixture`, usando as dependências do
`Cargo.lock`. Nenhum modelo de IA ou serviço externo é necessário para executar
os cenários.

Prepare as dependências na raiz do repositório:

```sh
cargo build --locked -p mlx-ollama-daemon
npm ci --prefix apps/desktop-ui
npm ci --prefix packages/plugin-sdk
```

Execute a auditoria isolada:

```sh
node scripts/release-gate/plugin-security-smoke.mjs
```

`RELEASE_GATE_DAEMON_BIN` permite indicar um daemon já compilado;
`RELEASE_GATE_OUTPUT_DIR` define onde salvar as evidências. A seleção automática
do binário reconhece `.exe` no Windows. O processo de teste usa diretórios e
portas locais próprios, com instalação e inicialização automática de runtimes
desativadas.

## Evidências e limites

Os casos do broker atravessam as rotas HTTP do daemon e verificam autorização
por plugin, workspace e documento, revogação, credenciais, políticas de rede,
conflitos de revisão e limites de execução WASM. As respostas de rejeição são
conferidas junto com o estado final e, nos casos de rede, as requisições
observadas pelo servidor local.

O contrato da ponte executa as suítes existentes da interface e do SDK em
subprocessos. A suíte da interface usa **DOM simulado com jsdom**, incluindo o
evento de navegação que invalida a ponte. O relatório identifica essa modalidade;
ela não comprova navegação em um navegador real. A automação completa do percurso
com navegador e daemon reais pertence à issue #59.

Cada caso obrigatório deve aparecer uma única vez e passar. Falhas, testes
ignorados, ausência de casos, saída vazia ou erro de infraestrutura impedem a
aprovação. O relatório e os logs dos testes ficam no diretório de evidências.
Não são evidências de uma auditoria completa do produto nem de validação de
serviços externos pela Internet.

## Resultado no release gate

Para registrar apenas este bloco no relatório de release:

```sh
node scripts/release-gate/run-all.mjs --only plugin_security_smoke --output-dir temp/plugin-security-evidence
```

Esse comando retorna **código 1 e NO-GO**, mesmo quando o bloco de plugins passa,
porque os demais blocos ficam explicitamente como não executados. O relatório
JSON/Markdown distingue a aprovação do bloco da aprovação do release inteiro.

O gate completo mantém o comando:

```sh
node scripts/release-gate/run-all.mjs
```

Todos os blocos obrigatórios precisam passar. Uma exceção em um bloco é
registrada como falha e os demais blocos ainda produzem seus resultados. A CLI
retorna código diferente de zero para qualquer decisão NO-GO.

O resultado original de março foi preservado em
[`docs/release-gate/history/2026-03-07.md`](../../docs/release-gate/history/2026-03-07.md).
Seu GO é histórico e não aprova revisões posteriores.

O [relatório atual](../../docs/release-gate-report.md) registra a revisão e os
casos efetivamente executados. Os logs TAP, as respostas sanitizadas e o hash da
fixture permitem conferir a evidência sem publicar configurações ou credenciais
do daemon de teste.

## Regressões e CI

```sh
node --test scripts/tests/release-gate.test.mjs scripts/tests/plugin-bridge-security.test.mjs
```

O CI roda essas regressões, compila o daemon, executa a auditoria de plugins e
publica o artefato `plugin-security-audit`, inclusive quando a auditoria falha.
Os demais blocos do release gate continuam exigindo sua própria execução.
