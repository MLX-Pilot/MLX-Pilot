// Grava um video da aba Workflows executando um fluxo de ponta a ponta na
// interface real do MLX Pilot, contra o daemon real — a execucao do video e
// de verdade, incluindo a chamada ao modelo.
//
// Uso:
//   node scripts/record-workflow-demo.mjs [uiUrl] [daemonUrl] [nomeDoFluxo]
//
// Precisa de um servidor estatico servindo apps/desktop-ui/ui e do daemon no ar.
// O video sai em docs/media/ como .webm; converta com ffmpeg se quiser mp4.

import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// O playwright nao esta instalado na raiz do repositorio, e a resolucao de ESM
// nao olha NODE_PATH. Procura nos lugares onde ele costuma estar antes de
// desistir, para o script rodar sem precisar de um install proprio.
const require = createRequire(import.meta.url);
function loadPlaywright() {
  const candidates = [
    path.join(__dirname, "..", "node_modules", "playwright"),
    path.join(__dirname, "..", "temp", "node_modules", "playwright"),
    path.join(__dirname, "..", "apps", "desktop-ui", "node_modules", "playwright"),
    "playwright",
  ];
  for (const candidate of candidates) {
    try {
      return require(candidate);
    } catch {
      /* tenta o proximo */
    }
  }
  throw new Error(
    "playwright nao encontrado. Instale com `npm i -D playwright` ou rode a partir de um diretorio que ja o tenha.",
  );
}
const { chromium } = loadPlaywright();
const OUT_DIR = path.join(__dirname, "..", "docs", "media");

const UI_URL = process.argv[2] || "http://127.0.0.1:5610";
const DAEMON_URL = process.argv[3] || "http://127.0.0.1:11500";
const FLOW_NAME = process.argv[4] || "Triagem de chamados com IA";

const VIEWPORT = { width: 1440, height: 900 };

// Ritmo do video: pausas curtas demais deixam a gravacao ilegivel.
const BEAT = 900;
const pause = (ms = BEAT) => new Promise((resolve) => setTimeout(resolve, ms));

// Cursor sintetico: o Playwright nao desenha o ponteiro no video, entao sem
// isso os cliques ficam invisiveis para quem assiste.
const CURSOR_SCRIPT = `
  window.addEventListener('DOMContentLoaded', () => {
    const dot = document.createElement('div');
    dot.id = '__demo_cursor';
    dot.style.cssText = [
      'position:fixed', 'z-index:2147483647', 'pointer-events:none',
      'width:18px', 'height:18px', 'margin:-9px 0 0 -9px', 'border-radius:50%',
      'border:2px solid #00d4ff', 'background:rgba(0,212,255,0.25)',
      'transition:transform 80ms linear', 'left:0', 'top:0',
    ].join(';');
    document.body.appendChild(dot);
    document.addEventListener('mousemove', (event) => {
      dot.style.transform = 'translate(' + event.clientX + 'px,' + event.clientY + 'px)';
    }, true);
    document.addEventListener('mousedown', () => {
      dot.style.background = 'rgba(0,212,255,0.6)';
      setTimeout(() => { dot.style.background = 'rgba(0,212,255,0.25)'; }, 180);
    }, true);
  });
`;

/// Move o mouse ate o elemento e clica, para o cursor sintetico acompanhar.
async function clickVisible(page, locator) {
  await locator.scrollIntoViewIfNeeded();
  const box = await locator.boundingBox();
  if (!box) throw new Error("elemento sem caixa visivel");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 18 });
  await pause(320);
  await page.mouse.down();
  await page.mouse.up();
}

async function main() {
  fs.mkdirSync(OUT_DIR, { recursive: true });

  const browser = await chromium.launch({ args: ["--force-color-profile=srgb"] });
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: 1,
    recordVideo: { dir: OUT_DIR, size: VIEWPORT },
  });

  // A UI e servida estaticamente, entao precisa saber onde esta o daemon.
  await context.addInitScript(`try { localStorage.setItem('mlxPilotDaemonUrl', ${JSON.stringify(DAEMON_URL)}); } catch {}`);
  await context.addInitScript(CURSOR_SCRIPT);

  const page = await context.newPage();
  await page.goto(UI_URL, { waitUntil: "domcontentloaded" });

  // O splash some sozinho quando o daemon responde /runtime/startup.
  await page.waitForSelector(".tab[data-panel='workflows']", { timeout: 30000 });
  await page.waitForFunction(
    () => !document.querySelector(".splash") || getComputedStyle(document.querySelector(".splash")).display === "none",
    { timeout: 30000 },
  ).catch(() => page.evaluate(() => document.querySelector(".splash")?.remove()));
  await pause(1200);

  await clickVisible(page, page.locator(".tab[data-panel='workflows']"));
  await page.waitForSelector("#flow-node-palette .workflow-palette-item", { timeout: 20000 });
  await pause(1400);

  // Abre o fluxo da demonstracao a partir da lista de fluxos salvos.
  const row = page.locator(".flow-list-row", { hasText: FLOW_NAME }).first();
  await row.waitFor({ timeout: 20000 });
  await clickVisible(page, row.locator("[data-flow-open]"));
  await page.waitForFunction(
    (name) => document.getElementById("flow-editor-name")?.value === name,
    FLOW_NAME,
    { timeout: 20000 },
  );
  await pause(900);

  await page.evaluate(() => document.getElementById("flow-builder")?.scrollIntoView({ block: "start" }));
  await pause(600);
  await clickVisible(page, page.locator("#flow-fit-btn"));
  await pause(1600);

  // Abre o no de IA para mostrar provedor e modelo herdados do app.
  const aiNode = page.locator("#flow-nodes .workflow-node", { hasText: "Classificar com IA" }).first();
  await clickVisible(page, aiNode);
  await page.waitForSelector("[data-field-key='model_id']", { timeout: 10000 });
  await pause(2400);

  // Payload de teste, digitado para aparecer no video.
  const payload = page.locator("#flow-run-payload");
  await payload.scrollIntoViewIfNeeded();
  await pause(500);
  await clickVisible(page, payload);
  await payload.fill("");
  await payload.type('{ "texto": "O servidor de producao caiu e ninguem consegue acessar o sistema" }', { delay: 18 });
  await pause(900);

  await page.evaluate(() => document.getElementById("flow-builder")?.scrollIntoView({ block: "start" }));
  await pause(700);

  // Executa de verdade: o no de IA chama o modelo local.
  await clickVisible(page, page.locator("#flow-run-btn"));
  await pause(600);

  await page.waitForFunction(
    () => {
      const box = document.getElementById("flow-run-result");
      return box && !box.hidden && /Sucesso|Falhou/.test(box.textContent || "");
    },
    { timeout: 180000 },
  );
  await pause(2200);

  // O canvas mostra o resultado por no e o ramo podado.
  await page.evaluate(() => document.getElementById("flow-builder")?.scrollIntoView({ block: "start" }));
  await pause(2600);

  // Abre o no do ramo escolhido para mostrar a saida daquele no.
  const branch = page.locator("#flow-nodes .workflow-node", { hasText: "Fila prioritaria" }).first();
  if (await branch.count()) {
    await clickVisible(page, branch);
    await pause(2600);
  }

  // Desce ate o painel de execucao: tabela por no, logs e saida final.
  await page.evaluate(() => document.getElementById("flow-run-result")?.scrollIntoView({ block: "center" }));
  await pause(3200);

  const details = page.locator(".flow-run-payload-details summary").first();
  if (await details.count()) {
    await clickVisible(page, details);
    await pause(2800);
  }

  await pause(1200);
  await context.close();
  await browser.close();

  const files = fs.readdirSync(OUT_DIR).filter((file) => file.endsWith(".webm"));
  const newest = files
    .map((file) => ({ file, mtime: fs.statSync(path.join(OUT_DIR, file)).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime)[0];
  if (newest) {
    const target = path.join(OUT_DIR, "workflow-demo.webm");
    if (path.join(OUT_DIR, newest.file) !== target) {
      fs.rmSync(target, { force: true });
      fs.renameSync(path.join(OUT_DIR, newest.file), target);
    }
    console.log(`video: ${target}`);
  }
}

main().catch((error) => {
  console.error("falhou:", error.message);
  process.exit(1);
});
