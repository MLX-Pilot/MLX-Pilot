#!/usr/bin/env node
// Run after building the daemon and packing plugins/reading-notes.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, rm } from "node:fs/promises";
import path from "node:path";
import {
  assertOkJson, createHarness, pickPort, readJson, repoRoot, startDaemon,
  stopDaemon, waitForHealth, writeJson,
} from "./_lib.mjs";

const origin = "https://openlibrary.org/";
const query = "Dom Casmurro";
const pluginDir = path.join(repoRoot, "plugins", "reading-notes");
const output = path.join(repoRoot, "output", "playwright", "live-catalog", "report.json");
const report = { started_at: new Date().toISOString(), service: origin, tool: "search_books", query, status: "fail" };
let harness;
let daemon;
try {
  report.stage = "load package";
  const { version } = await readJson(path.join(pluginDir, "package.json"));
  const contents = await readFile(path.join(pluginDir, `reading_notes-${version}.mlxplugin.json`));
  const pkg = JSON.parse(contents);
  assert.equal(pkg.manifest.id, "reading_notes");
  assert.equal(pkg.manifest.version, version);
  assert.equal(pkg.manifest.services.length, 1);
  const [service] = pkg.manifest.services;
  assert.equal(service.id, "open_library");
  assert.equal(service.base_url, origin);
  assert.deepEqual(service.methods, ["GET"]);
  report.package_sha256 = createHash("sha256").update(contents).digest("hex");

  report.stage = "start daemon";
  harness = await createHarness("reading-notes-live");
  daemon = startDaemon({ ...harness, port: await pickPort() });
  await waitForHealth(daemon.baseUrl, daemon.proc);
  const invoke = (method, route, body, timeoutMs = 60_000) =>
    assertOkJson(daemon.baseUrl, method, route, body, { timeoutMs });

  report.stage = "install and configure";
  const plugins = "/agent/wasm-plugins";
  const route = `${plugins}/reading_notes`;
  const grants = pkg.manifest.permissions;
  await invoke("POST", plugins, { ...pkg, grants }, 180_000);
  const configured = await invoke("PUT", `${route}/configuration`, {
    grants,
    services: { open_library: { enabled: true, base_url: origin, allow_private_network: false } },
    documents: { ids: [], allow_create: false },
  });
  assert.equal(configured.services.open_library.base_url, origin);
  assert.equal(configured.services.open_library.enabled, true);
  assert.equal(configured.services.open_library.allow_private_network, false);
  await invoke("PATCH", route, { enabled: true });

  report.stage = "search Open Library";
  const { books } = await invoke("POST", `${route}/tools/search_books`, { query });
  assert.ok(Array.isArray(books) && books.length >= 1 && books.length <= 5, "Expected 1–5 catalog results");
  for (const book of books) {
    assert.ok(typeof book.title === "string" && book.title.trim(), "Missing book title");
    assert.match(book.source, /^https:\/\/openlibrary\.org\/works\/OL\d+W$/);
  }
  assert.ok(books.some((book) => /dom casmurro/i.test(book.title)), "No title matched the live query");
  report.books = books;
  report.status = "pass";
} catch (error) {
  report.error = error.message;
} finally {
  try {
    await stopDaemon(daemon);
    if (harness) await rm(harness.root, { recursive: true, force: true });
  } catch (error) {
    report.status = "fail";
    report.cleanup_error = error.message;
  }
  report.finished_at = new Date().toISOString();
  await writeJson(output, report);
  console.log(`${report.status.toUpperCase()} — ${output}`);
}
if (report.status !== "pass") {
  console.error([report.error, report.cleanup_error].filter(Boolean).join("\n"));
  process.exitCode = 1;
}
