#!/usr/bin/env python3
"""Build the Orbital Command SVG sources using the bundled, OFL-licensed Outfit.

Requires fonttools[woff]==4.67.0. No network or installed system font is used.
Run from any directory, then run generate-brand.cjs for PNG/ICO/ICNS derivatives.
The hexagon, circular core and palette come from the application's original SVG.
"""

from pathlib import Path
from xml.sax.saxutils import escape

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "apps/desktop-ui/ui/assets"
font = instantiateVariableFont(
    TTFont(ASSETS / "fonts/outfit-variable.woff2"), {"wght": 700}, inplace=True
)
glyphs = font.getGlyphSet()
cmap = font.getBestCmap()
scale = 80 / font["head"].unitsPerEm
cursor = 120.0
paths = []
for char in "MLX Pilot":
    name = cmap[ord(char)]
    pen = SVGPathPen(glyphs)
    glyphs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, cursor, 84)))
    if pen.getCommands():
        paths.append(f'    <path d="{escape(pen.getCommands())}"/>')
    cursor += font["hmtx"][name][0] * scale + (0 if char == " " else 1.2)
width = round(cursor + 14)

mark = '''  <defs>
    <linearGradient id="orbital-gradient" x1="6" y1="4" x2="58" y2="60" gradientUnits="userSpaceOnUse">
      <stop stop-color="#00d4ff"/>
      <stop offset="1" stop-color="#7b61ff"/>
    </linearGradient>
  </defs>
  <path d="M32 4 58 18 58 46 32 60 6 46 6 18Z" fill="none" stroke="url(#orbital-gradient)" stroke-width="2.5" stroke-linejoin="round"/>
  <path d="M42 32a10 10 0 1 1-20 0 10 10 0 0 1 20 0ZM36 32a4 4 0 1 0-8 0 4 4 0 0 0 8 0Z" fill="url(#orbital-gradient)" fill-rule="evenodd"/>
  <path d="M32 18v4M32 42v4" fill="none" stroke="#00d4ff" stroke-width="1.5" stroke-linecap="round" opacity=".6"/>'''

icon = f'''<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64" role="img" aria-labelledby="icon-title">
  <title id="icon-title">MLX Pilot — símbolo orbital</title>
  <desc>Hexágono e núcleo circular da interface Orbital Command. Ciano e violeta sobre fundo transparente.</desc>
{mark}
</svg>
'''
wordmark_body = f'''  <svg x="8" y="8" width="96" height="96" viewBox="0 0 64 64">
{mark}
  </svg>
  <defs>
    <linearGradient id="wordmark-ink" x1="120" y1="30" x2="{width - 14}" y2="84" gradientUnits="userSpaceOnUse">
      <stop stop-color="#00d4ff"/>
      <stop offset="1" stop-color="#e8edf5"/>
    </linearGradient>
  </defs>
  <g fill="url(#wordmark-ink)">
{chr(10).join(paths)}
  </g>'''
wordmark = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="112" viewBox="0 0 {width} 112" role="img" aria-labelledby="wordmark-title">
  <title id="wordmark-title">MLX Pilot</title>
  <desc>Marca Orbital Command para fundos escuros. Letras Outfit Bold convertidas em curvas; nenhuma fonte externa é necessária.</desc>
  <metadata>Lettering: Outfit 700, SIL Open Font License 1.1. Source and license: fonts/outfit-variable.woff2 and fonts/outfit-variable-OFL.txt. Regenerate with scripts/generate-brand-vectors.py.</metadata>
{wordmark_body}
</svg>
'''
logo_width = 900
logo_height = 112 * logo_width / width
banner = f'''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="320" viewBox="0 0 1280 320" role="img" aria-labelledby="banner-title">
  <title id="banner-title">MLX Pilot</title>
  <desc>Marca completa sobre o painel escuro Orbital Command, legível em páginas claras ou escuras. SVG autocontido.</desc>
  <defs>
    <radialGradient id="cyan-atmosphere" cx="0" cy="0" r="1" gradientTransform="translate(190 12) scale(670 420)" gradientUnits="userSpaceOnUse"><stop stop-color="#00d4ff" stop-opacity=".09"/><stop offset="1" stop-color="#00d4ff" stop-opacity="0"/></radialGradient>
    <radialGradient id="violet-atmosphere" cx="0" cy="0" r="1" gradientTransform="translate(1110 300) scale(650 390)" gradientUnits="userSpaceOnUse"><stop stop-color="#7b61ff" stop-opacity=".11"/><stop offset="1" stop-color="#7b61ff" stop-opacity="0"/></radialGradient>
  </defs>
  <rect width="1280" height="320" rx="24" fill="#06090f"/>
  <rect width="1280" height="320" rx="24" fill="url(#cyan-atmosphere)"/>
  <rect width="1280" height="320" rx="24" fill="url(#violet-atmosphere)"/>
  <rect x=".5" y=".5" width="1279" height="319" rx="23.5" fill="none" stroke="#8899b0" stroke-opacity=".14"/>
  <svg x="190" y="{(320 - logo_height) / 2:.3f}" width="{logo_width}" height="{logo_height:.3f}" viewBox="0 0 {width} 112">
{wordmark_body}
  </svg>
</svg>
'''

for name, content in {
    "mlxpilot-icon.svg": icon,
    "mlxpilot-mark.svg": icon,
    "mlxpilot-wordmark.svg": wordmark,
    "mlxpilot-banner.svg": banner,
}.items():
    (ASSETS / name).write_text(content, encoding="utf-8")
print(f"Generated four self-contained SVG sources; wordmark {width} × 112.")
