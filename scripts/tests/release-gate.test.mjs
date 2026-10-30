import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { computeDecision, REQUIRED_BLOCK_IDS, runAllReleaseGate } from "../release-gate/run-all.mjs";
import { REQUIRED_PLUGIN_SECURITY_CASES } from "../release-gate/plugin-security-smoke.mjs";
import { createHarness, daemonCommand, repoRoot, requestJson, startDaemon, stopDaemon, waitForHealth } from "../release-gate/_lib.mjs";

async function temporary(t) {
  const directory = await mkdtemp(path.join(tmpdir(), "mlx-gate-test-"));
  t.after(async () => {
    assert.equal(path.dirname(directory), path.resolve(tmpdir()));
    await rm(directory, { recursive: true, force: true });
  });
  return directory;
}

function passingBlocks() {
  return REQUIRED_BLOCK_IDS.map((block) => ({
    block, title: block, status: "pass", executed: true, findings: [], blockers: [],
    ...(block === "plugin_security_smoke" ? {
      cases: REQUIRED_PLUGIN_SECURITY_CASES.map((id) => ({ id, status: "pass" })),
    } : {}),
  }));
}

const pluginBlock = (blocks) => blocks.find((block) => block.block === "plugin_security_smoke");
const fixtureRunners = () => Object.fromEntries(passingBlocks().map((block) => [block.block, async () => block]));

test("GO requires every block and every registered plugin case to pass", () => {
  assert.ok(REQUIRED_PLUGIN_SECURITY_CASES.length > 0);
  assert.equal(computeDecision(passingBlocks()).status, "GO");
  assert.equal(computeDecision([]).status, "NO-GO");
});

for (const [name, mutate] of [
  ["missing audit block", (blocks) => blocks.splice(blocks.findIndex((b) => b.block === "security_audit_smoke"), 1)],
  ["failed audit without findings", (blocks) => { blocks.find((b) => b.block === "security_audit_smoke").status = "fail"; }],
  ["duplicate block", (blocks) => blocks.push({ ...blocks[0] })],
  ["unexpected block", (blocks) => blocks.push({ block: "unknown", status: "pass" })],
  ["zero plugin cases", (blocks) => { pluginBlock(blocks).cases = []; }],
  ["missing required plugin case", (blocks) => pluginBlock(blocks).cases.pop()],
  ["skipped plugin case", (blocks) => { pluginBlock(blocks).cases[0].status = "skip"; }],
  ["failed plugin case", (blocks) => { pluginBlock(blocks).cases[0].status = "fail"; }],
  ["case not executed despite pass", (blocks) => { pluginBlock(blocks).cases[0].executed = false; }],
  ["duplicate plugin case", (blocks) => pluginBlock(blocks).cases.push({ ...pluginBlock(blocks).cases[0] })],
  ["critical security finding", (blocks) => { blocks[0].findings = [{ severity: "critical", title: "fixture finding" }]; }],
]) {
  test(`NO-GO for ${name}`, () => {
    const blocks = passingBlocks();
    mutate(blocks);
    const decision = computeDecision(blocks);
    assert.equal(decision.status, "NO-GO");
    assert.ok(decision.blockers.length > 0);
  });
}

test("missing runners create a fresh explicit not_run report", async (t) => {
  const outputDir = await temporary(t);
  await writeFile(path.join(outputDir, "release-gate-report.json"), '{"decision":{"status":"GO"}}');
  const report = await runAllReleaseGate({ runners: {}, outputDir });
  assert.equal(report.decision.status, "NO-GO");
  assert.equal(report.repo, ".");
  assert.match(report.revision.commit, /^[a-f0-9]{40,64}$/);
  assert.equal(typeof report.revision.dirty, "boolean");
  assert.equal(report.environment.node, process.version);
  assert.ok(report.blocks.every((block) => block.status === "not_run" && !block.executed));
  assert.deepEqual(JSON.parse(await readFile(path.join(outputDir, "release-gate-report.json"), "utf8")), report);
  const markdown = await readFile(path.join(outputDir, "release-gate-report.md"), "utf8");
  assert.match(markdown, /Security audit did not pass or was not executed/);
  assert.match(markdown, /Channel validation did not pass or was not executed/);
  assert.match(markdown, /Load smoke did not pass or was not executed/);
  assert.match(markdown, /Plugin Security/);
  assert.match(markdown, /NOT_RUN/);
});

test("runner exception records failure and continues collecting later evidence", async (t) => {
  const outputDir = await temporary(t);
  const runners = fixtureRunners();
  runners.security_audit_smoke = async () => { throw new Error("fixture startup failure"); };
  let pluginRan = false;
  runners.plugin_security_smoke = async (options) => {
    pluginRan = true;
    assert.equal(options.outputDir, outputDir);
    return pluginBlock(passingBlocks());
  };
  const report = await runAllReleaseGate({ runners, outputDir });
  assert.equal(pluginRan, true);
  assert.equal(report.results.security_audit_smoke.status, "fail");
  assert.match(report.results.security_audit_smoke.error, /fixture startup failure/);
  assert.equal(report.results.plugin_security_smoke.status, "pass");
  assert.equal(report.decision.status, "NO-GO");
});

test("--only scope does not run other blocks or claim a full gate", async (t) => {
  const outputDir = await temporary(t);
  const calls = [];
  const runners = Object.fromEntries(passingBlocks().map((block) => [block.block, async () => {
    calls.push(block.block);
    return block;
  }]));
  const report = await runAllReleaseGate({ runners, only: "plugin_security_smoke", outputDir });
  assert.deepEqual(calls, ["plugin_security_smoke"]);
  assert.equal(report.scope, "partial");
  assert.equal(report.decision.status, "NO-GO");
  assert.equal(report.blocks.filter((block) => block.status === "not_run").length, REQUIRED_BLOCK_IDS.length - 1);
  assert.match(await readFile(path.join(outputDir, "release-gate-report.md"), "utf8"), /not a complete release gate/);
});

test("full report prepares dependencies and reproduces the gate without a partial overwrite", async (t) => {
  const outputDir = await temporary(t);
  const report = await runAllReleaseGate({ runners: fixtureRunners(), outputDir });
  const commands = report.reproduction_commands;
  const gateCommand = "node scripts/release-gate/run-all.mjs";
  const gateIndex = commands.indexOf(gateCommand);
  assert.equal(report.scope, "full");
  assert.ok(gateIndex >= 0);
  for (const preparation of [
    "cargo build --locked -p mlx-ollama-daemon",
    "npm ci --prefix apps/desktop-ui",
    "npm ci --prefix packages/plugin-sdk",
  ]) {
    assert.ok(commands.indexOf(preparation) >= 0 && commands.indexOf(preparation) < gateIndex, preparation);
  }
  assert.deepEqual(commands.filter((command) => command.includes("run-all.mjs")), [gateCommand]);
  assert.ok(commands.every((command) => !command.includes("--only") && !command.startsWith("cd ")));
  const markdown = await readFile(path.join(outputDir, "release-gate-report.md"), "utf8");
  assert.match(markdown, /cargo build --locked -p mlx-ollama-daemon/);
  assert.doesNotMatch(markdown, /--only/);
});

test("CLI fixtures exit zero for GO and nonzero for partial NO-GO, honoring output destinations", async (t) => {
  const directory = await temporary(t);
  const moduleUrl = new URL("../release-gate/run-all.mjs", import.meta.url).href;
  for (const partial of [false, true]) {
    const outputDir = path.join(directory, partial ? "partial" : "full");
    const args = partial ? ["--only", "plugin_security_smoke", "--output-dir", outputDir] : [];
    const code = `import {runReleaseGateCli} from ${JSON.stringify(moduleUrl)};
      const blocks = ${JSON.stringify(passingBlocks())};
      const runners = Object.fromEntries(blocks.map(b => [b.block, async () => b]));
      await runReleaseGateCli(${JSON.stringify(args)}, {runners});`;
    const result = spawnSync(process.execPath, ["--input-type=module", "--eval", code], {
      windowsHide: true, encoding: "utf8", timeout: 10000,
      env: { ...process.env, RELEASE_GATE_OUTPUT_DIR: partial ? path.join(directory, "unused") : outputDir },
    });
    assert.ifError(result.error);
    assert.equal(result.status, partial ? 1 : 0, result.stderr);
    const printed = JSON.parse(result.stdout);
    assert.equal(printed.decision.status, partial ? "NO-GO" : "GO");
    assert.deepEqual(JSON.parse(await readFile(path.join(outputDir, "release-gate-report.json"), "utf8")), printed);
  }
});

test("repository and binary override resolve independently of the caller directory", () => {
  assert.equal(repoRoot, path.resolve(fileURLToPath(new URL("../../", import.meta.url))));
  const previous = process.env.RELEASE_GATE_DAEMON_BIN;
  try {
    process.env.RELEASE_GATE_DAEMON_BIN = "relative/daemon-fixture.exe";
    assert.equal(daemonCommand().command, path.join(repoRoot, "relative", "daemon-fixture.exe"));
  } finally {
    if (previous === undefined) delete process.env.RELEASE_GATE_DAEMON_BIN;
    else process.env.RELEASE_GATE_DAEMON_BIN = previous;
  }
});

test("default daemon requires a compiled platform binary and never starts cargo", async (t) => {
  const root = await temporary(t);
  const previous = process.env.RELEASE_GATE_DAEMON_BIN;
  try {
    delete process.env.RELEASE_GATE_DAEMON_BIN;
    assert.throws(() => daemonCommand({ root }), /Compiled daemon not found.*cargo build/);
    const binary = path.join(root, "target", "debug", `mlx-ollama-daemon${process.platform === "win32" ? ".exe" : ""}`);
    await mkdir(path.dirname(binary), { recursive: true });
    await writeFile(binary, "binary selection fixture; not executed");
    assert.deepEqual(daemonCommand({ root }), { command: binary, args: [] });
  } finally {
    if (previous === undefined) delete process.env.RELEASE_GATE_DAEMON_BIN;
    else process.env.RELEASE_GATE_DAEMON_BIN = previous;
  }
});

test("HTTP helper omits JSON content type for bodyless GET and includes it for JSON POST", async (t) => {
  const server = http.createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    res.end(JSON.stringify({ contentType: req.headers["content-type"] ?? null, body: Buffer.concat(chunks).toString() }));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => new Promise((resolve) => server.close(resolve)));
  const base = `http://127.0.0.1:${server.address().port}`;
  assert.equal((await requestJson(base, "GET", "/")).payload.contentType, null);
  const post = await requestJson(base, "POST", "/", { fixture: true });
  assert.equal(post.payload.contentType, "application/json");
  assert.deepEqual(JSON.parse(post.payload.body), { fixture: true });
});

test("daemon harness isolates child settings and models and detects spawn/exit/signal failures", async (t) => {
  const harness = await createHarness("unit-fixture");
  t.after(async () => {
    assert.equal(path.dirname(harness.root), path.resolve(tmpdir()));
    await rm(harness.root, { recursive: true, force: true });
  });
  const fixture = path.join(harness.root, "child.cjs");
  const snapshot = path.join(harness.root, "environment.json");
  const inspectedEnvironment = ["APP_SETTINGS_PATH", "APP_AGENT_WORKSPACE", "APP_MODELS_DIR", "APP_REMOTE_DOWNLOADS_DIR",
    "APP_LOCAL_PROVIDER", "APP_OLLAMA_BASE_URL", "APP_LLAMACPP_BASE_URL", "APP_OLLAMA_AUTO_START", "APP_OLLAMA_AUTO_INSTALL",
    "APP_LLAMACPP_AUTO_START", "APP_LLAMACPP_AUTO_INSTALL"];
  await writeFile(fixture, `const keys = ${JSON.stringify(inspectedEnvironment)};
    const env = Object.fromEntries(keys.map(key => [key, process.env[key]]));
    require("node:fs").writeFileSync(process.env.FIXTURE_OUTPUT, JSON.stringify({env,cwd:process.cwd()}));`);
  const previous = process.env.RELEASE_GATE_DAEMON_BIN;
  let child;
  try {
    process.env.RELEASE_GATE_DAEMON_BIN = process.execPath;
    child = startDaemon({ ...harness, port: 1, env: {
      NODE_OPTIONS: `--require ${JSON.stringify(fixture)}`, FIXTURE_OUTPUT: snapshot,
      APP_MODELS_DIR: "must-not-use", APP_OLLAMA_AUTO_INSTALL: "true",
    } });
    await once(child.proc, "exit");
    const recorded = JSON.parse(await readFile(snapshot, "utf8"));
    assert.equal(recorded.cwd, repoRoot);
    assert.equal(recorded.env.APP_SETTINGS_PATH, harness.settingsPath);
    assert.equal(recorded.env.APP_AGENT_WORKSPACE, harness.workspace);
    assert.equal(recorded.env.APP_MODELS_DIR, path.join(harness.root, "models"));
    assert.equal(recorded.env.APP_REMOTE_DOWNLOADS_DIR, path.join(harness.root, "downloads"));
    assert.equal(recorded.env.APP_LOCAL_PROVIDER, "ollama");
    assert.equal(recorded.env.APP_OLLAMA_BASE_URL, "http://127.0.0.1:1");
    assert.equal(recorded.env.APP_LLAMACPP_BASE_URL, "http://127.0.0.1:1");
    for (const key of ["APP_OLLAMA_AUTO_START", "APP_OLLAMA_AUTO_INSTALL", "APP_LLAMACPP_AUTO_START", "APP_LLAMACPP_AUTO_INSTALL"]) {
      assert.equal(recorded.env[key], "false");
    }
    await assert.rejects(waitForHealth(child.baseUrl, child.proc), /exited early/);

    await writeFile(fixture, "setInterval(() => {}, 1000);");
    child = startDaemon({ ...harness, port: 1, env: { NODE_OPTIONS: `--require ${JSON.stringify(fixture)}` } });
    await stopDaemon(child);
    await assert.rejects(waitForHealth(child.baseUrl, child.proc), /exited early/);

    process.env.RELEASE_GATE_DAEMON_BIN = path.join(harness.root, "missing-daemon");
    child = startDaemon({ ...harness, port: 1 });
    await assert.rejects(waitForHealth(child.baseUrl, child.proc, "missing fixture", 3), /ENOENT/);
  } finally {
    await stopDaemon(child);
    if (previous === undefined) delete process.env.RELEASE_GATE_DAEMON_BIN;
    else process.env.RELEASE_GATE_DAEMON_BIN = previous;
  }
});
