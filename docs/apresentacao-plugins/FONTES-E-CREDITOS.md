# Fontes, evidências e créditos

Edição preparada em **18 de outubro de 2026** para apresentar o sistema de plugins
do MLX-Pilot. Este documento acompanha a pasta distribuída, mesmo quando a
apresentação é assistida sem internet.

## Base técnica

As novas capturas partem da base
[`b7a38ed`](https://github.com/MLX-Pilot/MLX-Pilot/commit/b7a38edd6f46457701faf48b77abfa4afdad36fe),
com o alinhamento da interface Orbital em
[`198df96`](https://github.com/MLX-Pilot/MLX-Pilot/commit/198df968ca0d435c0f59253c57ebef0a0a487075)
e o **Reading Notes 1.1.0**, implementado em
[`b0b6718`](https://github.com/MLX-Pilot/MLX-Pilot/commit/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2).
Os links técnicos abaixo preservam essa revisão do código. A base anterior
`0e354a2` permanece identificada apenas nos registros históricos de validação.

| Informação | Fonte do projeto |
| --- | --- |
| Instalar, configurar, ativar, desativar e remover plugins | [README](https://github.com/MLX-Pilot/MLX-Pilot/blob/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2/README.md) |
| Arquitetura, permissões, formatos, limites e endpoints | [Plataforma de plugins](https://github.com/MLX-Pilot/MLX-Pilot/blob/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2/docs/plugin-platform.md) |
| SDK 2.0.0 e comandos de desenvolvimento | [SDK](https://github.com/MLX-Pilot/MLX-Pilot/tree/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2/packages/plugin-sdk) |
| Ferramentas, serviço e caderno de leitura | [Reading Notes](https://github.com/MLX-Pilot/MLX-Pilot/tree/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2/plugins/reading-notes) |
| Composição de capa, referências e fechamento | [Compositor do caderno](https://github.com/MLX-Pilot/MLX-Pilot/blob/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2/plugins/reading-notes/src/brief.ts) |
| Renderização, composição, validação e exportação | [Presentation Studio](https://github.com/MLX-Pilot/MLX-Pilot/tree/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2/plugins/presentation-studio/src) |
| Broker e acesso por plugin/workspace | [Host de capacidades](https://github.com/MLX-Pilot/MLX-Pilot/blob/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2/crates/daemon/src/plugin_host.rs) |
| Checks automatizados da integração | [Workflow Rust](https://github.com/MLX-Pilot/MLX-Pilot/blob/b0b6718ed1fc5a81b19f987f8a794a7bbf5662a2/.github/workflows/rust.yml) |

Open Library é a fonte de metadados consultada pelo exemplo. O plugin utiliza
`https://openlibrary.org/search.json` e mantém, quando disponível, o endereço da
obra em cada referência. A presença de um livro nos resultados não implica
endosso, leitura integral, comprovação de seu conteúdo ou autoria do MLX-Pilot.
As fontes completas ficam no briefing e nas notas do caderno. Criar faz uma nova
consulta; a lista pode mudar entre pesquisa e criação. Os cinco slides da gravação
são o resultado daquela consulta; o compositor admite de três a sete slides.

## Evidências de validação

O registro **histórico da base anterior** reúne **311 testes Rust**, **85 testes
JavaScript** e **4 fixtures do Reading Notes**. O
[run de CI 39166027511](https://github.com/MLX-Pilot/MLX-Pilot/actions/runs/39166027511)
foi confirmado aprovado em **18/10/2026** naquela revisão. O registro inclui lint
com **Rust/Clippy 1.99.0** e avisos tratados como erro. Esses números e esse CI
não são apresentados como uma nova execução da atualização de mídia.

Para o Reading Notes 1.1.0 e a nova demonstração, o registro adicional contém:

- **67 testes do Presentation Studio**.
- **3 fixtures do SDK e 8 testes de composição/integração JavaScript do Reading Notes**.
- Chamadas ao **WASM compilado pelo daemon real**, com serviço e criação
  autorizados: `urban gardens` e `café` retornaram projetos com cinco slides e
  revisão 2. A segunda consulta exerceu metadados Unicode.
- Inspeção da interface atualizada, das composições e dos arquivos exportados.

Não se somam essas contagens como uma suíte única: parte dos testes do Studio já
integra os conjuntos históricos, e as fixtures usam respostas sintéticas.

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
- [Manifesto de mídia](assets/media-manifest.json), com revisão, durações, cues e
  hashes SHA-256 dos MP4 e das capturas finais.

São gravações preparadas, não transmissões ao vivo. Os três clipes não têm cortes
nem aceleração.
O terceiro utiliza um documento editorial preparado; não se deve interpretá-lo
como cronômetro de inferência ou como geração contínua por um modelo. Textos de
exemplo servem para demonstrar edição e exportação. A chave da configuração
ilustrativa é fictícia; o material não depende de credenciais reais.
As transcrições textuais acompanham os vídeos como alternativa às trilhas VTT.

### Registro da captura 1

`01-instalar-configurar.mp4` tem **16,000 s**, resolução **1440 × 900**, **30 fps**
e codificação **H.264/MP4**, sem áudio, cortes ou aceleração. Mostra a revisão e
instalação do Reading Notes, a concessão de três
permissões e a configuração de Open Library, documento selecionado e criação.

O daemon é real e isolado. A indicação **Degradado** no cabeçalho pode aparecer
porque o modelo e seu início automático foram desativados para a demonstração.
Nenhum trecho deve ser anunciado como nova inferência do modelo ou contagem de
tokens. Uma configuração ilustrativa de API com credencial, caso exibida em
captura estática, utiliza chave fictícia e não comprova uma chamada real àquela API.

### Registro da captura 2

`02-servico-documento.mp4` tem **26,500 s**, resolução **1440 × 900**, **30 fps**
e codificação **H.264/MP4**, sem áudio, cortes ou aceleração. Registra uma consulta
HTTPS real à Open Library pelo tema `urban gardens`, o retorno de cinco obras, a
criação de um caderno de leitura de cinco slides e a abertura do documento no
Studio. Os títulos e fontes vêm da resposta do catálogo; o caderno é composto
pela lógica do Reading Notes 1.1.0, sem inferência de modelo nesse trecho.

O caderno efetivamente criado no segundo vídeo também acompanha o material em
`assets/exemplos/caderno-leitura.json` e `.html`, permitindo conferir seus cinco
slides, os metadados e a estrutura editável.

### Registro da captura 3 e dos arquivos de exemplo

`03-studio-exportar.mp4` tem **33,733 s**, resolução **1440 × 900**, **30 fps**
e codificação **H.264/MP4**, sem áudio, cortes ou aceleração. Mostra seleção de um subtítulo com
o mouse, alteração pelo inspetor, acesso ao segundo slide, animações no player
HTML e exportações reais para PDF, PPTX editável, HTML e projeto JSON.

O projeto **Jardins urbanos — da pesquisa ao cuidado** foi preparado por direção
de arte assistida para esta demonstração, com **cinco slides e duas ilustrações
geradas**. É separado do caderno criado pelo Reading Notes. O vídeo não contém
nova inferência do Qwen ou de outro modelo local. O chat com modelo local é uma
capacidade do Studio, mas não é a origem alegada deste projeto.

Os arquivos `assets/exemplos/jardins-urbanos.json`, `.pdf`, `.pptx` e `.html` foram
exportados pelo aplicativo e acompanham a entrega. O JSON preserva o projeto e
as duas imagens incorporadas; PDF e PPTX são estáticos, enquanto o HTML mantém
as animações suportadas. O PDF contém **cinco páginas**. A verificação dos arquivos
não afirma compatibilidade visual testada em todas as versões de PowerPoint.

### Ilustrações e referências do exemplo editorial

As duas cenas de jardim foram criadas com a ferramenta integrada **image_gen**,
em **18/10/2026**. A primeira mostra canteiros e um caminho ao nível do observador;
a segunda apresenta um jardim em vista elevada. São **ilustrações de locais
fictícios**, sem pessoas ou localização real identificada, creditadas nos slides.
Não são fotografias documentais, capas de livros ou plantas técnicas. Não foram
obtidas de bancos de imagens e não se atribui a elas uma licença Creative Commons.

Os pedidos orientaram uma estética fotográfica editorial, vegetação natural,
luz suave da manhã, canteiros de madeira, caminhos e contexto urbano, sem textos,
marcas ou gráficos sobrepostos. Os originais estão incorporados no projeto JSON;
o registro detalhado dos [prompts e da procedência](assets/exemplos/references.json)
acompanha a pasta distribuída.

O slide de referências reproduz dois registros da consulta real à Open Library:

| Obra | Autoria no catálogo | Primeira publicação | Fonte |
| --- | --- | --- | --- |
| City gardening | Deirdre Colby | 1987 | [OL15825666W](https://openlibrary.org/works/OL15825666W) |
| Urban Gardens | Ann-Marie Powell | 2005 | [OL9028256W](https://openlibrary.org/works/OL9028256W) |

Esses livros não foram lidos nem resumidos para compor o exemplo. O ano corresponde
ao campo `first_publish_year`. Os demais slides apresentam uma proposta de
investigação e discussão do lugar, sem estatísticas ou benefícios atribuídos às
obras. A fonte interna do projeto é Helvetica, aceita pelo schema atual do Studio;
Outfit/Manrope permanecem na interface e na apresentação sobre plugins.

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
