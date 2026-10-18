#!/usr/bin/env node
/*
 * Rasterize the committed Orbital Command SVGs and refresh tracked app icons.
 * Requires Node.js 20.9+, sharp 0.35.5 and macOS iconutil (for the native ICNS).
 * No network, system fonts or image-generation service is used during this build.
 *
 * Install the build tool once without changing application dependencies:
 *   npm install --prefix temp/brand-tools sharp@0.35.5
 * Then regenerate (SVG sources are already committed):
 *   NODE_PATH="$PWD/temp/brand-tools/node_modules" node scripts/generate-brand.cjs
 * To also rebuild lettering from the bundled OFL font, first run:
 *   python3 scripts/generate-brand-vectors.py
 */

const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const { execFileSync } = require('node:child_process');
const sharp = require('sharp');

const root = path.resolve(__dirname, '..');
const assets = path.join(root, 'apps/desktop-ui/ui/assets');
const docsAssets = path.join(root, 'docs/apresentacao-plugins/assets');
const icons = path.join(root, 'apps/desktop-ui/src-tauri/icons');
const scratch = path.join(root, 'temp/brand');

function embeddedMark(icon, x, y, width) {
  const content = icon.replace(/^<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '');
  return `<svg x="${x}" y="${y}" width="${width}" height="${width}" viewBox="0 0 64 64">${content}</svg>`;
}

function nativeArtwork(icon, variant = 'desktop') {
  const background = variant === 'foreground' ? ''
    : variant === 'round' ? '<circle cx="512" cy="512" r="512" fill="url(#native-panel)"/>'
    : variant === 'ios' || variant === 'android' ? '<rect width="1024" height="1024" fill="url(#native-panel)"/>'
    : '<rect x="64" y="64" width="896" height="896" rx="196" fill="url(#native-panel)"/><rect x="65" y="65" width="894" height="894" rx="195" fill="none" stroke="#8899b0" stroke-opacity=".16" stroke-width="2"/>';
  // Android adaptive foregrounds retain the platform's central safe zone.
  const extent = variant === 'foreground' ? 640 : 768;
  const inset = (1024 - extent) / 2;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><defs><linearGradient id="native-panel" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#141c2b"/><stop offset="1" stop-color="#06090f"/></linearGradient></defs>${background}${embeddedMark(icon, inset, inset, extent)}</svg>`;
}

async function render(svg, width, height = width, opaque = false) {
  let image = sharp(Buffer.from(svg), { density: 144 }).resize(width, height);
  if (opaque) image = image.flatten({ background: '#06090f' });
  return image.png({ compressionLevel: 9 }).toBuffer();
}

function windowsIcon(images) {
  // Windows Vista+ accepts PNG images in ICO entries, preserving full RGBA.
  const header = Buffer.alloc(6 + images.length * 16);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach(({ size, png }, index) => {
    const at = 6 + index * 16;
    header[at] = size === 256 ? 0 : size;
    header[at + 1] = size === 256 ? 0 : size;
    header.writeUInt16LE(1, at + 4);
    header.writeUInt16LE(32, at + 6);
    header.writeUInt32LE(png.length, at + 8);
    header.writeUInt32LE(offset, at + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...images.map(({ png }) => png)]);
}

async function main() {
  if (process.platform !== 'darwin') throw new Error('Run the full native icon build on macOS; iconutil produces the ICNS bundle.');
  const icon = await fs.readFile(path.join(assets, 'mlxpilot-icon.svg'), 'utf8');
  const banner = await fs.readFile(path.join(assets, 'mlxpilot-banner.svg'), 'utf8');
  const desktop = nativeArtwork(icon);
  await fs.mkdir(docsAssets, { recursive: true });
  await fs.mkdir(scratch, { recursive: true });
  for (const name of ['mlxpilot-icon.svg', 'mlxpilot-mark.svg', 'mlxpilot-wordmark.svg', 'mlxpilot-banner.svg']) {
    await fs.copyFile(path.join(assets, name), path.join(docsAssets, name));
  }
  const wordmarkPng = await render(banner, 1280, 320);
  await fs.writeFile(path.join(assets, 'mlxpilot-wordmark.png'), wordmarkPng);
  await fs.writeFile(path.join(docsAssets, 'mlxpilot-wordmark.png'), wordmarkPng);
  const masterIcon = await render(desktop, 1024);
  await fs.writeFile(path.join(assets, 'mlxpilot-mark.png'), masterIcon);
  await fs.writeFile(path.join(root, 'apps/desktop-ui/src-tauri/app-icon.png'), masterIcon);

  // Regenerate only the existing tracked platform artifacts, preserving dimensions.
  const tracked = execFileSync('git', ['ls-files', 'apps/desktop-ui/src-tauri/icons'], { cwd: root, encoding: 'utf8' }).trim().split('\n');
  let pngCount = 0;
  for (const relative of tracked.filter((file) => file.endsWith('.png'))) {
    const target = path.join(root, relative);
    const { width, height } = await sharp(target).metadata();
    if (!width || !height) throw new Error(`Missing dimensions: ${relative}`);
    const variant = relative.includes('/ios/') ? 'ios'
      : relative.endsWith('ic_launcher_foreground.png') ? 'foreground'
      : relative.endsWith('ic_launcher_round.png') ? 'round'
      : relative.includes('/android/') ? 'android' : 'desktop';
    await fs.writeFile(target, await render(nativeArtwork(icon, variant), width, height, variant === 'ios'));
    pngCount += 1;
  }
  await fs.writeFile(path.join(icons, 'android/values/ic_launcher_background.xml'), '<?xml version="1.0" encoding="utf-8"?>\n<resources>\n  <color name="ic_launcher_background">#06090f</color>\n</resources>\n');

  const icoSizes = [16, 24, 32, 48, 64, 256];
  const icoImages = [];
  for (const size of icoSizes) icoImages.push({ size, png: await render(desktop, size) });
  await fs.writeFile(path.join(icons, 'icon.ico'), windowsIcon(icoImages));

  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'mlx-pilot-icons-'));
  try {
    const iconset = path.join(temporary, 'MLXPilot.iconset');
    await fs.mkdir(iconset);
    for (const size of [16, 32, 128, 256, 512]) {
      for (const ratio of [1, 2]) {
        const suffix = ratio === 2 ? '@2x' : '';
        await fs.writeFile(path.join(iconset, `icon_${size}x${size}${suffix}.png`), await render(desktop, size * ratio));
      }
    }
    execFileSync('/usr/bin/iconutil', ['-c', 'icns', '-o', path.join(icons, 'icon.icns'), iconset]);
  } finally {
    await fs.rm(temporary, { recursive: true, force: true });
  }

  const relativeAssets = '../../apps/desktop-ui/ui/assets';
  const iconRow = [16, 24, 32, 48, 64, 128].map((size) => `<figure><img src="${relativeAssets}/mlxpilot-icon.svg" width="${size}" height="${size}" alt="Ícone ${size}px"><figcaption>${size}px</figcaption></figure>`).join('');
  await fs.writeFile(path.join(scratch, 'brand-preview.html'), `<!doctype html><html lang="pt-BR"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>MLX Pilot — marca Orbital Command</title><style>*{box-sizing:border-box}body{margin:0;background:#06090f;color:#e8edf5;font:16px system-ui,sans-serif}main{max-width:1200px;margin:auto;padding:48px}h1{font-size:18px;font-weight:500;color:#8899b0}.panel{padding:36px;margin:24px 0;border:1px solid #26324a;border-radius:24px;background:#0a0e17}.wordmark{width:min(100%,800px);height:auto}.light{background:#f1f3f7;color:#06090f}.light img{max-width:100%}.icons{display:flex;align-items:center;gap:32px;flex-wrap:wrap}figure{margin:0;text-align:center}figcaption{color:#8899b0;margin-top:10px;font-size:12px}.native{width:160px;height:160px}a{color:#00d4ff}</style><main><h1>MLX Pilot · fontes vetoriais da identidade existente</h1><section class="panel"><img class="wordmark" src="${relativeAssets}/mlxpilot-wordmark.svg" alt="MLX Pilot"></section><section class="panel light"><img src="${relativeAssets}/mlxpilot-banner.svg" alt="Banner MLX Pilot sobre fundo claro"></section><section class="panel icons">${iconRow}<figure><img class="native" src="${relativeAssets}/mlxpilot-mark.png" alt="Ícone nativo em painel escuro"><figcaption>Tile nativo</figcaption></figure></section><p>Fontes convertidas em curvas. SVGs autocontidos, sem carregamento externo.</p></main></html>`);
  await fs.writeFile(path.join(scratch, 'native-preview.png'), masterIcon);
  console.log(`Updated canonical SVG copies, two wordmark PNGs, native masters, ${pngCount} tracked PNG icons, ICO and ICNS. Preview: temp/brand/brand-preview.html`);
}

main().catch((error) => { console.error(error.message); process.exitCode = 1; });
