# Identidade visual — Orbital Command

A marca e a apresentação de plugins seguem o tema do frontend em `apps/desktop-ui/ui/styles.css`: fundo azul-escuro, ciano e violeta, com um hexágono envolvendo o núcleo orbital. O nome usa Outfit Bold; a interface usa Manrope e os dados usam JetBrains Mono.

| Uso | Cor |
| --- | --- |
| Fundo profundo / base | `#06090f` / `#0a0e17` |
| Superfície / superfície elevada | `#0f1520` / `#141c2b` |
| Texto principal / secundário | `#e8edf5` / `#8899b0` |
| Ciano / violeta | `#00d4ff` / `#7b61ff` |

## Arquivos da marca

As fontes vetoriais ficam em [`apps/desktop-ui/ui/assets`](../apps/desktop-ui/ui/assets/):

- `mlxpilot-icon.svg`: símbolo transparente para favicon, splash e espaços compactos.
- `mlxpilot-mark.svg`: o mesmo símbolo, disponível pelo nome tradicional da marca.
- `mlxpilot-wordmark.svg`: símbolo e nome completo, para superfícies escuras. As letras estão em curvas, sem depender de fontes instaladas.
- `mlxpilot-banner.svg`: marca completa sobre painel escuro, adequada aos READMEs tanto no tema claro quanto no escuro do GitHub.
- `mlxpilot-mark.png`: matriz de 1024 × 1024 do ícone nativo, com painel escuro e margem externa transparente.
- `mlxpilot-wordmark.png`: versão PNG do banner, mantida para consumidores que precisam de raster.

Mantenha a proporção e a área livre da marca. Os arquivos de distribuição ficam em `apps/desktop-ui/src-tauri/icons/`; o conjunto inclui PNG, ICNS, ICO e variantes Android/iOS. Os ícones iOS têm fundo opaco, e o foreground adaptativo do Android preserva sua área segura central.

## Fontes e funcionamento offline

As três famílias variáveis e suas licenças SIL Open Font License 1.1 estão em [`assets/fonts`](../apps/desktop-ui/ui/assets/fonts/). Carregue `assets/fonts/fonts.css` antes dos estilos da interface. A origem e a revisão dos arquivos estão no README desse diretório.

A [apresentação de plugins](apresentacao-plugins/index.html) mantém cópias locais da marca e das fontes, além dos seus vídeos e legendas. Assim, sua pasta continua portátil e pode ser aberta diretamente pelo `index.html`, sem rede ou instalação de fontes. As capturas e os vídeos preservam a aplicação e os documentos demonstrados no momento da gravação.

## Regenerar os arquivos

Execute a partir da raiz do repositório. Para alterar o lettering ou a geometria, gere primeiro os SVGs com a fonte Outfit local:

```bash
python3 -m venv temp/brand-venv
temp/brand-venv/bin/python -m pip install 'fonttools[woff]==4.67.0'
temp/brand-venv/bin/python scripts/generate-brand-vectors.py
```

Para rasterizar os SVGs existentes e atualizar os ícones, use Node.js 20.9 ou superior e o `iconutil` do macOS:

```bash
npm install --prefix temp/brand-tools sharp@0.35.5
NODE_PATH="$PWD/temp/brand-tools/node_modules" node scripts/generate-brand.cjs
```

O segundo comando também sincroniza os quatro SVGs e o banner PNG da apresentação. Depois da instalação das ferramentas, a geração utiliza apenas arquivos locais. O preview é criado em `temp/brand/brand-preview.html`; os diretórios temporários não entram no Git.

Após uma mudança, confira a marca em tamanhos pequenos, o banner em fundo claro/escuro, os caminhos de `tauri.conf.json` e a apresentação em desktop e celular. Para verificar seus arquivos locais e vídeos:

```bash
node docs/apresentacao-plugins/verify.mjs
git diff --check
```
