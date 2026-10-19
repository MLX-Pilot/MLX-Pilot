# Reading Notes

Plugin com frontend e ferramentas WASM que consulta a Open Library e cria um caderno de leitura editável no Presentation Studio. Não precisa de modelo de IA para diagramar o documento inicial e não gera resumos de livros que não foram lidos.

## Instalar e usar

```sh
npm ci --prefix plugins/reading-notes
npm test --prefix plugins/reading-notes
npm run build --prefix plugins/reading-notes
npm run pack --prefix plugins/reading-notes
```

O build exige `extism-js` no `PATH`. Instale `reading_notes-1.1.0.mlxplugin.json` pela central de plugins. Para atualizar uma instalação anterior, use o pacote novo e revise as permissões da instalação. Habilite o plugin, autorize o serviço HTTPS `open_library` e marque a permissão para criar documentos na sua configuração.

Na interface, informe um tema e clique em **Pesquisar livros** ou **Criar caderno de leitura**. Criar consulta o catálogo novamente; o resultado pode mudar entre pesquisas. O projeto novo aparece na lista do Presentation Studio com o título `Leituras: <tema>`. A ferramenta também devolve seu `id`, `title`, `revision` e os `books` usados.

## O que é gerado

- Capa 16:9 com a consulta, quantidade real de registros e catálogo de origem.
- Páginas de referências, com até duas obras por página. Títulos ou autorias extensos recebem uma página inteira; o texto muda de linha e ajusta a tipografia dentro de limites legíveis.
- Fechamento com um roteiro de leitura: escolher a obra, registrar evidências e construir uma síntese própria.
- Briefing e notas contendo título, autoria, primeiro ano de publicação e URL completa de cada referência. Os cartões mostram uma identificação curta da fonte para preservar a legibilidade.

O tema usa navy, ciano e violeta, com texto de alto contraste, objetos nativos, entradas suaves e fonte Arial disponível offline. O projeto contém entre três e sete slides para uma a cinco obras. A composição é determinística e usa os metadados recebidos, sem inferência e sem imagens de capa inventadas. A busca conserva a ordem do catálogo, limita títulos/autorias a 160 pontos de código e inclui até três autores; esses limites também se aplicam ao retorno das ferramentas. O ano representa a primeira publicação registrada, não a edição consultada. Campo ausente fica explicitamente não informado. A seleção não é um ranking ou recomendação de leitura.

O documento é montado antes da primeira gravação e aplicado por um lote de operações com revisão otimista. A criação e a edição são duas chamadas do host: uma falha de acesso, transporte ou gravação após a criação ainda pode deixar o projeto padrão, que pode ser removido no Studio. A edição de slides é atômica; o plugin não altera projetos anteriores.

## Editar e exportar

Abra o projeto no Studio. Selecione um texto ou forma para editar pelo inspetor e mover com o mouse. **Código e validação** permite editar o documento estruturado. Pelo chat, escolha um modelo disponível e peça mudanças mantendo as fontes: “Reorganize estas referências em uma apresentação de pesquisa. Preserve autores, anos e URLs. Não invente o conteúdo dos livros.” O chat depende do modelo; a criação inicial do Reading Notes não depende dele.

Adicione às notas suas leituras, citações e páginas verificadas antes de pedir sínteses. PDF e PPTX são exportações estáticas; o HTML preserva as animações e funciona offline. As fontes completas permanecem no briefing e nas notas do projeto; os cartões exibem rótulos curtos, não links interativos.

## Ferramentas e permissões

As ferramentas continuam sendo `search_books({query})` e `create_reading_brief({query})`. A primeira é uma consulta; a segunda é mutante e usa `documents.create`/`documents.edit`. As permissões são `network:request`, `documents:read` e `documents:write`, com acesso ao serviço `open_library` e criação de documentos autorizados separadamente pelo usuário. Não há chaves de API, acesso direto ao filesystem nem outro serviço externo.

`npm test` executa os fixtures do transporte do SDK e regressões de composição, metadados, Unicode, limites de documento e diagnósticos do Studio. Os fixtures são dados sintéticos, não respostas reais do catálogo.

Esses testes executam JavaScript no Node e não medem o combustível do WASM. Antes de distribuir um pacote, faça também uma criação pelo plugin compilado em um daemon descartável, com o serviço e a criação autorizados: pesquise um tema comum e outro com Unicode, abra o projeto e confira as páginas e as fontes. A composição usa larguras de glifos e palavras em cache e soma incremental para respeitar o limite de combustível do runtime; esse limite não é elevado pelo plugin.
