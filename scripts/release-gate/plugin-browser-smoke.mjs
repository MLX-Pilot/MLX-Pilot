#!/usr/bin/env node
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import {
  assertOkJson, createHarness, isDirectRun, pickPort, printStandaloneResult, repoRoot,
  requestJson, startDaemon, startJsonServer, stopDaemon, waitForHealth, writeJson,
} from './_lib.mjs';

export const REQUIRED_PLUGIN_BROWSER_CASES = Object.freeze([
  'install', 'configure', 'search', 'document', 'conflict', 'exports', 'revoke', 'disable_enable', 'remove',
]);
const pluginApi = '/agent/wasm-plugins/reading_notes';
const searchTerm = 'Design editorial';
const book = { key: '/works/OL123W', title: 'O desenho do livro', author_name: ['Ana Silva'], first_publish_year: 2020 };
const uiRoot = path.join(repoRoot, 'apps/desktop-ui');
const fixturePath = path.join(repoRoot, 'plugins/reading-notes/reading_notes-1.1.0.mlxplugin.json');
const dimensions = { width: 1440, height: 900 };

async function serveDesktop() {
  const port = await pickPort();
  const proc = spawn(process.execPath, [path.join(repoRoot, 'scripts/static-server.mjs'), path.join(uiRoot, 'ui'), String(port)], { stdio: ['ignore', 'pipe', 'inherit'] });
  const instance = { proc, baseUrl: `http://127.0.0.1:${port}` };
  try {
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('Desktop static server startup timed out')), 10_000);
      proc.stdout.once('data', () => { clearTimeout(timer); resolve(); });
      proc.once('error', error => { clearTimeout(timer); reject(error); });
      proc.once('exit', code => { clearTimeout(timer); reject(new Error(`Desktop static server exited ${code}`)); });
    });
    return instance;
  } catch (error) {
    await stopDaemon(instance);
    throw error;
  }
}

async function verifyExport(format, filename, doc) {
  const bytes = await fs.readFile(filename);
  if (format === 'pdf') {
    const pdf = bytes.toString('latin1');
    assert.ok(pdf.startsWith('%PDF-') && pdf.trimEnd().endsWith('%%EOF'), 'Invalid PDF structure');
    assert.equal((pdf.match(/\/Type\s*\/Page\b/g) || []).length, doc.slides.length);
  } else if (format === 'pptx') {
    const uiRequire = createRequire(path.join(uiRoot, 'package.json'));
    const zip = await uiRequire('jszip').loadAsync(bytes, { checkCRC32: true });
    const slides = zip.file(/^ppt\/slides\/slide\d+\.xml$/);
    assert.equal(slides.length, doc.slides.length);
    assert.ok(zip.file('ppt/presentation.xml'));
    const xml = await Promise.all(slides.map(slide => slide.async('string')));
    assert.ok(xml.some(content => content.includes(book.title)), 'PPTX missing the source book');
  } else {
    const html = bytes.toString('utf8');
    assert.match(html, /^<!doctype html>/i);
    assert.ok(html.includes(`<title>${doc.title}</title>`) && html.includes(book.title));
  }
  return { path: filename, bytes: bytes.length, slides: doc.slides.length };
}

export async function runPluginBrowserSmoke({ outputDir = process.env.RELEASE_GATE_OUTPUT_DIR } = {}) {
  const parent = path.resolve(outputDir || path.join(repoRoot, 'output/playwright'));
  await fs.mkdir(parent, { recursive: true });
  const dir = await fs.mkdtemp(path.join(parent, 'plugin-browser-'));
  const cases = [], blockers = [], artifacts = { report: path.join(dir, 'report.json'), exports: {} };
  let harness, daemon, server, desktop, browser, context, page, video, current, doc;
  const catalogRequests = [];
  try {
    const { chromium } = createRequire(path.join(uiRoot, 'package.json'))('playwright');
    browser = await chromium.launch({ channel: process.env.RELEASE_GATE_BROWSER_CHANNEL || undefined });
    context = await browser.newContext({ viewport: dimensions, recordVideo: { dir, size: dimensions }, reducedMotion: 'reduce', locale: 'pt-BR' });
    page = await context.newPage();
    video = page.video();
    page.setDefaultTimeout(20_000);
    harness = await createHarness('plugin-browser');
    await writeJson(harness.settingsPath, { agent: { workspace_root: harness.workspace } });
    daemon = startDaemon({ workspace: harness.workspace, settingsPath: harness.settingsPath, port: await pickPort(), name: 'browser-smoke' });
    await waitForHealth(daemon.baseUrl, daemon.proc);
    server = await startJsonServer(({ req }) => {
      const uri = new URL(req.url, 'http://127.0.0.1');
      if (req.method !== 'GET' || uri.pathname !== '/search.json' || uri.searchParams.get('q') !== searchTerm) return { status: 404 };
      catalogRequests.push(uri.pathname + uri.search);
      return { body: { docs: [book] } };
    }, 'open-library-fixture');
    desktop = await serveDesktop();
    await page.addInitScript(url => { window.__MLX_PILOT_DAEMON_URL__ = url; }, daemon.baseUrl);
    const ok = (method, route, body) => assertOkJson(daemon.baseUrl, method, route, body);
    const pluginList = () => ok('GET', '/agent/wasm-plugins');
    const openSection = async (section) => {
      await page.getByRole('button', { name: /mais seções/i }).click();
      await page.locator(`#more-sections [data-panel="${section}"]`).click();
    };
    const frame = page.frameLocator('#pm-frame iframe');
    const card = page.locator('[data-plugin-card="reading_notes"]');
    const open = card.locator('[data-open]');
    const grant = page.locator('input[name="grant"][value="documents:read"]');
    const saveConfiguration = async () => {
      const [response] = await Promise.all([
        page.waitForResponse(r => r.url() === daemon.baseUrl + pluginApi + '/configuration' && r.request().method() === 'PUT'),
        page.locator('#pm-save-configuration').click(),
      ]);
      assert.equal(response.status(), 200, await response.text());
      await page.locator('#pm-configuration-status').filter({ hasText: 'Configuração salva' }).waitFor();
      await page.locator('#plugin-manager[aria-busy]').waitFor({ state: 'hidden' });
    };
    const pass = (evidence = {}) => { cases.push({ id: current, status: 'pass', evidence }); current = null; };
    const getDoc = () => ok('GET', `/presentations/${doc.id}`);
    const invokeFromFrame = async (control, suffix) => {
      const [response] = await Promise.all([
        page.waitForResponse(r => r.url() === daemon.baseUrl + pluginApi + suffix && r.request().method() === 'POST'),
        frame.locator(control).press('Enter'),
      ]);
      assert.equal(response.status(), 200, await response.text());
      await frame.locator(control + ':enabled').waitFor();
    };
    const search = async () => {
      const count = catalogRequests.length;
      await frame.locator('#query').fill(searchTerm);
      await invokeFromFrame('#search', '/tools/search_books');
      assert.match(await frame.locator('#status').innerText(), /1 livros encontrados/);
      assert.equal(await frame.locator('#results h2').innerText(), book.title);
      assert.equal(catalogRequests.length, count + 1, 'Expected the guest to contact the loopback catalog');
    };
    current = 'install';
    await page.goto(desktop.baseUrl);
    await openSection('plugins');
    await page.locator('#plugin-manager[aria-busy]').waitFor({ state: 'hidden' });
    await page.locator('[data-plugin-card="presentation"]').waitFor();
    const fileInput = page.locator('#pm-file:enabled');
    await fileInput.waitFor({ state: 'attached' });
    const [chooser] = await Promise.all([page.waitForEvent('filechooser'), page.locator('label:has(#pm-file)').click()]);
    await chooser.setFiles(fixturePath);
    await page.locator('#pm-review').filter({ hasText: 'Reading Notes' }).waitFor();
    assert.equal(await page.locator('#pm-review input:checked').count(), 0);
    const requested = page.locator('#pm-review input[type="checkbox"]');
    assert.equal(await requested.count(), 3);
    for (const control of await requested.all()) await control.check();
    await page.locator('#pm-install').click();
    await card.waitFor();
    const installed = (await pluginList()).find(p => p.manifest.id === 'reading_notes');
    assert.equal(installed?.manifest.version, '1.1.0');
    assert.ok(installed.enabled && installed.has_frontend);
    pass({ plugin: installed.manifest.id, version: installed.manifest.version });

    current = 'configure';
    await card.locator('[data-configure]').click();
    await page.locator('[data-service-enabled]').check();
    await page.locator('[data-service-url]').fill(server.baseUrl);
    await page.locator('[data-service-private]').check();
    await page.locator('#pm-allow-create').check();
    await saveConfiguration();
    const config = await ok('GET', pluginApi + '/configuration');
    const service = config.services.open_library;
    assert.ok(service.enabled && service.allow_private_network && config.documents.allow_create);
    assert.equal(new URL(service.base_url).origin, server.baseUrl);
    await page.locator('#pm-close-configuration').click();
    pass({ catalog: service.base_url, workspace: 'temporary' });

    current = 'search';
    await open.click();
    await search();
    pass({ book: book.title, path: catalogRequests.at(-1) });

    current = 'document';
    await invokeFromFrame('#create', '/tools/create_reading_brief');
    assert.match(await frame.locator('#status').innerText(), /Caderno criado/);
    await invokeFromFrame('#projects', '/host');
    assert.match(await frame.locator('#status').innerText(), /1 projetos acessíveis/);
    assert.equal(await frame.locator('#results h2').innerText(), `Leituras: ${searchTerm}`);
    doc = (await ok('GET', '/presentations')).find(p => p.title === `Leituras: ${searchTerm}`);
    assert.ok(doc && doc.slides >= 3, 'Missing reading notebook');
    const original = await getDoc();
    assert.ok(original.slides.some(slide => slide.notes.includes(book.title)));
    // Reopening the desktop refreshes the editor's project inventory through the real API.
    await page.reload();
    await openSection('presentations');
    await page.locator('#ps-projects').selectOption(doc.id);
    await page.waitForFunction(title => document.querySelector('#ps-title')?.value === title, original.title);
    const studioTitle = 'Caderno de design editorial';
    const docUrl = daemon.baseUrl + '/presentations/' + doc.id;
    const studioSave = () => page.waitForResponse(r => r.url() === docUrl && r.request().method() === 'PUT');
    const [saved] = await Promise.all([studioSave(), page.locator('#ps-title').fill(studioTitle)]);
    assert.equal(saved.status(), 200, await saved.text());
    doc = await getDoc();
    assert.equal(doc.title, studioTitle);
    assert.ok(doc.revision > original.revision);
    pass({ document: doc.id, revision: doc.revision, slides: doc.slides.length });

    current = 'conflict';
    const winningTitle = 'Caderno revisado';
    const competing = await ok('POST', pluginApi + '/host', { operation: 'documents.edit', input: {
      id: doc.id, base_revision: doc.revision, operations: [{ type: 'set_title', text: winningTitle }],
    } });
    assert.ok(competing.revision > doc.revision);
    const [conflict] = await Promise.all([studioSave(), page.locator('#ps-title').fill('Rascunho com revisão antiga')]);
    assert.equal(conflict.status(), 409);
    await page.locator('#ps-status.error').filter({ hasText: /confli/i }).waitFor();
    assert.equal((await getDoc()).title, winningTitle);
    page.once('dialog', dialog => dialog.accept());
    await page.locator('#ps-reload').click();
    await page.waitForFunction(title => document.querySelector('#ps-title')?.value === title, winningTitle);
    doc = await getDoc();
    assert.equal(doc.revision, competing.revision);
    pass({ status: 409, winningRevision: doc.revision });

    current = 'exports';
    for (const format of ['pdf', 'pptx', 'html']) {
      await page.locator('#ps-format').selectOption(format);
      const [download] = await Promise.all([page.waitForEvent('download', { timeout: 90_000 }), page.locator('#ps-export').click()]);
      assert.ok(download.suggestedFilename().endsWith(`.${format}`));
      const filename = path.join(dir, `caderno.${format}`);
      await download.saveAs(filename);
      artifacts.exports[format] = await verifyExport(format, filename, doc);
    }
    pass({ formats: Object.keys(artifacts.exports) });

    current = 'revoke';
    await openSection('plugins');
    await page.locator('#plugin-manager[aria-busy]').waitFor({ state: 'hidden' });
    await open.click();
    await frame.locator('#query').waitFor();
    await card.locator('[data-configure]').click();
    await grant.uncheck();
    await saveConfiguration();
    await page.locator('#pm-frame iframe').waitFor({ state: 'detached' });
    assert.ok(await open.isDisabled());
    const denied = await requestJson(daemon.baseUrl, 'POST', pluginApi + '/host', { operation: 'documents.get', input: { id: doc.id } });
    assert.ok([400, 403].includes(denied.status), denied.text);
    assert.match(denied.text, /permission.*not granted/i);
    await grant.check();
    await saveConfiguration();
    await page.locator('#pm-close-configuration').click();
    assert.ok(await open.isEnabled());
    pass({ deniedStatus: denied.status });

    current = 'disable_enable';
    await card.locator('[data-toggle]').click();
    await card.locator('.pm-badge').filter({ hasText: 'Desativado' }).waitFor();
    assert.ok(await open.isDisabled());
    const off = (await pluginList()).find(p => p.manifest.id === 'reading_notes');
    assert.equal(off.enabled, false);
    await card.locator('[data-toggle]').click();
    await card.locator('.pm-badge').filter({ hasText: 'Ativo' }).waitFor();
    await open.click();
    await search();
    pass({ repeatSearch: book.title });

    current = 'remove';
    page.once('dialog', dialog => dialog.accept());
    await card.locator('[data-remove]').click();
    await card.waitFor({ state: 'detached' });
    assert.ok(!(await pluginList()).some(p => p.manifest.id === 'reading_notes'));
    assert.equal((await getDoc()).title, doc.title);
    pass({ preservedDocument: doc.id });
  } catch (error) {
    blockers.push(error.message);
    if (page && !page.isClosed()) {
      const failureFrame = path.join(dir, 'failure.png');
      try { await page.screenshot({ path: failureFrame, timeout: 5000 }); artifacts.failureFrame = failureFrame; } catch {}
    }
    if (current) cases.push({ id: current, status: 'fail', reason: error.message });
  } finally {
    const cleanup = async (name, action) => { try { await action(); } catch (error) { blockers.push(`${name}: ${error.message}`); } };
    await cleanup('browser context', async () => { await context?.close(); });
    await cleanup('video', async () => {
      assert.ok(video, 'Browser recording could not start');
      artifacts.video = await video.path();
      assert.ok((await fs.stat(artifacts.video)).size > 0, 'Empty recording');
    });
    await cleanup('browser', async () => { await browser?.close(); });
    await cleanup('desktop server', async () => { await stopDaemon(desktop); });
    await cleanup('daemon', async () => { await stopDaemon(daemon); });
    await cleanup('catalog server', async () => { await server?.close(); });
    if (daemon) await cleanup('daemon log', async () => {
      artifacts.daemonLog = path.join(dir, 'daemon.log');
      await fs.copyFile(daemon.logPath, artifacts.daemonLog);
    });
    if (harness) await cleanup('temporary workspace', async () => {
      assert.equal(path.dirname(await fs.realpath(harness.root)), await fs.realpath(os.tmpdir()));
      await fs.rm(harness.root, { recursive: true, force: true });
    });
  }
  for (const id of REQUIRED_PLUGIN_BROWSER_CASES) {
    if (!cases.some(entry => entry.id === id)) cases.push({ id, status: 'not_run', reason: 'A prerequisite did not complete' });
  }
  const result = { block: 'plugin_browser_smoke', title: 'Reading Notes and Studio in Chromium', executed: cases.some(entry => entry.status !== 'not_run'),
    status: blockers.length || cases.some(entry => entry.status !== 'pass') ? 'fail' : 'pass', cases, blockers, artifacts };
  await writeJson(artifacts.report, result);
  return result;
}

if (isDirectRun(import.meta.url)) runPluginBrowserSmoke().then(printStandaloneResult).catch(error => {
  process.stderr.write(`${error.stack || error}\n`);
  process.exitCode = 1;
});
