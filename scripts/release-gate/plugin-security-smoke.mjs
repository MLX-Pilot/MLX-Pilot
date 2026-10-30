#!/usr/bin/env node
import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import {
  buildLegacySettings, createHarness, daemonCommand, isDirectRun, listFilesRecursive, pickPort,
  repoRoot, requestJson, startDaemon, startJsonServer, stopDaemon, waitForHealth, writeJson,
} from "./_lib.mjs";
import { BRIDGE_SECURITY_CASES, runPluginBridgeSecurity } from "./plugin-bridge-security.mjs";

const BROKER_CASES = [
  "plugin_document_isolation", "plugin_workspace_isolation", "plugin_grant_revocation",
  "plugin_credentials_redaction", "plugin_network_authorization", "plugin_revision_conflict",
  "plugin_wasm_limit", "plugin_request_quota", "plugin_response_limit",
];
export const REQUIRED_PLUGIN_SECURITY_CASES = Object.freeze([...BROKER_CASES, ...BRIDGE_SECURITY_CASES]);
const permissions = ["compute", "documents:read", "documents:write", "network:request", "credentials:use"];
const pluginRoute = (id, suffix = "") => `/agent/wasm-plugins/${id}${suffix}`;

function manifest(id) {
  const tool = (name, exported, mutating) => ({ name, export: exported, mutating,
    description: "Release-gate adversarial fixture", parameters: { type: "object" } });
  return {
    api_version: 1, id, name: `Release gate ${id}`, version: "1.0.0", permissions,
    tools: [tool("broker", "broker", true), tool("reader", "broker", false),
      tool("quota", "quota", false), tool("spin", "spin", false)],
    services: [{ id: "api", name: "Isolated local fixture", base_url: "http://127.0.0.1:1",
      methods: ["GET", "POST"], credential: { id: "token", header: "Authorization", prefix: "Bearer " } }],
  };
}

async function generateFixture(outputDir) {
  const fixturePath = path.join(outputDir, "plugin-security-fixture.json");
  const args = ["run", "--locked", "--quiet", "-p", "mlx-plugin-runtime", "--example", "plugin_security_fixture", "--", fixturePath];
  const result = await new Promise((resolve) => {
    const child = spawn("cargo", args, { cwd: repoRoot, stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true, detached: process.platform !== "win32" });
    let stdout = "", stderr = "", failure = null, finished = false, forceTimer, finishTimer;
    const finish = (exitCode = null, signal = null) => {
      if (finished) return;
      finished = true;
      clearTimeout(timer); clearTimeout(forceTimer); clearTimeout(finishTimer);
      child.stdout.destroy(); child.stderr.destroy(); child.unref();
      resolve({ exitCode, signal, stdout, stderr, failure });
    };
    const killGroup = (signal) => {
      if (!child.pid) return;
      if (process.platform === "win32") {
        const killer = spawn("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
        killer.on("error", () => child.kill("SIGKILL")); killer.unref();
      } else {
        try { process.kill(-child.pid, signal); } catch { child.kill(signal); }
      }
    };
    const abort = (reason) => {
      if (failure || finished) return;
      failure = reason;
      killGroup("SIGTERM");
      forceTimer = setTimeout(() => killGroup("SIGKILL"), 250);
      // A descendant retaining a pipe must not prevent an evidence report.
      finishTimer = setTimeout(() => finish(), 1000);
    };
    const collect = (stream, chunk) => {
      const remaining = 1024 * 1024 - stdout.length - stderr.length;
      const text = chunk.toString();
      if (stream === "stdout") stdout += text.slice(0, remaining);
      else stderr += text.slice(0, remaining);
      if (text.length > remaining) abort("Fixture compilation exceeded the 1 MiB output limit");
    };
    child.stdout.on("data", (chunk) => collect("stdout", chunk));
    child.stderr.on("data", (chunk) => collect("stderr", chunk));
    const timer = setTimeout(() => abort("Fixture compilation exceeded 600 seconds"), 600_000);
    child.once("error", (error) => { failure = error.message; finish(); });
    child.once("close", finish);
  });
  await fs.writeFile(path.join(outputDir, "fixture-build.log"), result.stdout + result.stderr);
  assert.equal(result.failure, null, result.failure || "fixture compilation failed");
  assert.equal(result.exitCode, 0, `WAT fixture compilation failed: ${result.stderr}`);
  const { wasm } = JSON.parse(await fs.readFile(fixturePath, "utf8"));
  assert.ok(Array.isArray(wasm) && wasm.length > 8, "fixture generator returned no WASM");
  const bytes = Buffer.from(wasm);
  return { wasm_base64: bytes.toString("base64"), sha256: createHash("sha256").update(bytes).digest("hex"),
    command: ["cargo", ...args], path: fixturePath };
}

// Every recorded body is sanitized, including failures: a detected leak must not
// copy the credential into the report used to demonstrate it.
function redact(value, secret) {
  return JSON.parse(JSON.stringify(value).replaceAll(secret, "[REDACTED]"));
}

export async function runPluginSecuritySmoke({ outputDir = process.env.RELEASE_GATE_OUTPUT_DIR } = {}) {
  const harness = await createHarness("plugin-security");
  outputDir = path.resolve(outputDir || path.join(harness.root, "evidence"));
  await fs.mkdir(outputDir, { recursive: true });
  const secret = `release-plugin-${randomUUID()}`;
  const cases = [], findings = [], observations = { execution: "Real daemon HTTP and Extism WASM; deterministic loopback service", output_dir: outputDir };
  const traces = [], logs = [];
  let daemon, server, fixture;
  const alpha = "release_alpha", beta = "release_beta";
  let documentA, documentB, settingsA, settingsB;
  const clean = (value) => redact(value, secret);
  const capture = async (id, run) => {
    const offset = traces.length;
    try {
      const details = await run();
      cases.push({ id, status: "pass", evidence: clean({ ...details, requests: traces.slice(offset) }) });
    } catch (error) {
      cases.push({ id, status: "fail", evidence: clean({ failure_kind: error.code === "ERR_ASSERTION" ? "assertion" : "execution", error: error.message, requests: traces.slice(offset) }) });
    }
  };
  async function request(method, route, body, options) {
    const response = await requestJson(daemon.baseUrl, method, route, body, options);
    traces.push(clean({ method, route, status: response.status, operation: body?.operation,
      response: response.payload }));
    return response;
  }
  async function ok(method, route, body, options) {
    const response = await request(method, route, body, options);
    assert.equal(response.status, 200, `${method} ${route}: ${response.text}`);
    assert.ok(!response.payload?.error, `${route}: ${response.text}`);
    return response.payload;
  }
  async function host(id, operation, input, tool = null) {
    return request("POST", pluginRoute(id, tool ? `/tools/${tool}` : "/host"), { operation, input });
  }
  async function allowed(id, operation, input, tool = null) {
    const response = await host(id, operation, input, tool);
    assert.equal(response.status, 200, response.text);
    assert.ok(!response.payload?.error, response.text);
    return tool ? response.payload.result : response.payload;
  }
  async function denied(id, operation, input, pattern, tool = null) {
    const response = await host(id, operation, input, tool);
    assert.ok(tool ? [200, 400, 403, 409].includes(response.status) : [400, 403, 409].includes(response.status), response.text);
    assert.equal(typeof response.payload?.error, "string", response.text);
    assert.match(response.payload.error, pattern);
    return response;
  }
  async function configure(id, settings) { return ok("PUT", pluginRoute(id, "/configuration"), settings); }
  async function launch(workspace, label) {
    daemon = startDaemon({ workspace, settingsPath: harness.settingsPath, port: await pickPort(), name: label,
      env: { APP_OLLAMA_AUTO_INSTALL: "false", APP_OLLAMA_AUTO_START: "false", APP_LLAMACPP_AUTO_INSTALL: "false", APP_LLAMACPP_AUTO_START: "false" } });
    logs.push(daemon.logPath);
    await waitForHealth(daemon.baseUrl, daemon.proc, label);
  }

  try {
    const executable = daemonCommand();
    const gitOptions = { cwd: repoRoot, encoding: "utf8", windowsHide: true, timeout: 5000 };
    observations.runtime = { git_revision: execFileSync("git", ["rev-parse", "HEAD"], gitOptions).trim(),
      git_dirty: Boolean(execFileSync("git", ["status", "--porcelain"], gitOptions).trim()), started_at: new Date().toISOString(),
      platform: process.platform, architecture: process.arch, node: process.version,
      daemon_command: executable.command, daemon_arguments: executable.args,
      daemon_sha256: executable.args.length === 0 ? createHash("sha256").update(await fs.readFile(executable.command)).digest("hex") : null };
    fixture = await generateFixture(outputDir);
    observations.fixture = { sha256: fixture.sha256, command: fixture.command, path: fixture.path };
    await writeJson(harness.settingsPath, buildLegacySettings(harness.workspace));
    server = await startJsonServer(({ req }) => {
      if (req.url === "/oversize") return { body: "x".repeat(1024 * 1024 + 1), headers: { "content-type": "text/plain" } };
      if (req.url === "/error") return { status: 500, body: { error: `echo ${secret}` } };
      return { headers: { "content-type": `application/json; echoed=${secret}` },
        body: { echo: `Bearer ${secret}`, nested: { [secret]: [secret] }, value: 42 } };
    }, "plugin-security-service");
    await launch(harness.workspace, "plugin-security-workspace-a");
    for (const id of [alpha, beta]) await ok("POST", "/agent/wasm-plugins", { manifest: manifest(id), wasm_base64: fixture.wasm_base64, grants: permissions });
    documentA = await ok("POST", "/presentations", { title: "Alpha authorized" });
    documentB = await ok("POST", "/presentations", { title: "Beta authorized" });
    const settings = (document) => ({ grants: permissions, documents: { ids: [document.id], allow_create: false },
      services: { api: { enabled: false, base_url: server.baseUrl, allow_private_network: false } } });
    settingsA = settings(documentA); settingsB = settings(documentB);
    await configure(alpha, settingsA); await configure(beta, settingsB);

    await capture("plugin_document_isolation", async () => {
      assert.equal((await allowed(alpha, "documents.get", { id: documentA.id }, "broker")).id, documentA.id);
      assert.equal((await allowed(beta, "documents.get", { id: documentB.id }, "broker")).id, documentB.id);
      await denied(alpha, "documents.get", { id: documentB.id }, /not authorized/i, "broker");
      await denied(beta, "documents.get", { id: documentA.id }, /not authorized/i);
      assert.deepEqual((await allowed(alpha, "documents.list", {})).map((doc) => doc.id), [documentA.id]);
      const forged = await request("POST", pluginRoute(beta, "/tools/broker"), { operation: "documents.get", input: { id: documentA.id }, plugin_id: alpha });
      assert.equal(forged.status, 200); assert.match(forged.payload.error, /Invalid plugin host request/);
      await denied(alpha, "documents.create", { title: "Forbidden" }, /not authorized/i);
      await denied(alpha, "documents.create", { title: "Reader write" }, /mutating tool/i, "reader");
      return { plugins: [alpha, beta], documents: [documentA.id, documentB.id], positive_controls: 2, denied_cross_plugin_and_document_access: true };
    });

    await capture("plugin_grant_revocation", async () => {
      assert.equal((await allowed(alpha, "documents.get", { id: documentA.id })).id, documentA.id);
      try {
        await configure(alpha, { ...settingsA, grants: ["compute"] });
        await denied(alpha, "documents.get", { id: documentA.id }, /permission.*not granted/i, "broker");
        await denied(alpha, "documents.list", {}, /permission.*not granted/i);
      } finally { await configure(alpha, settingsA); }
      try {
        await ok("PATCH", pluginRoute(alpha), { enabled: false });
        await denied(alpha, "documents.list", {}, /disabled/i, "broker");
      } finally { await ok("PATCH", pluginRoute(alpha), { enabled: true }); }
      assert.equal((await allowed(alpha, "documents.get", { id: documentA.id }, "broker")).id, documentA.id);
      return { revoked_grants_and_disabled_plugin_denied: true, restored_positive_control: true };
    });

    await capture("plugin_revision_conflict", async () => {
      const original = await allowed(alpha, "documents.get", { id: documentA.id });
      const input = { id: documentA.id, base_revision: original.revision,
        operations: [{ type: "set_text", slide_id: "slide-1", element_id: "title-1", text: "Authorized revision" }] };
      const changed = await allowed(alpha, "documents.edit", input, "broker");
      assert.equal(changed.revision, original.revision + 1);
      const conflict = await denied(alpha, "documents.edit", input, /^REVISION_CONFLICT/);
      assert.equal(conflict.status, 409);
      const current = await allowed(alpha, "documents.get", { id: documentA.id });
      assert.equal(current.revision, changed.revision);
      assert.equal(current.slides[0].elements[0].text, "Authorized revision");
      return { original_revision: original.revision, final_revision: current.revision, rejected_status: conflict.status, rejected_write_preserved_document: true };
    });

    await capture("plugin_network_authorization", async () => {
      const before = server.requests.length;
      await denied(alpha, "http.request", { service: "api", path: "/data" }, /enable this service/i, "broker");
      const privateWithoutConsent = await request("PUT", pluginRoute(alpha, "/configuration"), { ...settingsA,
        services: { api: { enabled: true, base_url: server.baseUrl, allow_private_network: false } } });
      assert.equal(privateWithoutConsent.status, 400);
      assert.match(privateWithoutConsent.payload.error, /HTTPS/);
      const metadataSettings = { ...settingsA, services: { api: { enabled: true, base_url: "https://169.254.169.254", allow_private_network: true } } };
      try {
        await configure(alpha, metadataSettings);
        await denied(alpha, "http.request", { service: "api", path: "/" }, /prohibited address/i, "broker");
      } finally { await configure(alpha, settingsA); }
      settingsA.services.api = { enabled: true, base_url: server.baseUrl, allow_private_network: true, credential: secret };
      await configure(alpha, settingsA);
      delete settingsA.services.api.credential;
      for (const [input, pattern] of [
        [{ service: "api", path: "//127.0.0.1/escape" }, /relative service path/i],
        [{ service: "api", path: "https://example.invalid/escape" }, /relative service path/i],
        [{ service: "api", path: "/", method: "DELETE" }, /method not declared/i],
        [{ service: "unknown", path: "/" }, /unknown.*service/i],
        [{ service: "api", path: "/", headers: { Host: "example.invalid" } }, /reserved request header/i],
      ]) await denied(alpha, "http.request", input, pattern, "broker");
      await denied(alpha, "http.request", { service: "api", path: "/", method: "POST" }, /mutating tool/i, "reader");
      assert.equal(server.requests.length, before, "denied requests contacted the service");
      const response = await allowed(alpha, "http.request", { service: "api", path: "/data" }, "broker");
      assert.equal(response.body.value, 42);
      assert.equal(server.requests.length, before + 1);
      assert.equal(server.requests.at(-1).headers.authorization, `Bearer ${secret}`);
      return { denied_requests_reached_service: 0, authorized_requests: 1, credential_injected_by_host: true, metadata_target_blocked: true };
    });

    await capture("plugin_credentials_redaction", async () => {
      // This case can run independently if a previous network assertion failed.
      settingsA.services.api = { enabled: true, base_url: server.baseUrl, allow_private_network: true };
      const config = await configure(alpha, { ...settingsA, services: { api: { ...settingsA.services.api, credential: secret } } });
      assert.equal(config.services.api.credential_set, true);
      assert.ok(!JSON.stringify(config).includes(secret));
      assert.deepEqual(await allowed(alpha, "credentials.status", { service: "api" }, "broker"), { configured: true });
      assert.deepEqual(await allowed(beta, "credentials.status", { service: "api" }, "broker"), { configured: false });
      await denied(alpha, "credentials.read", { service: "api" }, /unsupported.*operation/i, "broker");
      for (const url of ["/data", "/error"]) {
        const response = await host(alpha, "http.request", { service: "api", path: url }, "broker");
        assert.equal(response.status, 200); assert.ok(!response.payload.error, response.text);
        assert.ok(!response.text.includes(secret), "credential leaked in service result");
        assert.ok(response.text.includes("[REDACTED]"), "echoed credential was not redacted");
      }
      await denied(alpha, "http.request", { service: "api", path: "/data", response_type: "base64" }, /protected credentials/i, "broker");
      const invalid = await request("PUT", pluginRoute(alpha, "/configuration"), { ...settingsA,
        services: { api: { ...settingsA.services.api, credential: `${secret}\n` } } });
      assert.equal(invalid.status, 400); assert.ok(!invalid.text.includes(secret));
      const scanned = await listFilesRecursive(harness.root);
      const leaks = [];
      for (const filename of scanned) if ((await fs.readFile(filename)).includes(Buffer.from(secret))) leaks.push(filename);
      assert.deepEqual(leaks, [], "credential leaked to settings, vault ciphertext or logs");
      return { write_only_configuration: true, separate_plugin_credential: true, success_and_error_redacted: true,
        protected_binary_response_denied: true, scanned_file_count: scanned.length, leaked_files: leaks };
    });

    await capture("plugin_workspace_isolation", async () => {
      const workspaceB = path.join(harness.root, "workspace-b"); await fs.mkdir(workspaceB);
      await stopDaemon(daemon); daemon = null;
      try {
        await launch(workspaceB, "plugin-security-workspace-b");
        const config = await ok("GET", pluginRoute(alpha, "/configuration"));
        assert.deepEqual(config.documents.ids, []);
        assert.equal(config.services.api.credential_set, false);
        assert.deepEqual(await allowed(alpha, "documents.list", {}, "broker"), []);
        await denied(alpha, "documents.get", { id: documentA.id }, /not authorized/i, "broker");
        await denied(alpha, "documents.create", { title: "Unauthorized workspace" }, /not authorized/i);
      } finally {
        await stopDaemon(daemon); daemon = null;
        await launch(harness.workspace, "plugin-security-workspace-a-restored");
      }
      assert.equal((await allowed(alpha, "documents.get", { id: documentA.id }, "broker")).id, documentA.id);
      assert.deepEqual(await allowed(alpha, "credentials.status", { service: "api" }), { configured: true });
      return { same_plugin_store: true, distinct_workspace: true, grants_and_credentials_not_inherited: true, original_workspace_restored: true };
    });

    await capture("plugin_request_quota", async () => {
      const before = server.requests.length;
      await denied(alpha, "http.request", { service: "api", path: "/quota" }, /at most three service requests/i, "quota");
      assert.equal(server.requests.length - before, 3);
      return { wasm_attempts: 4, service_requests: 3, fourth_request_denied: true };
    });
    await capture("plugin_response_limit", async () => {
      const before = server.requests.length;
      await denied(alpha, "http.request", { service: "api", path: "/oversize", response_type: "text" }, /response exceeds 1 MiB/i, "broker");
      assert.equal(server.requests.length - before, 1);
      return { served_body_bytes: 1024 * 1024 + 1, runtime_response_limit_bytes: 1024 * 1024, rejected: true };
    });
    await capture("plugin_wasm_limit", async () => {
      const began = performance.now();
      const response = await request("POST", pluginRoute(alpha, "/tools/spin"), {}, { timeoutMs: 15_000 });
      assert.equal(response.status, 400, response.text);
      assert.match(response.payload.error, /fuel|interrupt|timeout|trap|wasm backtrace/i);
      assert.ok(performance.now() - began < 15_000);
      assert.equal((await request("GET", "/health")).status, 200);
      assert.equal((await allowed(alpha, "documents.get", { id: documentA.id }, "broker")).id, documentA.id);
      return { elapsed_ms: Math.round(performance.now() - began), infinite_guest_rejected: true, daemon_and_next_guest_healthy: true };
    });
  } catch (error) {
    observations.setup_error = clean({ message: error.message });
  } finally {
    try { await stopDaemon(daemon); } catch (error) { observations.shutdown_error = clean({ message: error.message }); }
    try { await server?.close(); } catch (error) { observations.server_shutdown_error = clean({ message: error.message }); }
  }

  // Inspect raw logs/state from every scenario before redaction or cleanup, so a
  // late leak cannot be hidden by the sanitized artifact copies below.
  try {
    const scanned = await listFilesRecursive(harness.root);
    const leaks = [];
    for (const filename of scanned) if ((await fs.readFile(filename)).includes(Buffer.from(secret))) leaks.push(filename);
    observations.final_credential_scan = { scanned_file_count: scanned.length, leaked_files: leaks };
    const credentialCase = cases.find(({ id }) => id === "plugin_credentials_redaction");
    if (leaks.length) {
      const evidence = { failure_kind: "assertion", error: "Credential leaked to raw state or daemon logs after final shutdown", leaked_files: leaks };
      if (credentialCase) { credentialCase.status = "fail"; Object.assign(credentialCase.evidence, evidence); }
      else cases.push({ id: "plugin_credentials_redaction", status: "fail", evidence });
    }
  } catch (error) {
    observations.final_credential_scan_error = clean({ message: error.message });
    const credentialCase = cases.find(({ id }) => id === "plugin_credentials_redaction");
    if (credentialCase) { credentialCase.status = "fail"; Object.assign(credentialCase.evidence, { failure_kind: "execution", error: "Final credential scan did not complete" }); }
  }

  // DOM/SDK contracts still run if compilation, daemon startup or HTTP setup failed.
  try {
    const bridge = await runPluginBridgeSecurity({ outputDir });
    cases.push(...bridge.cases);
    observations.bridge = { status: bridge.status, execution: bridge.execution };
  } catch (error) { observations.bridge_error = clean({ message: error.message }); }
  for (const id of REQUIRED_PLUGIN_SECURITY_CASES) {
    if (!cases.some((entry) => entry.id === id)) cases.push({ id, status: "fail", evidence: { execution: "not_run", reason: "Required scenario did not finish; see setup/bridge error" } });
  }
  const failedCases = cases.filter((entry) => entry.status !== "pass");
  for (const entry of failedCases.filter((entry) => entry.evidence.failure_kind === "assertion")) findings.push({ severity: "review", area: "plugin_security", title: `Plugin security assertion failed: ${entry.id}`, evidence: entry.evidence, action: "Inspect the observed response and reproduce the assertion; this is not automatically a confirmed vulnerability." });
  const blockers = failedCases.map((entry) => `Required plugin case did not pass: ${entry.id}`);
  if (observations.shutdown_error || observations.server_shutdown_error) blockers.push("Plugin gate cleanup did not complete");
  const sanitizedLogs = [];
  for (const [index, logfile] of logs.entries()) {
    try {
      const destination = path.join(outputDir, `daemon-${index + 1}.log`);
      await fs.writeFile(destination, (await fs.readFile(logfile, "utf8")).replaceAll(secret, "[REDACTED]"));
      sanitizedLogs.push(destination);
    } catch (error) { blockers.push(`Could not preserve daemon log ${index + 1}: ${error.code || error.message}`); }
  }
  observations.daemon_logs = sanitizedLogs;
  observations.service_requests = server?.requests.map(({ method, url, headers }) => ({ method, url, credential_received: headers.authorization === `Bearer ${secret}` })) || [];
  // Remove only the fresh harness created above, never a user-provided output directory.
  const outputRelativeToState = path.relative(harness.root, outputDir);
  if (outputRelativeToState.startsWith(`..${path.sep}`) || path.isAbsolute(outputRelativeToState)) {
    try {
      const resolvedState = await fs.realpath(harness.root);
      const resolvedTemp = await fs.realpath(os.tmpdir());
      assert.equal(path.dirname(resolvedState), resolvedTemp, "refusing cleanup outside temporary directory");
      assert.ok(path.basename(resolvedState).startsWith("mlx-pilot-plugin-security-"), "unexpected harness name");
      await fs.rm(resolvedState, { recursive: true, force: true });
      observations.temporary_state_removed = true;
    } catch (error) { blockers.push(`Temporary plugin state cleanup failed: ${error.code || error.message}`); }
  }
  const result = clean({ block: "plugin_security_smoke", title: "Plugin broker and bridge security", status: blockers.length ? "fail" : "pass", blockers, findings, cases, observations,
    commands: ["node scripts/release-gate/plugin-security-smoke.mjs"], artifacts: { report: path.join(outputDir, "plugin-security-report.json"), daemon_logs: sanitizedLogs } });
  await writeJson(result.artifacts.report, result);
  return result;
}

if (isDirectRun(import.meta.url)) {
  runPluginSecuritySmoke().then((result) => {
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (result.status !== "pass") process.exitCode = 1;
  }).catch((error) => { process.stderr.write(`${error.stack || error}\n`); process.exitCode = 1; });
}
