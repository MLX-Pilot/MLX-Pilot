// Run against an isolated real daemon; no model, npm install or external service required.
// Usage: node scripts/smoke-native-workflows.mjs <path-to-daemon>
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const root = fileURLToPath(new URL("../", import.meta.url));
const binary = process.argv[2];
assert.ok(binary, "Pass the compiled daemon path as the first argument.");
const temporary = await mkdtemp(path.join(os.tmpdir(), "mlx-flow-smoke-"));
const workspace = path.join(temporary, "workspace");
await mkdir(workspace);
const portProbe = net.createServer();
portProbe.listen(Number(process.env.FLOW_SMOKE_PORT || 0), "127.0.0.1");
await once(portProbe, "listening");
const port = portProbe.address().port;
await new Promise((resolve) => portProbe.close(resolve));
const base = `http://127.0.0.1:${port}`;
const daemon = spawn(path.resolve(binary), [], {
  cwd: root,
  windowsHide: true,
  env: {
    ...process.env,
    APP_BIND_ADDR: `127.0.0.1:${port}`,
    APP_SETTINGS_PATH: path.join(temporary, "settings.json"),
    APP_AGENT_WORKSPACE: workspace,
    APP_MODELS_DIR: path.join(temporary, "models"),
    APP_REMOTE_DOWNLOADS_DIR: path.join(temporary, "downloads"),
    APP_LOCAL_PROVIDER: "ollama",
    APP_OLLAMA_BASE_URL: "http://127.0.0.1:1",
    APP_OLLAMA_AUTO_START: "false",
    APP_OLLAMA_AUTO_INSTALL: "false",
    APP_LLAMACPP_AUTO_START: "false",
    APP_LLAMACPP_AUTO_INSTALL: "false",
  },
  stdio: ["ignore", "pipe", "pipe"],
});
let log = "";
daemon.stdout.on("data", (data) => { log = (log + data).slice(-20_000); });
daemon.stderr.on("data", (data) => { log = (log + data).slice(-20_000); });
let spawnError;
daemon.on("error", (error) => { spawnError = error; });

async function request(route, method = "GET", body, status = 200) {
  const response = await fetch(`${base}${route}`, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(15_000),
  });
  const text = await response.text();
  assert.equal(response.status, status, `${method} ${route}: ${text}`);
  return text ? JSON.parse(text) : null;
}
const node = (id, kind, parameters = {}) => ({ id, name: id, kind, parameters });
const flow = (name, nodes, edges, extra = {}) => ({
  schema: "mlxflow.v1", name, nodes, edges, ...extra,
});
const save = async (value) => (await request("/flows", "POST", value)).flow;
const run = (id, payload = {}) => request(`/flows/${id}/run`, "POST", { payload });
const output = (record, id) => record.nodes.find((entry) => entry.node_id === id);

try {
  let healthy = false;
  for (let attempt = 0; attempt < 120; attempt++) {
    assert.ifError(spawnError);
    assert.equal(daemon.exitCode, null, log);
    assert.equal(daemon.signalCode, null, log);
    try {
      await request("/health");
      healthy = true;
      break;
    } catch { await delay(250); }
  }
  assert.ok(healthy, `Daemon did not become healthy: ${log}`);
  const catalog = await request("/flows/node-types");
  assert.equal(catalog.nodes.length, 11);
  assert.ok(catalog.nodes.some((entry) => entry.kind === "mcp.call"));

  const conditional = await save(flow("Smoke: conditional branches", [
    node("start", "trigger.manual"),
    node("branch", "flow.if", { condition: "{{ $json.n > 2 }}" }),
    node("double", "data.set", { keep_only_set: true, assignments: [{ name: "value", value: "{{ $json.n * 2 }}" }] }),
    node("other", "debug.log", { message: "small" }),
  ], [{ from: "start", to: "branch" }, { from: "branch", from_port: "true", to: "double" }, { from: "branch", from_port: "false", to: "other" }]));
  assert.equal((await request(`/flows/${conditional.id}`)).name, conditional.name);
  const high = await run(conditional.id, { n: 4 });
  assert.equal(high.status, "success");
  assert.equal(output(high, "double").output.main.items[0].value, 8);
  assert.equal(output(high, "other").status, "skipped");
  const low = await run(conditional.id, { n: 1 });
  assert.equal(low.status, "success");
  assert.equal(output(low, "double").status, "skipped");
  assert.equal(output(low, "other").status, "success");
  assert.ok((await request(`/flows/${conditional.id}/runs`)).runs.length >= 2);
  console.log("PASS: catalog, persistence, expressions, both branches and history");

  const cycle = structuredClone(conditional);
  cycle.edges.push({ from: "double", to: "branch" });
  assert.equal((await request("/flows/validate", "POST", cycle)).valid, false);
  await request("/flows", "POST", cycle, 400);
  const imported = await request("/flows/import/n8n", "POST", {
    name: "Imported smoke", nodes: [{ id: "start", name: "Start", type: "n8n-nodes-base.manualTrigger", parameters: {}, position: [0, 0] }], connections: {},
  });
  assert.equal(imported.validation.valid, true);
  assert.equal(imported.flow.nodes[0].kind, "trigger.manual");
  console.log("PASS: cycle rejection and offline n8n import");

  const webhook = await save(flow("Smoke: webhook", [
    node("hook", "trigger.webhook", { path: "smoke", method: "POST", response_mode: "immediate" }),
    node("copy", "data.set", { keep_only_set: true, assignments: [{ name: "value", value: "{{ $json.body.value }}" }] }),
  ], [{ from: "hook", to: "copy" }], { active: true }));
  const accepted = await request("/flows/webhook/smoke", "POST", { value: 42 }, 202);
  let webhookRun;
  for (let attempt = 0; attempt < 80; attempt++) {
    const response = await fetch(`${base}/flows/runs/${accepted.run_id}`, { signal: AbortSignal.timeout(5000) });
    if (response.ok) { webhookRun = await response.json(); break; }
    assert.equal(response.status, 404);
    await delay(50);
  }
  assert.ok(webhookRun, "Webhook run never became available");
  assert.equal(webhookRun.id, accepted.run_id);
  assert.equal(webhookRun.status, "success");
  assert.equal(output(webhookRun, "copy").output.main.items[0].value, 42);
  await request("/flows/webhook/smoke", "GET", undefined, 405);
  await request("/flows/webhook/missing", "POST", {}, 404);
  console.log("PASS: webhook run identity, payload and HTTP errors");

  await request("/flows/mcp/servers", "POST", { name: "smoke-echo", command: process.execPath, args: [path.join(root, "scripts", "mcp-echo-server.mjs")], enabled: true });
  const probe = await request("/flows/mcp/servers/smoke-echo/probe", "POST");
  assert.equal(probe.reachable, true);
  assert.ok(probe.tools.some((tool) => tool.name === "somar"));
  const mcpFlow = await save(flow("Smoke: MCP", [
    node("start", "trigger.manual"), node("sum", "mcp.call", { server: "smoke-echo", tool: "somar", arguments: { a: "{{ $json.n }}", b: 5 } }),
  ], [{ from: "start", to: "sum" }]));
  const mcpRun = await run(mcpFlow.id, { n: 10 });
  assert.equal(mcpRun.status, "success", JSON.stringify(mcpRun));
  assert.equal(output(mcpRun, "sum").output.main.items[0].mcp_output, "15");
  console.log("PASS: real MCP stdio probe and expression-bound tool execution");
  await writeFile(path.join(workspace, "input.txt"), "native smoke\n");
  const toolFlow = await save(flow("Smoke: sandbox tool", [
    node("start", "trigger.manual"),
    node("read", "tool.call", { tool: "read_file", params: { path: "input.txt" }, read_only: true }),
  ], [{ from: "start", to: "read" }]));
  const toolRun = await run(toolFlow.id);
  assert.equal(toolRun.status, "success", JSON.stringify(toolRun));
  assert.match(output(toolRun, "read").output.main.items[0].tool_output, /native smoke/);
  console.log("PASS: builtin tool execution in the isolated workspace");
  for (const saved of [conditional, webhook, mcpFlow, toolFlow]) await request(`/flows/${saved.id}`, "DELETE");
  await request("/flows/mcp/servers/smoke-echo", "DELETE");
  assert.equal((await request("/flows")).flows.length, 0);
  console.log(`PASS: deletion and isolated cleanup (${base})`);
  if (process.env.FLOW_SMOKE_KEEP_ALIVE === "1") {
    console.log(`READY_FOR_BROWSER ${base}`);
    await new Promise((resolve) => {
      process.once("SIGINT", resolve);
      process.once("SIGTERM", resolve);
    });
  }
} catch (error) {
  console.error(log);
  throw error;
} finally {
  if (daemon.pid && daemon.exitCode === null && daemon.signalCode === null) {
    const exited = once(daemon, "exit");
    daemon.kill();
    await exited;
  }
  assert.equal(path.dirname(temporary), path.resolve(os.tmpdir()));
  await rm(temporary, { recursive: true, force: true });
}
