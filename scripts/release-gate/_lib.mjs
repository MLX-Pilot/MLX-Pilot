#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import fsSync from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

export const repoRoot = "/Users/kaike/mlx-ollama-pilot";
export const releaseGateDir = path.join(repoRoot, "scripts", "release-gate");
export const reportJsonPath = path.join(repoRoot, "docs", "release-gate-report.json");
export const reportMarkdownPath = path.join(repoRoot, "docs", "release-gate-report.md");

export function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function pickPort() {
  return await new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(port);
      });
    });
  });
}

export async function mkdtemp(prefix) {
  return await fs.mkdtemp(path.join(os.tmpdir(), prefix));
}

export async function writeJson(filePath, value) {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

export async function readJson(filePath) {
  return JSON.parse(await fs.readFile(filePath, "utf8"));
}

export async function writeExecutable(filePath, content, mode = 0o755) {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, content, { mode });
  await fs.chmod(filePath, mode);
}

export async function writeSkill(skillsDir, name, content) {
  const dir = path.join(skillsDir, name);
  await fs.mkdir(dir, { recursive: true });
  await fs.writeFile(path.join(dir, "SKILL.md"), content);
}

export async function requestJson(baseUrl, method, route, body, options = {}) {
  const {
    headers = {},
    timeoutMs = 10_000,
    expectedStatuses = null,
  } = options;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(`${baseUrl}${route}`, {
      method,
      headers: {
        "Content-Type": "application/json",
        ...headers,
      },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: controller.signal,
    });
    const text = await response.text();
    let payload = null;
    if (text) {
      try {
        payload = JSON.parse(text);
      } catch {
        payload = text;
      }
    }
    if (
      expectedStatuses &&
      !expectedStatuses.includes(response.status)
    ) {
      throw new Error(`${method} ${route} unexpected ${response.status}: ${text}`);
    }
    return {
      ok: response.ok,
      status: response.status,
      headers: Object.fromEntries(response.headers.entries()),
      payload,
      text,
    };
  } finally {
    clearTimeout(timer);
  }
}

export async function assertOkJson(baseUrl, method, route, body, options = {}) {
  const response = await requestJson(baseUrl, method, route, body, options);
  if (!response.ok) {
    throw new Error(`${method} ${route} failed (${response.status}): ${response.text}`);
  }
  return response.payload;
}

export async function waitForHealth(baseUrl, proc, name = "mlx-ollama-daemon", attempts = 180) {
  for (let index = 0; index < attempts; index += 1) {
    if (proc.exitCode !== null) {
      throw new Error(`${name} exited early with code ${proc.exitCode}`);
    }
    try {
      const response = await fetch(`${baseUrl}/health`);
      if (response.ok) {
        return;
      }
    } catch {}
    await delay(500);
  }
  throw new Error(`Timed out waiting for ${name} health check`);
}

function daemonCommand() {
  const override = process.env.RELEASE_GATE_DAEMON_BIN?.trim();
  if (override) {
    return { command: override, args: [] };
  }

  const debugBin = path.join(repoRoot, "target", "debug", "mlx-ollama-daemon");
  if (fsSync.existsSync(debugBin)) {
    return { command: debugBin, args: [] };
  }

  return {
    command: "cargo",
    args: ["run", "-p", "mlx-ollama-daemon"],
  };
}

export function startDaemon({
  workspace,
  settingsPath,
  port,
  pathPrefix = [],
  env = {},
  name = "release-gate-daemon",
}) {
  const logPath = path.join(path.dirname(settingsPath), `${name}.log`);
  const stdout = fsSync.openSync(logPath, "w");
  const pathValue = [...pathPrefix, process.env.PATH || ""].filter(Boolean).join(path.delimiter);
  const spec = daemonCommand();
  const proc = spawn(spec.command, spec.args, {
    cwd: repoRoot,
    env: {
      ...process.env,
      APP_BIND_ADDR: `127.0.0.1:${port}`,
      APP_SETTINGS_PATH: settingsPath,
      APP_AGENT_WORKSPACE: workspace,
      PATH: pathValue,
      ...env,
    },
    stdio: ["ignore", stdout, stdout],
  });
  fsSync.closeSync(stdout);

  return {
    proc,
    logPath,
    baseUrl: `http://127.0.0.1:${port}`,
  };
}

export async function stopDaemon(instance, signal = "SIGTERM") {
  if (!instance?.proc || instance.proc.exitCode !== null) {
    return;
  }
  instance.proc.kill(signal);
  for (let index = 0; index < 20; index += 1) {
    if (instance.proc.exitCode !== null) {
      return;
    }
    await delay(250);
  }
  instance.proc.kill("SIGKILL");
  await delay(250);
}

export async function startJsonServer(handler, label = "json-server") {
  const port = await pickPort();
  const requests = [];
  const server = http.createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) {
      chunks.push(chunk);
    }
    const raw = Buffer.concat(chunks).toString("utf8");
    let body = null;
    if (raw) {
      try {
        body = JSON.parse(raw);
      } catch {
        body = raw;
      }
    }
    requests.push({
      method: req.method,
      url: req.url,
      headers: req.headers,
      body,
      raw,
    });

    try {
      const result = await handler({
        req,
        res,
        body,
        raw,
        requests,
      });
      if (res.writableEnded) {
        return;
      }
      const status = result?.status ?? 200;
      const payload = result?.body ?? {};
      const headers = result?.headers ?? { "content-type": "application/json" };
      res.writeHead(status, headers);
      if (typeof payload === "string") {
        res.end(payload);
      } else {
        res.end(JSON.stringify(payload));
      }
    } catch (error) {
      res.writeHead(500, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: `${label}_handler_failed`, details: error.message }));
    }
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });

  return {
    label,
    port,
    baseUrl: `http://127.0.0.1:${port}`,
    requests,
    async close() {
      await new Promise((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
    },
  };
}

export async function startOpenAiMockServer(options = {}) {
  const {
    delayMs = 0,
    finalPrefix = "release-gate-mock",
    toolCallName = null,
    toolCallArguments = {},
    toolCallContent = "",
  } = options;

  return await startJsonServer(async ({ req, body }) => {
    if (req.method === "GET" && req.url === "/models") {
      return {
        body: {
          object: "list",
          data: [{ id: "mock-small", object: "model", owned_by: "local" }],
        },
      };
    }

    if (req.method === "POST" && req.url === "/chat/completions") {
      if (delayMs > 0) {
        await delay(delayMs);
      }
      const messages = Array.isArray(body?.messages) ? body.messages : [];
      const hasToolResult = messages.some((entry) => entry.role === "tool");
      const lastUser = messages.filter((entry) => entry.role === "user").at(-1)?.content || "ok";
      if (toolCallName && !hasToolResult) {
        return {
          body: {
            id: "chatcmpl-release-gate-tool",
            object: "chat.completion",
            created: Math.floor(Date.now() / 1000),
            model: body?.model || "mock-small",
            choices: [
              {
                index: 0,
                message: {
                  role: "assistant",
                  content: toolCallContent,
                  tool_calls: [
                    {
                      id: "call_release_gate",
                      type: "function",
                      function: {
                        name: toolCallName,
                        arguments: JSON.stringify(toolCallArguments),
                      },
                    },
                  ],
                },
                finish_reason: "tool_calls",
              },
            ],
            usage: {
              prompt_tokens: 64,
              completion_tokens: 16,
              total_tokens: 80,
            },
          },
        };
      }

      return {
        body: {
          id: "chatcmpl-release-gate-final",
          object: "chat.completion",
          created: Math.floor(Date.now() / 1000),
          model: body?.model || "mock-small",
          choices: [
            {
              index: 0,
              message: {
                role: "assistant",
                content: `${finalPrefix}: ${String(lastUser).slice(0, 120)}`,
              },
              finish_reason: "stop",
            },
          ],
          usage: {
            prompt_tokens: 64,
            completion_tokens: 16,
            total_tokens: 80,
          },
        },
      };
    }

    return {
      status: 404,
      body: { error: "not_found" },
    };
  }, "openai-mock");
}

export async function startBridgeMockServer(options = {}) {
  const {
    failMode = null,
    responseDelayMs = 0,
  } = options;

  return await startJsonServer(async ({ req, body }) => {
    if (responseDelayMs > 0) {
      await delay(responseDelayMs);
    }
    if (!["/login", "/logout", "/probe", "/resolve", "/send"].includes(req.url)) {
      return {
        status: 404,
        body: { error: "invalid_target", message: "unsupported bridge route" },
      };
    }
    if (failMode === "auth") {
      return { status: 401, body: { error: "auth_error", message: "unauthorized" } };
    }
    if (failMode === "provider") {
      return { status: 502, body: { error: "provider_error", message: "upstream failed" } };
    }

    if (req.url === "/login") {
      return {
        body: {
          protocol_version: "v1",
          status: "connected",
          message: "bridge login ok",
        },
      };
    }
    if (req.url === "/logout") {
      return {
        body: {
          protocol_version: "v1",
          status: "logged_out",
          message: "bridge logout ok",
        },
      };
    }
    if (req.url === "/probe") {
      return {
        body: {
          protocol_version: "v1",
          status: "healthy",
          message: "bridge probe ok",
        },
      };
    }
    if (req.url === "/resolve") {
      const target = typeof body?.target === "string" ? body.target : "";
      if (!target.trim()) {
        return { status: 404, body: { error: "invalid_target", message: "missing target" } };
      }
      return {
        body: {
          protocol_version: "v1",
          resolved_target: `canonical:${target.trim().toLowerCase()}`,
        },
      };
    }
    return {
      body: {
        protocol_version: "v1",
        message_id: `bridge-${Math.abs(hashString(body?.target || "message"))}`,
      },
    };
  }, "bridge-mock");
}

export async function startWebhookSink(options = {}) {
  const {
    failMode = null,
    responseDelayMs = 0,
  } = options;
  return await startJsonServer(async ({ req }) => {
    const parsedUrl = new URL(req.url, "http://127.0.0.1");
    if (responseDelayMs > 0) {
      await delay(responseDelayMs);
    }
    if (req.method !== "POST" || parsedUrl.pathname !== "/webhook") {
      return { status: 404, body: { error: "invalid_target" } };
    }
    if (failMode === "auth") {
      return { status: 403, body: { error: "auth_error" } };
    }
    if (failMode === "provider") {
      return { status: 502, body: { error: "provider_error" } };
    }
    return { status: 200, body: { ok: true } };
  }, "webhook-sink");
}

export function buildLegacySettings(workspace) {
  return {
    agent: {
      provider: "custom",
      model_id: "mock-small",
      base_url: "",
      enabled_tools: ["read_file", "exec"],
      enabled_skills: ["mock-install"],
      workspace_root: workspace,
      approval_mode: "auto",
      execution_mode: "full",
      security: {
        use_secrets_vault: true,
      },
    },
  };
}

export async function createHarness(name, { settings = null } = {}) {
  const root = await mkdtemp(`mlx-pilot-${name}-`);
  const workspace = path.join(root, "workspace");
  const skillsDir = path.join(workspace, "skills");
  const settingsPath = path.join(root, "settings.json");
  await fs.mkdir(workspace, { recursive: true });
  await fs.mkdir(skillsDir, { recursive: true });
  if (settings) {
    await writeJson(settingsPath, settings);
  }
  return {
    root,
    workspace,
    skillsDir,
    settingsPath,
  };
}

export async function seedReleaseGateSkills(skillsDir) {
  await writeSkill(
    skillsDir,
    "mock-install",
    `---
name: mock-install
description: Mock install helper for release gate scripts.
metadata:
  openclaw:
    requires:
      bins:
        - rg
---

# Mock Install
`,
  );

  await writeSkill(
    skillsDir,
    "summarize",
    `---
name: summarize
description: Summary helper.
metadata:
  openclaw:
    primaryEnv: OPENAI_API_KEY
    requires:
      env:
        - OPENAI_API_KEY
      config:
        - provider
---

# Summarize
`,
  );
}

export async function configureCustomProvider(baseUrl, workspace) {
  return {
    provider: "custom",
    model_id: "mock-small",
    base_url: baseUrl,
    api_key: "",
    api_key_ref: null,
    custom_headers: {},
    approval_mode: "auto",
    execution_mode: "full",
    streaming: false,
    fallback_enabled: false,
    fallback_provider: "mlx",
    fallback_model_id: "",
    max_prompt_tokens: 2200,
    max_history_messages: 14,
    max_tools_in_prompt: 6,
    temperature: 0.1,
    aggressive_tool_filtering: true,
    enable_tool_call_fallback: true,
    workspace_root: workspace,
    enabled_skills: ["mock-install", "summarize"],
    node_package_manager: "npm",
    skill_overrides: {},
    enabled_tools: [],
    tool_policy: {
      profile: "coding",
      agent_overrides: {},
      session_overrides: {},
    },
    security: {
      security_mode: "standard",
      require_capabilities: false,
      airgapped: false,
      owner_only: false,
      block_direct_ip_egress: true,
      tool_allowlist: [],
      tool_denylist: [],
      exec_safe_bins: ["ls", "cat", "grep", "git", "find", "rg"],
      exec_deny_patterns: ["rm -rf *", "sudo *", "chmod 777 *", "mkfs*"],
      sensitive_paths: ["~/.ssh/*", "~/.aws/*", "~/.gnupg/*", "**/.env", "**/.env.*"],
      egress_allow_domains: [],
      skill_sha256_pins: {},
      use_secrets_vault: true,
    },
  };
}

export function percentile(values, ratio) {
  if (!values.length) {
    return 0;
  }
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * ratio) - 1));
  return sorted[index];
}

export function summarizeLatencies(latencies) {
  return {
    count: latencies.length,
    min_ms: latencies.length ? Math.min(...latencies) : 0,
    max_ms: latencies.length ? Math.max(...latencies) : 0,
    p50_ms: percentile(latencies, 0.5),
    p95_ms: percentile(latencies, 0.95),
    p99_ms: percentile(latencies, 0.99),
  };
}

export function hashString(value) {
  let out = 0;
  for (const char of String(value)) {
    out = (out * 131 + char.charCodeAt(0)) | 0;
  }
  return out;
}

export async function statMode(filePath) {
  const stats = await fs.stat(filePath);
  return stats.mode & 0o777;
}

export function containsAnySecret(text, markers) {
  return markers.some((marker) => marker && text.includes(marker));
}

export async function scanFilesForMarkers(paths, markers) {
  const hits = [];
  for (const filePath of paths) {
    if (!filePath || !fsSync.existsSync(filePath)) {
      continue;
    }
    const raw = await fs.readFile(filePath, "utf8");
    if (containsAnySecret(raw, markers)) {
      hits.push(filePath);
    }
  }
  return hits;
}

export async function listFilesRecursive(root) {
  const out = [];
  async function walk(current) {
    if (!fsSync.existsSync(current)) {
      return;
    }
    const entries = await fs.readdir(current, { withFileTypes: true });
    for (const entry of entries) {
      const next = path.join(current, entry.name);
      if (entry.isDirectory()) {
        await walk(next);
      } else if (entry.isFile()) {
        out.push(next);
      }
    }
  }
  await walk(root);
  return out.sort();
}

export function canonicalChannelError(payloadOrText) {
  const detail =
    typeof payloadOrText === "string"
      ? payloadOrText
      : payloadOrText?.details || payloadOrText?.error || JSON.stringify(payloadOrText);
  const match = String(detail).match(
    /(auth_error|network_error|provider_error|invalid_target|rate_limited|invalid_request|permission_error)/,
  );
  return match ? match[1] : "provider_error";
}

export function runReproductionCommands() {
  return [
    "cargo check -p mlx-ollama-daemon",
    "cargo test -p mlx-ollama-daemon",
    "cd apps/desktop-ui && npm run test:e2e:channels-smoke",
    "cd apps/desktop-ui && npm run test:e2e:skills-smoke",
    "node scripts/release-gate/run-all.mjs",
  ];
}

export function isDirectRun(metaUrl) {
  return process.argv[1] && pathToFileURL(process.argv[1]).href === metaUrl;
}

export function printStandaloneResult(result) {
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
}

export async function ensureHealthy(daemon) {
  const response = await requestJson(daemon.baseUrl, "GET", "/health", undefined, {
    expectedStatuses: [200],
  });
  assert.equal(response.status, 200);
}
