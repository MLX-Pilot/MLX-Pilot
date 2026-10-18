#!/usr/bin/env node
import { readFile, realpath, stat, open } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Script } from "node:vm";

const directory = path.dirname(fileURLToPath(import.meta.url));
const root = await realpath(directory);
const errors = [];
const resources = new Map();
const expectedVideos = [
  "01-instalar-configurar",
  "02-servico-documento",
  "03-studio-exportar",
];
const check = (condition, message) => {
  if (!condition) errors.push(message);
};
const display = (filename) => path.relative(root, filename) || "index.html";
const decode = (value) =>
  value.replace(/&(?:amp|quot|apos|lt|gt);/g, (entity) => {
    return {
      "&amp;": "&",
      "&quot;": '"',
      "&apos;": "'",
      "&lt;": "<",
      "&gt;": ">",
    }[entity];
  });

function attributes(source) {
  const values = {};
  for (const match of source.matchAll(
    /([^\s=<>/]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g,
  )) {
    values[match[1].toLowerCase()] = decode(
      match[2] ?? match[3] ?? match[4] ?? "",
    );
  }
  return values;
}

async function read(filename) {
  try {
    return await readFile(filename, "utf8");
  } catch (error) {
    errors.push(`${display(filename)}: não foi possível ler (${error.code}).`);
    return "";
  }
}

async function localReference(
  value,
  source,
  context,
  { external = false } = {},
) {
  if (external && /^(https?:|mailto:)/i.test(value)) return;
  check(Boolean(value), `${context}: referência vazia.`);
  if (!value || value.startsWith("#")) return;
  if (/^(?:[a-z][a-z\d+.-]*:|\/|\\)/i.test(value)) {
    errors.push(`${context}: use um caminho relativo local (${value}).`);
    return;
  }
  let relative;
  try {
    relative = decodeURIComponent(value.split(/[?#]/, 1)[0]);
  } catch {
    errors.push(`${context}: caminho com codificação inválida.`);
    return;
  }
  const filename = path.resolve(path.dirname(source), relative);
  const boundary = path.relative(root, filename);
  if (boundary.startsWith("..") || path.isAbsolute(boundary)) {
    errors.push(`${context}: referência sai da pasta portátil (${value}).`);
    return;
  }
  try {
    const actual = await realpath(filename);
    const actualBoundary = path.relative(root, actual);
    check(
      !actualBoundary.startsWith("..") && !path.isAbsolute(actualBoundary),
      `${context}: symlink aponta para fora da pasta portátil.`,
    );
    const info = await stat(filename);
    check(
      info.isFile() && info.size > 0,
      `${context}: arquivo vazio ou inválido.`,
    );
    resources.set(filename, info.size);
    return filename;
  } catch (error) {
    errors.push(`${context}: arquivo indisponível (${value}; ${error.code}).`);
  }
}

async function inspectCss(source, filename) {
  const css = source.replace(/\/\*[\s\S]*?\*\//g, "");
  check(!/@import\b/i.test(css), `${display(filename)}: não use @import.`);
  for (const match of css.matchAll(/url\(\s*(["']?)(.*?)\1\s*\)/gi)) {
    if (match[2].startsWith("#")) continue;
    await localReference(match[2], filename, `${display(filename)}: url()`);
  }
}

function inspectJs(source, filename) {
  try {
    new Script(source, { filename: display(filename) });
  } catch (error) {
    errors.push(
      `${display(filename)}: JavaScript inválido (${error.message}).`,
    );
  }
  check(
    !/\b(?:fetch|XMLHttpRequest|WebSocket|EventSource|sendBeacon|importScripts)\s*\(|\bimport\s*(?:\(|["'{*])/m.test(
      source,
    ),
    `${display(filename)}: a apresentação não pode depender de requisições ou imports.`,
  );
  check(
    !/https?:\/\//i.test(source),
    `${display(filename)}: remova URLs remotas do JavaScript executável.`,
  );
}

async function inspectMp4(filename) {
  const handle = await open(filename, "r");
  try {
    const { size } = await handle.stat();
    const atoms = [];
    let offset = 0;
    while (offset + 8 <= size) {
      const header = Buffer.alloc(16);
      await handle.read(header, 0, Math.min(16, size - offset), offset);
      let length = header.readUInt32BE(0);
      const type = header.toString("ascii", 4, 8);
      if (length === 1) length = Number(header.readBigUInt64BE(8));
      if (length === 0) length = size - offset;
      if (length < 8 || offset + length > size) break;
      atoms.push(type);
      offset += length;
    }
    check(atoms[0] === "ftyp", `${display(filename)}: não é um MP4 válido.`);
    check(
      atoms.includes("moov") && atoms.includes("mdat"),
      `${display(filename)}: MP4 não contém metadados e mídia completos.`,
    );
    check(
      atoms.indexOf("moov") < atoms.indexOf("mdat"),
      `${display(filename)}: falta faststart (moov deve preceder mdat).`,
    );
  } finally {
    await handle.close();
  }
}

async function inspectVtt(filename) {
  const source = await read(filename);
  check(
    /^\uFEFF?WEBVTT(?:\s|$)/.test(source),
    `${display(filename)}: falta WEBVTT.`,
  );
  const cues = [
    ...source.matchAll(/([\d:.]+)\s+-->\s+([\d:.]+)([^\n]*)\n([^\n]+)/g),
  ];
  const seconds = (timestamp) =>
    timestamp.split(":").reduce((total, part) => total * 60 + Number(part), 0);
  check(cues.length > 0, `${display(filename)}: nenhuma legenda encontrada.`);
  let previous = -1;
  for (const cue of cues) {
    const start = seconds(cue[1]);
    const end = seconds(cue[2]);
    check(
      Number.isFinite(start) && end > start && start >= previous,
      `${display(filename)}: tempos de legenda inválidos ou fora de ordem.`,
    );
    previous = start;
  }
}

const htmlFile = path.join(root, "index.html");
const html = await read(htmlFile);
check(/^\s*<!doctype html>/i.test(html), "index.html: falta <!doctype html>.");
check(
  /<html\b[^>]*\blang=["']pt(?:-BR)?["']/i.test(html),
  "index.html: idioma deve ser português.",
);
check(
  /<meta\b[^>]*\bcharset=["']?utf-8/i.test(html),
  "index.html: falta charset UTF-8.",
);
check(
  /<meta\b[^>]*\bname=["']viewport["']/i.test(html),
  "index.html: falta viewport.",
);
check(/<title>\s*\S[\s\S]*?<\/title>/i.test(html), "index.html: falta título.");
const markup = html
  .replace(/<!--[\s\S]*?-->/g, "")
  .replace(/<(script|style)\b([^>]*)>[\s\S]*?<\/\1>/gi, "<$1$2></$1>");
const tags = [...markup.matchAll(/<([a-z][\w-]*)\b([^>]*)>/gi)].map(
  (match) => ({
    tag: match[1].toLowerCase(),
    attrs: attributes(match[2]),
  }),
);
const ids = new Set();
for (const { tag, attrs } of tags) {
  if (attrs.id) {
    check(!ids.has(attrs.id), `index.html: ID repetido (${attrs.id}).`);
    ids.add(attrs.id);
  }
  check(tag !== "base", "index.html: <base> quebra as referências portáteis.");
  check(
    !["iframe", "object", "embed"].includes(tag),
    `index.html: não inclua conteúdo externo por <${tag}>.`,
  );
  check(
    !Object.keys(attrs).some((name) => /^on/i.test(name)),
    `index.html: mantenha handlers de <${tag}> no JavaScript local.`,
  );
}
for (const { tag, attrs } of tags) {
  const context = `index.html: <${tag}${attrs.id ? `#${attrs.id}` : ""}>`;
  for (const attribute of ["src", "poster", "href"]) {
    if (!(attribute in attrs)) continue;
    const value = attrs[attribute];
    if (value.startsWith("#")) {
      check(
        ids.has(value.slice(1)),
        `${context}: fragmento inexistente (${value}).`,
      );
    }
    await localReference(value, htmlFile, context, { external: tag === "a" });
  }
  for (const attribute of [
    "aria-controls",
    "aria-labelledby",
    "aria-describedby",
    "for",
  ]) {
    for (const id of (attrs[attribute] ?? "").split(/\s+/).filter(Boolean)) {
      check(
        ids.has(id),
        `${context}: ${attribute} referencia ID inexistente (${id}).`,
      );
    }
  }
  if (attrs.srcset)
    errors.push(`${context}: use src local simples, sem srcset.`);
  if (attrs.style) await inspectCss(attrs.style, htmlFile);
  if (tag === "img")
    check("alt" in attrs, `${context}: imagem sem texto alternativo.`);
  if (tag === "script") {
    check(
      attrs.type !== "module",
      `${context}: módulos não funcionam de forma portátil em file://.`,
    );
    check(
      "defer" in attrs && Boolean(attrs.src),
      `${context}: use script local com defer.`,
    );
  }
  if (tag === "video") {
    check(
      "controls" in attrs && "playsinline" in attrs,
      `${context}: vídeo precisa de controls e playsinline.`,
    );
    check(
      attrs.preload === "metadata",
      `${context}: preload deve ser metadata.`,
    );
    check(
      !("autoplay" in attrs),
      `${context}: vídeo não deve iniciar automaticamente.`,
    );
    check(Boolean(attrs.poster), `${context}: vídeo precisa de poster local.`);
  }
}
const slides = tags.filter(
  ({ tag, attrs }) =>
    tag === "section" && attrs.class?.split(/\s+/).includes("slide"),
);
check(
  slides.length === 12,
  `index.html: esperados 12 slides; encontrados ${slides.length}.`,
);
slides.forEach(({ attrs }, index) => {
  check(
    attrs.id === `slide-${index + 1}`,
    `index.html: slide ${index + 1} sem ID sequencial.`,
  );
  check(
    Boolean(attrs["data-title"]),
    `index.html: ${attrs.id} sem data-title.`,
  );
});
check(
  tags.filter(
    ({ tag, attrs }) =>
      tag === "aside" &&
      attrs.class?.split(/\s+/).includes("slide-notes") &&
      "hidden" in attrs,
  ).length === 12,
  "index.html: cada slide precisa de notas locais ocultas.",
);
check(
  tags.filter(({ tag }) => tag === "video").length === 3,
  "index.html: esperados três vídeos.",
);
for (const id of ["overview", "help", "notes-panel"])
  check(ids.has(id), `index.html: interface ausente (${id}).`);
for (const name of expectedVideos) {
  for (const extension of ["mp4", "vtt"]) {
    const filename = path.join(root, "assets", `${name}.${extension}`);
    check(
      resources.has(filename),
      `index.html: falta referência local a assets/${name}.${extension}.`,
    );
  }
}
for (const match of html.matchAll(/<style\b[^>]*>([\s\S]*?)<\/style>/gi))
  await inspectCss(match[1], htmlFile);
for (const filename of [...resources.keys()]) {
  if (filename.endsWith(".css"))
    await inspectCss(await read(filename), filename);
  if (filename.endsWith(".js")) inspectJs(await read(filename), filename);
  if (filename.endsWith(".mp4")) await inspectMp4(filename);
  if (filename.endsWith(".vtt")) await inspectVtt(filename);
}
for (const name of ["README.md", "ROTEIRO.md", "FONTES-E-CREDITOS.md"]) {
  const filename = path.join(root, name);
  const markdown = await read(filename);
  for (const match of markdown.matchAll(
    /!?\[[^\]]*\]\(([^\s)]+)(?:\s+["'][^)]*)?\)/g,
  )) {
    await localReference(match[1], filename, `${name}: link`, {
      external: true,
    });
  }
}

if (errors.length) {
  console.error(`Apresentação inválida: ${errors.length} problema(s).`);
  errors.forEach((error) => console.error(`- ${error}`));
  process.exitCode = 1;
} else {
  console.log(
    `Apresentação verificada: 12 slides, 3 MP4 com faststart e ${resources.size} arquivos locais referenciados.`,
  );
  console.log(
    "HTML, JavaScript, caminhos portáteis e legendas válidos. Confira a aparência e a reprodução no navegador antes de apresentar.",
  );
}
