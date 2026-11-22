#!/usr/bin/env node
// With --qr this opens a fresh WhatsApp pairing request, checks it, then logs out.
import assert from "node:assert/strict";
import { readFile, readdir, rm, stat } from "node:fs/promises";
import path from "node:path";
import {
  assertOkJson, createHarness, pickPort, repoRoot, startDaemon, stopDaemon, waitForHealth,
} from "./_lib.mjs";

const sandbox = await createHarness("whatsapp-helper");
const account = { channel: "whatsapp", account_id: "unpaired-smoke" };
let daemon;
let loginStarted = false;
const post = (operation, body = account) => assertOkJson(
  daemon.baseUrl, "POST", `/agent/channels/${operation}`, body, { timeoutMs: 180_000 },
);
try {
  daemon = startDaemon({ ...sandbox, port: await pickPort() });
  await waitForHealth(daemon.baseUrl, daemon.proc);
  await post("upsert-account", { ...account, enabled: true, limits: { timeout_ms: 150_000, max_retries: 0 } });
  const [before] = await post("probe");
  assert.equal(before.status, "not_logged_in");

  const cache = path.join(sandbox.root, "runtime-cache", "whatsapp-bridge");
  const versions = await readdir(cache);
  assert.equal(versions.length, 1);
  const runtime = path.join(cache, versions[0]);
  for (const name of ["bridge.mjs", "package.json", "package-lock.json"]) {
    const source = path.join(repoRoot, "crates", "daemon", "whatsapp-bridge", name);
    assert.ok((await readFile(path.join(runtime, name))).equals(await readFile(source)), `Stale ${name}`);
  }
  const installedAt = (await stat(path.join(runtime, ".installed"))).mtimeMs;
  assert.equal((await post("probe"))[0].status, "not_logged_in");
  assert.equal((await stat(path.join(runtime, ".installed"))).mtimeMs, installedAt);
  console.log("PASS: clean install from embedded files, native dependencies loaded, cache reused");

  if (process.argv.includes("--qr")) {
    loginStarted = true;
    const login = await post("login");
    assert.equal(login.status, "pending_qr");
    assert.equal(login.details.backend, "embedded");
    assert.ok(typeof login.details.qr_code === "string" && login.details.qr_code.trim().length > 0);
    assert.equal((await post("probe"))[0].status, "pending_qr");
    console.log("PASS: real WhatsApp QR issued and pending; physical pairing requires a phone");
    assert.equal((await post("logout")).status, "logged_out");
    loginStarted = false;
    assert.equal((await post("probe"))[0].status, "not_logged_in");
    console.log("PASS: logged out and confirmed unpaired");
  }
} finally {
  try {
    if (loginStarted) await post("logout");
  } finally {
    await stopDaemon(daemon);
    await rm(sandbox.root, { recursive: true, force: true });
  }
}
