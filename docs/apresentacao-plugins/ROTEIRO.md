# Roteiro de apresentação — plugins do MLX-Pilot

**Público:** professor e turma de desenvolvimento de software. **Duração-alvo:**
10 minutos, com margem entre 9 e 11. Os tempos abaixo são janelas de fala e
reprodução, não a duração técnica de cada MP4. Os três vídeos são gravações reais
preparadas; não anuncie uma execução ao vivo.

Antes de começar, abra `index.html`, confirme o funcionamento dos três vídeos e
deixe o movimento reduzido disponível. Se uma legenda VTT não carregar em
`file://`, use a descrição junto ao vídeo e as transcrições deste roteiro. Use **N** para consultar as
notas e **G** para ir diretamente a um slide durante as perguntas.

| Slide | Tema | Janela | Acumulado |
| --- | --- | --- | --- |
| 1 | Um sistema de plugins para IA local | 0:30 | 0:30 |
| 2 | Estender sem reescrever o aplicativo | 0:35 | 1:05 |
| 3 | Arquitetura e fronteiras de execução | 0:55 | 2:00 |
| 4 | Anatomia de um plugin | 0:40 | 2:40 |
| 5 | Uma ferramenta em poucas linhas | 0:45 | 3:25 |
| 6 | O usuário decide o alcance | 0:50 | 4:15 |
| 7 | Instalação e configuração — vídeo 1 | 1:00 | 5:15 |
| 8 | Pesquisa real, documento editável — vídeo 2 | 1:20 | 6:35 |
| 9 | Um projeto, diferentes entregas — vídeo 3 | 1:30 | 8:05 |
| 10 | A IA usa as mesmas ferramentas | 0:50 | 8:55 |
| 11 | Engenharia com limites explícitos | 0:45 | 9:40 |
| 12 | Equipe e fechamento | 0:20 | 10:00 |

## 1. Um sistema de plugins para IA local

“O MLX-Pilot executa modelos localmente e agora permite ampliar suas funções por
plugins. A proposta é dar ao desenvolvedor um contrato comum para criar uma
ferramenta, oferecer uma interface e disponibilizar essa mesma ferramenta para a
IA. Vamos acompanhar um caso concreto: pesquisar referências, criar um caderno
de leitura editável e conhecer as possibilidades de edição e exportação.”

**Aponte:** o nome do projeto e o fluxo pesquisa → documento → apresentação.
Apresente os vídeos como evidência da interface em funcionamento, com dados de
demonstração e daemon isolado.

## 2. Estender sem reescrever o aplicativo

“Sem um contrato de extensão, cada integração tende a repetir interface, chamadas
de API e armazenamento. Aqui, o plugin declara suas ferramentas e os recursos de
que precisa. A central permite instalar, configurar, abrir, desativar e remover o
pacote. A inspiração em extensões como as do SillyTavern é a experiência de
ampliar o aplicativo; a execução foi organizada em WASM e em capacidades
controladas pelo daemon.”

**Aponte:** os cartões da central. Desativar interrompe novas chamadas; remover
apaga o pacote e sua configuração, preservando os documentos produzidos.

## 3. Arquitetura e fronteiras de execução

“Há seis responsabilidades diferentes neste desenho. O frontend é a tela
HTML/CSS/JS, isolada do DOM principal. A ferramenta WASM contém a lógica do
plugin. O daemon é o processo local que executa, aplica políticas e persiste
estado. Dentro dele, o host de capacidades confere qual plugin pode acessar qual
serviço ou documento. O cofre guarda as credenciais, e a API externa fornece os
dados. O plugin pede uma operação ao host; ele não recebe acesso geral ao
computador nem o valor da chave.”

**Aponte:** frontend → ponte do SDK → daemon/WASM → host → serviço ou documento.
O cofre participa da chamada autorizada, sem virar uma resposta com a chave.
Explique que “host” aqui é o mediador de recursos dentro do daemon, não outro
servidor que o desenvolvedor precisa publicar.

## 4. Anatomia de um plugin

“O pacote reúne um manifesto, o módulo WASM e, opcionalmente, os arquivos da
interface. O manifesto informa identidade, ferramentas, schemas, permissões e
serviços. O SDK fornece o projeto inicial e os comandos de teste, build e
empacotamento. Node e npm são usados pelo desenvolvedor nessa preparação. Na
execução, o backend da ferramenta é WASM, não um processo Node. Quem instala o
arquivo pronto não precisa dessa cadeia de build.”

**Aponte:** `src/index.ts`, os três arquivos do frontend e o resultado
`.mlxplugin.json`. Distingua SDK **2.0.0** de manifesto **API v1**: são versões de
contratos diferentes. Mudanças nos fontes exigem novo build/pack e reinstalação.

## 5. Uma ferramenta em poucas linhas

“A definição de uma ferramenta liga quatro coisas: um nome, uma descrição, um
schema de entrada e a função que produz o resultado. A descrição orienta o
modelo sobre quando usá-la; o schema é conferido antes da execução. A interface
chama essa ferramenta pelo SDK, e o agente a encontra com um nome que inclui o
ID do plugin. Operações que escrevem dados precisam ser declaradas mutantes.”

**Aponte:** a relação entre o botão e o nome da ferramenta, sem ler o código
inteiro. O cliente do frontend é assíncrono, com Promises; o cliente dentro do
WASM é síncrono. Testes de fixtures exercitam a lógica JavaScript e o contrato das
chamadas simuladas; instalar o WASM testa outra camada da integração.

## 6. O usuário decide o alcance

“Declarar uma permissão não libera automaticamente todos os recursos. O usuário
autoriza serviços, escolhe documentos e decide se o plugin pode criar novos
projetos. Serviços externos usam HTTPS; rede local exige uma opção explícita.
Para APIs com chave, a configuração recebe o segredo e o host o injeta na
requisição. O plugin só consulta se a credencial está cadastrada. Essas escolhas
de recursos são separadas por workspace.”

**Aponte:** serviços, seleção de projetos e autorização de criação. Não confunda
`documents:read` com edição. Se uma permissão declarada for revogada, o plugin
fica bloqueado até nova concessão. Mudanças nas declarações de serviços ou
permissões exigem revisão das autorizações de recursos.

## 7. Instalação e configuração — vídeo 1

“Nesta gravação, a interface real mostra o caminho da instalação à configuração.
Observem a revisão do pacote e os controles que definem o alcance do plugin.
Instalar a ferramenta e autorizar um serviço são etapas distintas. Para o usuário,
essas escolhas ficam visíveis na mesma central.”

**Reproduza:** `01-instalar-configurar.mp4`, usando a transcrição como apoio.
Durante o vídeo, indique apenas ações efetivamente visíveis. Se o tempo estiver
curto, pause no quadro da configuração e explique o vínculo entre a permissão e
o recurso escolhido. Não use uma chave real para ilustrar o campo de credencial.

## 8. Pesquisa real, documento editável — vídeo 2

“O Reading Notes 1.1.0 conecta uma função útil a esse contrato. Ele consulta a Open
Library, uma API pública que não exige chave neste exemplo, e devolve até cinco
registros. O resultado contém título, autoria, ano quando disponível e fonte.
A ação Criar caderno de leitura faz uma nova consulta e compõe capa, referências
e próximos passos. Aqui são cinco slides reais, com textos e formas editáveis e
fontes completas no briefing e nas notas. A diagramação inicial é feita pela lógica
do plugin, sem modelo. Não há leitura do texto integral nem resumo inventado dos livros.”

**Reproduza:** `02-servico-documento.mp4`. Aponte os slides preenchidos e a abertura
real no Studio. Explique que a reprodução do vídeo é offline, mas a consulta
original ao serviço exige internet. O caderno tem entre três e sete slides,
dependendo dos registros; cinco é o resultado desta demonstração. Criar consulta
novamente, então os resultados podem mudar entre pesquisa e criação.

## 9. Um projeto, diferentes entregas — vídeo 3

“No Studio, o documento pode ser trabalhado pelo mouse, pelo código ou pelo chat.
Imagens e paletas ajudam a orientar o visual. A exportação nasce da mesma
estrutura editável: o JSON guarda o projeto completo; o PDF preserva o visual em páginas rasterizadas; o PPTX nativo
mantém textos e outros elementos editáveis; o HTML reproduz as animações
suportadas. PDF e PPTX são estáticos. Não estamos tentando reconstruir um
PowerPoint a partir dos pixels de um PDF.”

**Reproduza:** `03-studio-exportar.mp4`. Diferencie exportação disponível de arquivo
efetivamente aberto no vídeo. Antes de iniciar, diga: “Este terceiro trecho usa um
exemplo editorial preparado separadamente, com cinco slides e duas ilustrações
geradas. Vamos mostrar edição e exportação.” Os créditos identificam a procedência;
não atribua esse projeto ao Reading Notes ou a uma nova execução do Qwen.
A opção PPTX visual usa uma imagem por slide para
preservar CSS, com perda da edição individual dos elementos. O preview e os
exportadores atuais precisam do editor aberto.

## 10. A IA usa as mesmas ferramentas

“A mesma ferramenta pode ser acionada no Agent, desde que esteja instalada,
ativa e autorizada. O modelo precisa oferecer suporte a ferramentas, e o catálogo
e as políticas continuam valendo. Uma edição exige modo Full e uma ferramenta
declarada mutante. Para composição no Studio, a IA recebe um processo em etapas:
planejamento, conteúdo por slide, revisão editorial e validação visual pelo
aplicativo.”

**Aponte:** descrição/schema → chamada → resultado. O perfil de referência para
o Mac M4 com 24 GB é Qwen 3.5 9B, com contexto de 8192 tokens no Studio. Isso não
significa que todos os vídeos mostrem nova inferência desse modelo. O modelo de
texto recebe descrições das imagens; não se deve afirmar que ele avaliou seus
pixels. O suporte no Agent também não habilita ferramentas em todo chat comum.

## 11. Engenharia com limites explícitos

“A base histórica registra 311 testes Rust, 85 testes JavaScript e quatro fixtures,
além do CI aprovado daquela revisão. Para esta atualização, o registro inclui 67
testes do Studio, três fixtures e oito testes do Reading Notes, mais a criação de
cadernos pelo WASM no daemon real. São escopos distintos. O Studio bloqueia
exportações com certos erros detectados, como cortes e sobreposição de texto.
Essas evidências reduzem falhas conhecidas; não demonstram cobertura total nem
substituem a conferência visual e editorial.”

**Aponte:** o link do CI e os limites do produto. CSS personalizado e contraste
sobre imagens ainda requerem inspeção humana. Não some testes de escopos
diferentes para apresentar um percentual de qualidade ou de segurança.

## 12. Equipe e fechamento

“O resultado é um caminho comum para integrar interface, ferramentas e recursos
autorizados ao MLX-Pilot. O exemplo mostrou dados chegando de um serviço,
transformando-se em um caderno editável. Vimos também um exemplo editorial
separado e seus formatos de exportação. A equipe e
as fontes estão registradas aqui; os vídeos e o roteiro podem ser compartilhados
junto com a apresentação.”

**Aponte:** os seis perfis GitHub. Cite os integrantes sem atribuir individualmente
funções que não foram documentadas. Reserve as perguntas para demonstrar uma
decisão: permissões, revisão do documento, contrato do SDK ou formatos de saída.

## Respostas curtas para perguntas frequentes

| Pergunta | Resposta |
| --- | --- |
| “O plugin roda em Node?” | Node participa do desenvolvimento. A ferramenta instalada executa em WASM; a interface é HTML/CSS/JS. |
| “Qualquer plugin pode ler todos os projetos?” | Não. O host exige permissões e IDs de documentos autorizados naquele workspace. |
| “O modelo recebe a chave da API?” | O SDK não retorna a chave. O host faz a injeção usando o cofre local. |
| “É tudo offline?” | Assistir a esta apresentação, editar e exportar os arquivos locais pode ser offline. Consultar uma API externa requer rede. |
| “O PDF mantém a animação?” | Não. PDF e PPTX são estáticos; as animações suportadas ficam no HTML. |
| “Isso resume automaticamente os livros?” | O Reading Notes trabalha com metadados do catálogo e fontes; não acessa o texto integral. |
| “A validação garante um resultado profissional?” | Ela detecta alguns defeitos mensuráveis. Qualidade visual, clareza e veracidade ainda precisam de revisão. |
| “O CI verde prova que não há bugs?” | Não. Mostra que os checks daquela revisão passaram nos ambientes e cenários executados. |

## Transcrição — vídeo 1

**Arquivo:** `01-instalar-configurar.mp4`. **Duração:** 16,000 segundos.
**Formato:** 1440 × 900, 30 quadros/s, H.264/MP4, sem áudio, cortes ou aceleração.
As legendas abaixo foram copiadas do [manifesto final](assets/media-manifest.json);
os tempos são apresentados em milissegundos, como nos arquivos VTT.

| Intervalo | Legenda descritiva |
| --- | --- |
| 00:00,000–00:03,637 | Adicione o pacote Reading Notes e revise as permissões e o serviço declarado. |
| 00:03,637–00:07,461 | Conceda as permissões de rede e documentos e instale o plugin. |
| 00:07,461–00:09,275 | Abra Configurar e autorize a Open Library para consultas HTTPS. |
| 00:09,275–00:11,839 | Selecione o projeto permitido e autorize a criação de novas apresentações. |
| 00:11,839–00:16,000 | Salve a configuração. O acesso permanece sob controle do usuário. |

O trecho usa a aplicação e o daemon reais, sem cortes nem aceleração. O estado **Degradado** eventualmente visível corresponde ao modelo desativado no ambiente isolado; as operações de plugins continuam sendo executadas pelo daemon.

## Transcrição — vídeo 2

**Arquivo:** `02-servico-documento.mp4`. **Duração:** 26,500 segundos.
**Formato:** 1440 × 900, 30 quadros/s, H.264/MP4, sem áudio, cortes ou aceleração.
As legendas abaixo foram copiadas do [manifesto final](assets/media-manifest.json);
os tempos são apresentados em milissegundos, como nos arquivos VTT.

| Intervalo | Legenda descritiva |
| --- | --- |
| 00:00,000–00:02,942 | Pesquise livros sobre “urban gardens” na interface do Reading Notes. |
| 00:02,942–00:06,448 | O broker consulta a Open Library por HTTPS e retorna metadados reais. |
| 00:06,448–00:09,132 | Crie um caderno de leitura com capa, referências e fontes preservadas. |
| 00:09,132–00:12,402 | Abra o projeto recém-criado: o resultado já tem cinco slides editáveis. |
| 00:12,402–00:17,072 | Confira títulos, autoria, anos e referências nas páginas diagramadas. |
| 00:17,072–00:19,501 | O roteiro de leitura orienta a continuação do trabalho no mesmo documento. |
| 00:19,501–00:26,500 | Apresente com animações no HTML e continue a edição no Studio. |

A consulta HTTPS e a criação do caderno ocorreram de verdade na gravação. O Reading Notes 1.1.0 compõe o documento sem inferência de modelo. A reprodução offline do MP4 não faz outra consulta. O caderno criado está disponível em `assets/exemplos/caderno-leitura.json` e `.html`.

## Transcrição — vídeo 3

**Arquivo:** `03-studio-exportar.mp4`. **Duração:** 33,733 segundos.
**Formato:** 1440 × 900, 30 quadros/s, H.264/MP4, sem áudio, cortes ou aceleração.
As legendas abaixo foram copiadas do [manifesto final](assets/media-manifest.json);
os tempos são apresentados em milissegundos, como nos arquivos VTT.

| Intervalo | Legenda descritiva |
| --- | --- |
| 00:00,000–00:04,114 | No Studio, selecione um texto com o mouse e edite pelo inspetor. |
| 00:04,114–00:08,123 | O projeto usa cinco slides editáveis e duas imagens incorporadas. |
| 00:08,123–00:21,186 | Apresentar abre o player HTML com transições e animações de entrada. |
| 00:21,186–00:25,721 | PDF estático: a exportação preserva a composição visual. |
| 00:25,721–00:28,166 | PPTX nativo: textos, imagens e formas continuam editáveis. |
| 00:28,166–00:30,458 | HTML animado: imagens e player ficam incorporados para abrir offline. |
| 00:30,458–00:33,733 | Projeto JSON: reimporte no Studio para continuar a autoria. |

O projeto editorial tem cinco slides e duas ilustrações geradas, com procedência em [Fontes e créditos](FONTES-E-CREDITOS.md). Foi preparado separadamente do caderno do segundo vídeo e não representa uma nova inferência do Qwen. Os quatro arquivos reais estão em `assets/exemplos/`, com links no [README](README.md).
