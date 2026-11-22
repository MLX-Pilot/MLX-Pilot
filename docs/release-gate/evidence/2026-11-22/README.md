# MLX Pilot 0.1.0 · Demonstração de fim de semestre

Os três instaladores foram produzidos com o código da revisão `24b9525e26bf0194228703d9f2c23ccd008f5a71`. Os vídeos mostram o agente, a criação e reabertura de conversas, a entrega por um canal de teste e o trabalho com documentos. Todos os MP4 usam H.264 e podem ser reproduzidos localmente. [Revisão do aplicativo no GitHub](https://github.com/MLX-Pilot/MLX-Pilot/commit/24b9525e26bf0194228703d9f2c23ccd008f5a71)

## Roteiro para a aula

| O que mostrar | Gravação | O que observar |
|---|---|---|
| Agente na menor janela suportada | [960 × 600](videos/agente-960.mp4) | O histórico pode ser recolhido. A área de mensagens, o compositor e o botão de envio ficam visíveis ao mesmo tempo. |
| Conversa em janela padrão | [1280 × 900](videos/agente-1280.mp4) | A pessoa alterna histórico e configuração pelo teclado, confere as políticas carregadas, escolhe um modelo, envia uma pergunta e reabre a conversa salva. O modelo local devolveu **12** nas duas dimensões. |
| Navegação geral | [960 × 600](videos/navegacao-960.mp4) · [1280 × 800](videos/navegacao-1280.mp4) | Quatro seções têm acesso direto e as dez restantes ficam no menu Mais. O roteiro com dados de demonstração percorre as 14 seções, as teclas de navegação e o seletor de modelos. |
| Contas de canais | [Criar e testar conta](videos/contas-de-canais.mp4) | A conta é criada como padrão, editada preservando a credencial e renomeada. A entrega fica bloqueada enquanto a conta está desativada. O receptor local recebe uma mensagem após a reativação. |
| Reading Notes e Presentation Studio | [Instalação, pesquisa e exportação](videos/plugins.mp4) | Instalação com permissões explícitas, pesquisa bibliográfica, escrita de um documento, conflito entre revisões, recuperação e exportação dos slides. |
| Janela do aplicativo empacotado | [macOS ARM64](videos/macos-arm64.mp4) · [Ubuntu ARM64](videos/linux-arm64.mp4) | As ações ocorrem na janela nativa do pacote. A criação da conversa e a persistência da resposta são verificadas no daemon pertencente ao aplicativo. |
| Identidade pública | [Avatar da organização](videos/avatar-publico.mp4) | O perfil público do GitHub serviu uma imagem de 200 × 200, exibida a 100 × 100. A comparação leva em conta o redimensionamento da arte original. |

## O que os testes cobrem

| Escopo | Testes aprovados | Comando |
|---|---:|---|
| Workspace Rust | 502 | `cargo test --locked --workspace` |
| Interface desktop | 51 | `node --test apps/desktop-ui/e2e/*.test.js` |
| Regressões dos verificadores de entrega | 53 | `node --test scripts/tests/*.test.mjs` |
| Presentation Studio 1.0.0 | 67 | `npm test --prefix plugins/presentation-studio` |
| Plugin SDK 2.0.0 | 10 | `npm test --prefix packages/plugin-sdk` |
| Reading Notes 1.1.0 | 8 | `npm test --prefix plugins/reading-notes` |

O gate em checkout limpo `24b9525e26bf0194228703d9f2c23ccd008f5a71` registrou **GO**, com oito blocos aprovados. Foram exercitados carga curta, migração e rollback, reinício após interrupção, auditoria do agente, entrega de mensagens, plugins e o ensaio de lançamento. Os testes de políticas conferem as ferramentas permitidas por perfil e o bloqueio das operações restringidas pelo modo de execução. [Contagens e comandos das suítes](resultados/testes.json) · [Decisão e verificações de políticas](resultados/gate.json)

**Carga concorrente.** A gravação atômica das configurações foi testada com 10, 25 e 50 requisições simultâneas. O gate macOS e o gate Ubuntu retornaram HTTP 200 em 510 de 510 requisições cada. Três repetições locais adicionais retornaram HTTP 200 em todas as 1.530 requisições, com zero erros ou timeouts. [Nove lotes por plataforma](resultados/carga-gates.json) · [Repetições locais](resultados/repeticoes-carga.json)

**Plugins.** O teste de navegador usa o pacote WASM Reading Notes 1.1.0, o daemon da aplicação e um catálogo bibliográfico local com resposta controlada. Ele verifica instalação, configuração, pesquisa, criação do documento, conflito de revisão, três formatos de exportação, revogação de permissão, desativação, reativação e remoção. Os arquivos abaixo foram produzidos pela interface durante a execução. Após um timeout numa tentativa anterior, a candidata consultou a Open Library e encontrou *Dom Casmurro*, de Machado de Assis, datado de 1899. [Resultado do navegador](resultados/reading-notes.json) · [Consulta pública aprovada](resultados/catalogo-publico.json) · [Timeout anterior](resultados/catalogo-publico-repeticao.json) · [PDF](exportacoes/caderno.pdf) · [PowerPoint](exportacoes/caderno.pptx) · [HTML](exportacoes/caderno.html)

**Canais.** Os adaptadores de Telegram, Discord e Slack foram validados com respostas HTTP locais que implementam os respectivos contratos de autenticação e envio. O teste visual de contas conta as mensagens entregues ao receptor de teste. O resultado cobre a integração local e o tratamento de falhas. A confirmação de entrega a contas de pessoas nesses serviços exige credenciais próprias. [Resultado do teste visual](resultados/contas-de-canais.json)

**Provider WASM, tarefa #61.** A RFC define a descoberta, o manifesto e o protocolo da ABI 1 para provedores de inferência. A prova compila o guest WAT em WASM, carrega o módulo real via Extism, consulta o catálogo de modelos, transmite um `ChatRequest` e decodifica a resposta `pong` como `ChatResponse`. Também cobre incompatibilidade de versão e exports ausentes. O aplicativo distribuído continua usando seus provedores nativos. O registro dinâmico no daemon e a execução de streaming pelo plugin são trabalhos previstos pela RFC. [RFC](https://github.com/MLX-Pilot/MLX-Pilot/blob/24b9525e26bf0194228703d9f2c23ccd008f5a71/docs/provider-plugin-abi.md) · [Prova executável](https://github.com/MLX-Pilot/MLX-Pilot/blob/24b9525e26bf0194228703d9f2c23ccd008f5a71/crates/plugin-runtime/tests/provider_abi.rs)

## Execução dos pacotes

**Mac Mini, macOS 26.3.1, Apple Silicon.** A imagem DMG passou na verificação do macOS e foi montada em modo somente leitura. Foram conferidos cinco arquivos da aplicação, incluindo o executável. O teste abriu o bundle .app na pasta de compilação. O daemon embutido reportou `ok`. Ollama 0.30.7, llama.cpp e MLX estavam prontos. O teste usou um serviço Ollama local e um executável MLX-LM instalado no ambiente de QA. Através dos controles de acessibilidade, o teste abriu o Agente, criou uma conversa, entrou em Configurações, voltou à Conversa e enviou uma pergunta pelo compositor. Em duas execuções da candidata, a configuração `locked` com aprovação `risk_based` se manteve após andar pelas abas e após o modelo responder **4**. [Verificação do Mac](resultados/macos-arm64.json) · [Segunda execução da candidata](videos/macos-arm64-repeticao.mp4)

Na revisão anterior da interface (`22b3867`), o modelo real `qwen3:0.6b` respondeu **4** na primeira execução de “Quanto é 2 + 2?” e **2** na repetição. As respostas foram persistidas. A segunda execução falhou na asserção da resposta esperada, com o aplicativo em funcionamento e o daemon saudável. O teste de transporte do gate também usa uma resposta controlada para verificar chamadas ao modelo e conclusão do streaming. [Primeira execução](videos/macos-22b3867-primeira.mp4) · [Resposta da repetição](resultados/macos-repeticao.json) · [Trecho da repetição](videos/macos-repeticao.mp4)

Ainda na revisão anterior, em duas novas sessões nativas com temperatura zero e a diretiva `/no_think`, o modelo respondeu **4** nas duas execuções. O teste confirmou a resposta persistida e o texto visível na janela. [Dados das repetições](resultados/macos-modelo-controlado.json) · [Primeira reprodução](videos/macos-reproducao-1.mp4) · [Segunda reprodução](videos/macos-reproducao-2.mp4)

**Ubuntu ARM64.** O pacote Debian foi instalado em uma máquina virtual Ubuntu 24.04.5 LTS ARM64, executada no Mac Mini pelo Lima 2.2.1 com Apple Virtualization.framework. O guest executou o kernel Linux 6.8.0-142-generic e WebKitGTK 2.52.6. O teste abriu o executável instalado com WebKitGTK em um servidor de vídeo virtual Xvfb. O daemon identificou Ollama como pronto. Nesse ambiente os provedores MLX e llama.cpp estavam indisponíveis e a saúde geral reportou `degraded`. O agente exibiu e persistiu “Hello! How can I assist you today?” com o modelo local. O modo `locked` e a temperatura zero permaneceram ativos. A sessão e a auditoria registraram zero chamadas de ferramentas. Em outra inicialização, a interface permitiu criar a conversa e editar um rascunho sem modelo instalado. Nesse caso o daemon reportou `provider_ready: false` e a inferência ficou indisponível. [Verificação do Ubuntu com modelo](resultados/linux-arm64.json) · [Vídeo sem modelo](videos/linux-arm64-sem-modelo.mp4) · [Resultado sem modelo](resultados/linux-arm64-sem-modelo.json)

**Ubuntu x86_64.** O GitHub Actions compilou e instalou o pacote AMD64 no merge de teste `3aef76d7cab1d97d534f493e1c8a1a75375eeafe`, cuja árvore de fontes é idêntica à revisão `24b9525`. A segunda tentativa do job instalou o pacote e concluiu o teste. O gate do CI também registrou oito blocos aprovados, em Linux x64 com Node 22.23.5. Com a janela aberta, o teste conferiu o PID do daemon, criou uma sessão e digitou um rascunho no compositor. Esse ambiente rodou sem modelo instalado. [Vídeo x86_64](videos/linux-amd64.mp4) · [Resultado](resultados/linux-amd64.json) · [Job da build e teste](https://github.com/MLX-Pilot/MLX-Pilot/actions/runs/45009094679/job/137168870490) · [Suíte de CI](https://github.com/MLX-Pilot/MLX-Pilot/actions/runs/45009094679)

### Identificação dos artefatos

| Artefato | SHA-256 |
|---|---|
| MLX Pilot_0.1.0_aarch64.dmg | `c74013cddb10290a67a49567277d0e28a628838887767c3d514f5fb7b6df296f` |
| Executável macOS `MLX Pilot.app/Contents/MacOS/mlx-ollama-desktop` | `4f7e2f51a37c5b892fae2bf02fc1846da46ad987d94fb1c7e421397965c3334c` |
| MLX Pilot_0.1.0_arm64.deb | `181a960569cf77a8eef62db54f3352bcff9966f1ab079384a27925ff94f05b23` |
| MLX Pilot_0.1.0_amd64.deb | `0b9b5d6be31f999743dacaef32949f14aa4fa4e9ac67598d49c586b06a226bf6` |
| reading_notes-1.1.0.mlxplugin.json | `17ec06c8234d4a736a622e754f55bd27fc09828240dbb592366938c05a96214a` |

### Repetir os testes

Na raiz do repositório, com as dependências dos projetos instaladas e Chromium disponível, os comandos abaixo verificam a prova de provider, a execução dos oito blocos do gate e a navegação de contas.

```sh
cargo test --locked -p mlx-plugin-runtime --test provider_abi
cargo build --locked -p mlx-ollama-daemon
RELEASE_GATE_OUTPUT_DIR="$PWD/output/playwright/repeticao" node scripts/release-gate/run-all.mjs
RELEASE_GATE_OUTPUT_DIR="$PWD/output/playwright/repeticao" node scripts/release-gate/channel-accounts-browser.mjs
```

Para repetir o teste nativo do Mac, ajuste o caminho do bundle e use um diretório de evidências novo. `QA_OLLAMA_URL` aponta para o servidor local com o modelo já disponível. Na validação do MLX, `QA_MLX_COMMAND` recebeu o caminho completo do executável `mlx_lm.generate`.

```sh
QA_OLLAMA_URL=http://127.0.0.1:11434 QA_OLLAMA_MODEL=qwen3:0.6b \
python3 scripts/release-gate/macos/smoke.py "/caminho/MLX Pilot.app" evidencias-mac --check-answer
```

No Ubuntu, execute o teste a partir da raiz do projeto com um arquivo .deb já compilado para a arquitetura da máquina e o Ollama local em execução. A opção de modelo habilita a conversa com inferência durante o teste nativo.

```sh
/usr/bin/python3 scripts/release-gate/linux-package-smoke.py "/caminho/MLX Pilot_0.1.0_arm64.deb" evidencias-linux --ollama-model qwen3:0.6b
```

A integridade dos vídeos, exportações e resultados pode ser conferida com `shasum -a 256 -c SHA256SUMS` nesta pasta. [Somas de verificação](SHA256SUMS)

[Resultado do agente por resolução](resultados/agente.json) · [Comparação do avatar público](resultados/avatar-publico.json)
