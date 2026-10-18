# MLX-Pilot — apresentação do sistema de plugins

Apresentação acadêmica em **12 slides**, com animações, capturas da interface e
três vídeos de demonstração. O [roteiro de fala](ROTEIRO.md) foi planejado para
**8–10 minutos**, incluindo os vídeos.

## Abrir e apresentar

1. Copie ou extraia **esta pasta inteira**, mantendo `index.html` e `assets/`
   juntos. Não abra o HTML de dentro de um arquivo ZIP.
2. Dê duplo clique em **[index.html](index.html)** para abrir no navegador.
3. Use os botões da apresentação ou os atalhos abaixo. Inicie cada vídeo pelos
   seus controles; eles não tocam automaticamente.

O material de apresentação funciona por `file://`, **offline, sem servidor e sem
build**. HTML, estilos, scripts, imagens, vídeos e legendas estão no pacote local.
Os links de referências e perfis GitHub só precisam de internet quando abertos.
Não é necessário instalar MLX-Pilot, Node.js, Rust ou um modelo para assistir.

A identidade visual acompanha o **Orbital Command** do frontend: fundos azul-escuros,
ciano e violeta, logomarca hexagonal e as fontes Outfit, Manrope e JetBrains Mono.
As fontes e suas licenças estão em `assets/fonts/`; nenhuma delas precisa ser
baixada ou instalada ao abrir a apresentação.

| Controle | Ação |
| --- | --- |
| Setas ← / →, Page Up / Page Down | Slide anterior / próximo |
| Espaço | Próximo slide quando o foco está na apresentação; dentro de um vídeo, use os controles do player |
| Home / End | Primeiro / último slide |
| F | Solicitar ou sair de tela cheia |
| N | Mostrar ou ocultar notas do apresentador |
| G | Abrir ou fechar a visão geral dos slides |
| ? | Exibir ajuda de teclado |
| Movimento reduzido | Diminuir animações pelo botão da interface |

Se o navegador recusar tela cheia em arquivo local, use seu comando de tela cheia.
Para apresentar, teste antes no navegador e na tela que serão usados em sala.
As notas e o roteiro podem ser consultados sem depender de uma segunda janela.

## Vídeos e acessibilidade

| Arquivo | Duração | Conteúdo |
| --- | --- | --- |
| [01-instalar-configurar.mp4](assets/01-instalar-configurar.mp4) | 19,266 s | Central de plugins, instalação e autorizações |
| [02-servico-documento.mp4](assets/02-servico-documento.mp4) | 14,833 s | Reading Notes: serviço autorizado, pesquisa e criação de briefing |
| [03-studio-exportar.mp4](assets/03-studio-exportar.mp4) | 19,900 s | Presentation Studio: edição e exportação |

Cada vídeo tem controles próprios, o botão **CC · Legendas** e uma
**Transcrição visual** que abre dentro da apresentação. As legendas estão
embutidas no JavaScript e funcionam offline, inclusive quando o navegador bloqueia
arquivos `.vtt` externos em `file://`. Os VTT também acompanham os MP4 para uso em
outros players. As transcrições estão ainda em [ROTEIRO.md](ROTEIRO.md).
As capturas estáticas e as descrições permitem acompanhar o argumento sem assistir
a toda a reprodução. O botão de movimento reduzido afeta as animações dos slides;
o vídeo continua sob controle de quem assiste.

As gravações mostram a **aplicação real**, com daemon isolado e dados de
demonstração. São demonstrações preparadas, não uma transmissão ao vivo nem uma
medição de velocidade de geração. Os três clipes não têm cortes nem aceleração. O terceiro usa
um projeto separado, gerado anteriormente com **Qwen 3.5 9B**, e mostra edição e
exportação reais; não representa uma nova inferência. A configuração ilustrativa
com chave usa um valor fictício e não demonstra uma chamada àquela API. A
procedência e o registro de edição ficam em
[Fontes e créditos](FONTES-E-CREDITOS.md).

O ambiente isolado pode exibir **Degradado** no cabeçalho porque a inferência e
o início automático do modelo foram desativados para a captura. Isso não significa
que as chamadas de plugins mostradas sejam simuladas: o daemon e o broker reais
executam o fluxo. Os vídeos não apresentam uma nova geração de texto pela IA.

## Arquivos exportados na demonstração

Os resultados reais do terceiro vídeo acompanham a pasta:

- [PDF de três páginas](assets/exemplos/jardins-urbanos.pdf).
- [PPTX com elementos editáveis](assets/exemplos/jardins-urbanos.pptx).
- [HTML animado](assets/exemplos/jardins-urbanos.html).

O PDF foi renderizado com Poppler para conferência visual das três páginas.
Esses exemplos permitem inspecionar o resultado além da gravação. O PPTX precisa
de um aplicativo compatível; o HTML pode ser aberto diretamente no navegador.

## O que a apresentação demonstra

O fio condutor é **pesquisar → preparar um briefing → editar → exportar**.
Reading Notes consulta metadados de livros na Open Library e cria um projeto no
Studio com fontes. O Studio permite trabalhar por chat, código ou mouse e exportar
o mesmo documento estruturado em formatos diferentes.

| Camada | Responsabilidade |
| --- | --- |
| Frontend do plugin | HTML/CSS/JS da interface, em iframe isolado; usa a ponte do SDK |
| Ferramenta WASM | Lógica do plugin, entradas validadas e resultados JSON |
| Daemon local | Execução, instalação, estado, políticas e persistência |
| Host de capacidades | Confere as permissões do plugin e os recursos autorizados no workspace |
| Cofre | Guarda credenciais criptografadas e permite sua injeção pelo host |
| API externa | Serviço de dados declarado pelo plugin e autorizado pelo usuário |

O frontend é uma parte do plugin; não é o daemon. O WASM não é um processo Node.js.
Node/npm e os compiladores são ferramentas para **desenvolver e empacotar**;
usuários instalam o `.mlxplugin.json` pronto.

## Limites a explicar corretamente

- A apresentação funciona offline; **refazer uma pesquisa na Open Library exige
  internet**. Os vídeos locais não fazem novas consultas durante a reprodução.
- Reading Notes retorna até cinco registros de catálogo. Título, autoria, ano e
  endereço de origem não equivalem à leitura ou ao resumo integral dos livros.
- Serviços e documentos começam sem autorização. O acesso é aos projetos
  selecionados; criar novos projetos exige autorização própria.
- Serviços externos usam HTTPS. Rede local exige consentimento explícito; HTTP
  só é aceito para endereços locais autorizados. O SDK não entrega chaves brutas
  ao plugin: ele consulta disponibilidade e pede chamadas ao host.
- O SDK está na versão **2.0.0**. Frontends construídos com a ponte anterior
  precisam de novo build/pack e reinstalação. O manifesto continua com API v1.
- **PDF e PPTX são estáticos; HTML preserva as animações suportadas.** O PDF é
  rasterizado. O PPTX nativo mantém elementos editáveis, com limites para CSS;
  o PPTX visual preserva a aparência por imagens. Nenhum deles recupera estrutura
  a partir de um PDF pronto: todos partem do documento do Studio.
- Exportação e preview acontecem no editor aberto. O agente não ganhou, por isso,
  um exportador headless genérico. Um modelo local com ferramentas pode usar os
  plugins no Agent, sujeito ao catálogo, às políticas e ao modo de execução.
- Os testes reduzem classes conhecidas de falhas. Não comprovam cobertura total,
  ausência de vulnerabilidades, veracidade de todo texto ou qualidade estética
  automática. Imagens de fundo e CSS personalizado ainda pedem inspeção humana.

O registro de validação usado nesta edição, em **18/10/2026**, reúne **311 testes
Rust, 85 testes JavaScript e 4 fixtures do Reading Notes**. O
[CI 39166027511](https://github.com/MLX-Pilot/MLX-Pilot/actions/runs/39166027511)
foi confirmado aprovado nessa data. São evidências daquela revisão, com escopos
diferentes; não um percentual de cobertura.

## Compartilhar

Envie a pasta `apresentacao-plugins/` completa ou um ZIP dela. Preserve `assets/`,
os vídeos, as legendas e estes documentos. Quem recebe deve extrair a pasta antes
de abrir `index.html`. O roteiro e as fontes acompanham a apresentação para que
outra pessoa consiga explicá-la sem acesso à conversa que a originou.

Consulte [ROTEIRO.md](ROTEIRO.md) para a fala e
[FONTES-E-CREDITOS.md](FONTES-E-CREDITOS.md) para procedência, evidências e equipe.
