# Fontes, evidências e créditos

Edição preparada em **18 de outubro de 2026** para apresentar o sistema de plugins
do MLX-Pilot. Este documento acompanha a pasta distribuída, mesmo quando a
apresentação é assistida sem internet.

## Base técnica

As descrições foram conferidas no código e na documentação da revisão
[`0e354a2`](https://github.com/MLX-Pilot/MLX-Pilot/commit/0e354a205d398801870335ef32ee4c96b7b07d35).
Os links abaixo apontam para essa revisão, para preservar o contexto da edição.

| Informação | Fonte do projeto |
| --- | --- |
| Instalar, configurar, ativar, desativar e remover plugins | [README](https://github.com/MLX-Pilot/MLX-Pilot/blob/0e354a205d398801870335ef32ee4c96b7b07d35/README.md) |
| Arquitetura, permissões, formatos, limites e endpoints | [Plataforma de plugins](https://github.com/MLX-Pilot/MLX-Pilot/blob/0e354a205d398801870335ef32ee4c96b7b07d35/docs/plugin-platform.md) |
| SDK 2.0.0 e comandos de desenvolvimento | [SDK](https://github.com/MLX-Pilot/MLX-Pilot/tree/0e354a205d398801870335ef32ee4c96b7b07d35/packages/plugin-sdk) |
| Ferramentas, serviço e briefing do Reading Notes | [Fonte do exemplo](https://github.com/MLX-Pilot/MLX-Pilot/blob/0e354a205d398801870335ef32ee4c96b7b07d35/plugins/reading-notes/src/index.ts) |
| Casos simulados do exemplo | [Fixtures do Reading Notes](https://github.com/MLX-Pilot/MLX-Pilot/blob/0e354a205d398801870335ef32ee4c96b7b07d35/plugins/reading-notes/fixtures.json) |
| Renderização, composição, validação e exportação | [Presentation Studio](https://github.com/MLX-Pilot/MLX-Pilot/tree/0e354a205d398801870335ef32ee4c96b7b07d35/plugins/presentation-studio/src) |
| Broker e acesso por plugin/workspace | [Host de capacidades](https://github.com/MLX-Pilot/MLX-Pilot/blob/0e354a205d398801870335ef32ee4c96b7b07d35/crates/daemon/src/plugin_host.rs) |
| Checks automatizados da integração | [Workflow Rust](https://github.com/MLX-Pilot/MLX-Pilot/blob/0e354a205d398801870335ef32ee4c96b7b07d35/.github/workflows/rust.yml) |

Open Library é a fonte de metadados consultada pelo exemplo. O plugin utiliza
`https://openlibrary.org/search.json` e mantém, quando disponível, o endereço da
obra em cada referência. A presença de um livro nos resultados não implica
endosso, leitura integral, comprovação de seu conteúdo ou autoria do MLX-Pilot.
As fontes dos livros específicos pertencem ao resultado e ao briefing mostrados.

## Evidências de validação

O registro desta revisão reúne **311 testes Rust**, **85 testes JavaScript** e
**4 fixtures do Reading Notes**. O
[run de CI 39166027511](https://github.com/MLX-Pilot/MLX-Pilot/actions/runs/39166027511)
foi confirmado aprovado em **18/10/2026**. O lint do workspace também foi
verificado com **Rust/Clippy 1.99.0** e avisos tratados como erro.

As contagens correspondem a conjuntos diferentes de verificações. Fixtures do
SDK podem simular respostas do host; capturas de interface demonstram caminhos
específicos do aplicativo real. Nenhuma dessas evidências equivale a cobertura
total, certificação de segurança, prova de ausência de defeitos ou avaliação
visual completa em todos os leitores de PDF/PPTX.

## Procedência da mídia

### Identidade visual e tipografia

A apresentação usa o tema **Orbital Command** do frontend do MLX-Pilot:
fundos `#06090f`, `#0a0e17` e `#0f1520`, ciano `#00d4ff`, violeta `#7b61ff`,
Outfit nos títulos, Manrope no corpo e JetBrains Mono nos dados. A logomarca e o
ícone vetoriais reproduzem a geometria hexagonal e o núcleo orbital do aplicativo.
Os SVGs são locais e o texto da marca está convertido em curvas.

As três fontes variáveis são distribuídas sob SIL Open Font License 1.1, com
[origem e licenças locais](assets/fonts/README.md). Foram obtidas do repositório
oficial `google/fonts`, na revisão `47d54ca1ebb14fd864695f1d39dc734f39017c33`,
e convertidas de TTF para WOFF2 sem recorte de glifos. A apresentação e o
aplicativo carregam essas cópias locais para manter a tipografia offline.

### Capturas da aplicação

As imagens e os vídeos desta pasta são **capturas próprias da aplicação real**,
produzidas para a demonstração com daemon isolado e dados de exemplo. Não foram
usadas imagens externas da web para representar telas, pessoas ou resultados do
produto. As capturas demonstram a interface e os fluxos do MLX-Pilot.

Os arquivos locais são:

- `assets/01-instalar-configurar.mp4`: instalação e configuração.
- `assets/02-servico-documento.mp4`: Reading Notes e passagem para documento.
- `assets/03-studio-exportar.mp4`: Studio e caminhos de exportação.
- Legendas `.vtt` correspondentes e capturas `.png` da interface.

São gravações preparadas, não transmissões ao vivo. Os três clipes não têm cortes
nem aceleração.
O terceiro utiliza um documento previamente gerado; não se deve interpretá-lo
como cronômetro de inferência ou como geração contínua por um modelo. Textos de
exemplo servem para demonstrar edição e exportação. A chave da configuração
ilustrativa é fictícia; o material não depende de credenciais reais.
As transcrições textuais acompanham os vídeos como alternativa às trilhas VTT.

### Registro da captura 1

`01-instalar-configurar.mp4` tem **19,266 s**, resolução **1440 × 900**, **30 fps**
e codificação **H.264/MP4**, sem áudio. Foi gravado sem cortes ou aceleração; a
ferramenta de captura acrescentou a indicação do cursor sem alterar o conteúdo da
interface. Mostra a revisão e instalação do Reading Notes, a concessão de três
permissões e a configuração de Open Library, documento selecionado e criação.

O daemon é real e isolado. A indicação **Degradado** no cabeçalho pode aparecer
porque o modelo e seu início automático foram desativados para a demonstração.
Nenhum trecho deve ser anunciado como nova inferência do modelo ou contagem de
tokens. Uma configuração ilustrativa de API com credencial, caso exibida em
captura estática, utiliza chave fictícia e não comprova uma chamada real àquela API.

### Registro da captura 2

`02-servico-documento.mp4` tem **14,833 s**, resolução **1440 × 900**, **30 fps**
e codificação **H.264/MP4**, sem áudio, cortes ou aceleração. Registra uma consulta
HTTPS real à Open Library pelo tema `urban gardens`, o retorno de cinco obras, a
criação de um briefing no Studio e a abertura do documento. Os títulos e fontes
apresentados vêm da resposta do catálogo; a criação do briefing é executada pela
lógica do Reading Notes, sem inferência de modelo nesse trecho.

### Registro da captura 3 e dos arquivos de exemplo

`03-studio-exportar.mp4` tem **19,900 s**, resolução **1440 × 900**, **30 fps**
e codificação **H.264/MP4**, sem áudio, cortes ou aceleração. Mostra seleção de título com
o mouse, alteração pelo inspetor, acesso ao segundo slide, animações no player
HTML e exportações reais para PDF, PPTX editável e HTML.

O projeto utilizado foi **gerado anteriormente com Qwen 3.5 9B em um teste do
Studio**. Não é o briefing recém-criado pelo Reading Notes, e o vídeo não contém
nova inferência. Essa escolha permite demonstrar o editor e seus formatos de saída
sem apresentar uma espera ou uma sequência fabricada como geração ao vivo.

Os arquivos `assets/exemplos/jardins-urbanos.pdf`, `.pptx` e `.html` foram exportados
pelo aplicativo e acompanham a entrega. As **três páginas do PDF** foram
renderizadas com Poppler para inspeção visual, sem cortes ou defeitos de layout
observados nessa conferência. Esse resultado diz respeito a esse PDF; não afirma
compatibilidade visual testada em todas as versões de PowerPoint ou leitores.

## Equipe

Créditos aos seis perfis GitHub indicados no histórico do projeto. A apresentação
não atribui papéis, falas ou contribuições individuais além dessa identificação.

- [@Kaike-Vitorino](https://github.com/Kaike-Vitorino)
- [@RamLi06](https://github.com/RamLi06)
- [@GabrielSalustiano](https://github.com/GabrielSalustiano)
- [@PETROMYZONMONSTER](https://github.com/PETROMYZONMONSTER)
- [@gabriellima-4](https://github.com/gabriellima-4)
- [@MarcellinhoHM](https://github.com/MarcellinhoHM)

Marcas, nomes de projetos e serviços mantêm suas respectivas autorias. Este
material descreve a integração implementada no MLX-Pilot; não implica participação
ou aprovação da Open Library, SillyTavern ou dos autores das tecnologias citadas.
