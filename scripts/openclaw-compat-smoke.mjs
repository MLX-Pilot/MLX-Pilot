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

const repoRoot = "/Users/kaike/mlx-ollama-pilot";
const reportJsonPath = path.join(repoRoot, "docs", "openclaw-compat-report.json");
const reportMarkdownPath = path.join(repoRoot, "docs", "openclaw-compat-report.md");

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function pickPort() {
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

async function writeExecutable(filePath, content, mode = 0o755) {
  await fs.writeFile(filePath, content, { mode });
  await fs.chmod(filePath, mode);
}

async function writeSkill(skillsDir, name, content) {
  const dir = path.join(skillsDir, name);
  await fs.mkdir(dir, { recursive: true });
  await fs.writeFile(path.join(dir, "SKILL.md"), content);
}

async function request(baseUrl, method, route, body) {
  const response = await fetch(`${baseUrl}${route}`, {
    method,
    headers: {
      "Content-Type": "application/json",
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;
  if (!response.ok) {
    throw new Error(`${method} ${route} failed (${response.status}): ${text}`);
  }
  return payload;
}

async function waitForHealth(baseUrl, proc, name) {
  for (let index = 0; index < 180; index += 1) {
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

function startDaemon({
  workspace,
  settingsPath,
  port,
  pathPrefix = [],
  env = {},
  npmPrefix,
  npmCache,
  gobin,
}) {
  const logPath = path.join(path.dirname(settingsPath), "openclaw-compat-daemon.log");
  const stdout = fsSync.openSync(logPath, "w");
  const pathValue = [...pathPrefix, process.env.PATH || ""].filter(Boolean).join(path.delimiter);
  const proc = spawn("cargo", ["run", "-p", "mlx-ollama-daemon"], {
    cwd: repoRoot,
    env: {
      ...process.env,
      APP_BIND_ADDR: `127.0.0.1:${port}`,
      APP_SETTINGS_PATH: settingsPath,
      APP_AGENT_WORKSPACE: workspace,
      npm_config_prefix: npmPrefix,
      NPM_CONFIG_PREFIX: npmPrefix,
      npm_config_cache: npmCache,
      NPM_CONFIG_CACHE: npmCache,
      GOBIN: gobin,
      PATH: pathValue,
      ...env,
    },
    stdio: ["ignore", stdout, stdout],
  });
  fsSync.closeSync(stdout);

  return {
    proc,
    baseUrl: `http://127.0.0.1:${port}`,
    logPath,
  };
}

async function stopDaemon(instance) {
  if (!instance?.proc || instance.proc.exitCode !== null) {
    return;
  }
  instance.proc.kill("SIGTERM");
  for (let index = 0; index < 20; index += 1) {
    if (instance.proc.exitCode !== null) {
      return;
    }
    await delay(250);
  }
  instance.proc.kill("SIGKILL");
  await delay(250);
}

async function startMockProvider() {
  const port = await pickPort();
  const requests = [];
  const server = http.createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) {
      chunks.push(chunk);
    }
    const raw = Buffer.concat(chunks).toString("utf8");
    const body = raw ? JSON.parse(raw) : null;
    requests.push({ method: req.method, url: req.url, body });

    if (req.method === "GET" && req.url === "/models") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          object: "list",
          data: [{ id: "mock-small", object: "model", owned_by: "local" }],
        }),
      );
      return;
    }

    if (req.method === "POST" && req.url === "/chat/completions") {
      const lastUser =
        body?.messages?.filter?.((entry) => entry.role === "user").at(-1)?.content || "ok";
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          id: "chatcmpl-local-compat",
          object: "chat.completion",
          created: Math.floor(Date.now() / 1000),
          model: body?.model || "mock-small",
          choices: [
            {
              index: 0,
              message: {
                role: "assistant",
                content: `compat-mock: ${String(lastUser).slice(0, 80)}`,
              },
              finish_reason: "stop",
            },
          ],
          usage: {
            prompt_tokens: 128,
            completion_tokens: 32,
            total_tokens: 160,
          },
        }),
      );
      return;
    }

    res.writeHead(404, { "content-type": "application/json" });
    res.end(JSON.stringify({ error: "not_found" }));
  });

  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolve);
  });

  return {
    baseUrl: `http://127.0.0.1:${port}`,
    requests,
    async close() {
      await new Promise((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())));
    },
  };
}

function buildLegacySettings(workspace) {
  return {
    agent: {
      provider: "ollama",
      model_id: "qwen2.5-coder:7b",
      enabled_tools: ["read_file", "exec"],
      enabled_skills: ["mock-install"],
      workspace_root: workspace,
      approval_mode: "auto",
      execution_mode: "full",
    },
  };
}

function renderMarkdown(report, steps) {
  const gaps = report.gaps.length
    ? report.gaps
        .map(
          (gap) =>
            `- [${gap.severity}] ${gap.area}/${gap.id}: ${gap.message} Action: ${gap.action}`,
        )
        .join("\n")
    : "- Nenhum gap residual.";

  const channels = report.channels
    .map(
      (channel) =>
        `- ${channel.id}: ${channel.state}, contas=${channel.account_count}, local_testable=${channel.local_testable}`,
    )
    .join("\n");

  const plugins = report.plugins
    .map((plugin) => `- ${plugin.id}: ${plugin.state}, enabled=${plugin.enabled}, health=${plugin.health}`)
    .join("\n");

  const skills = report.skills.entries
    .map((skill) => `- ${skill.name}: ${skill.status}, eligible=${skill.eligible}, active=${skill.active}`)
    .join("\n");

  const toolProfiles = report.tools.profiles
    .map(
      (profile) =>
        `- ${profile.id}: coverage=${profile.coverage_percent}%, allowed=${profile.allowed_tools}, blocked=${profile.blocked_tools}`,
    )
    .join("\n");

  return `# OpenClaw Compatibility Report

- Generated at: ${report.generated_at}
- Mode: ${report.mode}
- Coverage: ${report.summary.coverage_percent}% (${report.summary.passed_checks}/${report.summary.total_checks})
- Critical gaps: ${report.summary.critical_gaps}
- Warning gaps: ${report.summary.warning_gaps}
- Config schema: v${report.migration.schema_version}
- Migration flags: ${report.migration.migration_flags.join(", ")}

## Validated flows

${steps.map((step) => `- ${step}`).join("\n")}

## Channels

${channels}

## Plugins

${plugins}

## Skills

${skills}

## Tool Profiles

${toolProfiles}

## Context Benchmark

- Model: ${report.context_benchmark.model_id}
- Profile: ${report.context_benchmark.model_profile}
- Status: ${report.context_benchmark.status}
- Prompt tokens: ${report.context_benchmark.prompt_tokens_after_compression}/${report.context_benchmark.max_prompt_tokens}
- Summaries: ${report.context_benchmark.summary_entries}
- Recommendation: ${report.context_benchmark.recommendation}

## Remaining Gaps

${gaps}

## Re-run

\`\`\`bash
node /Users/kaike/mlx-ollama-pilot/scripts/openclaw-compat-smoke.mjs
\`\`\`
`;
}

async function main() {
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "mlx-pilot-openclaw-compat-"));
  const workspace = path.join(tempRoot, "workspace");
  const skillsDir = path.join(workspace, "skills");
  const binsDir = path.join(tempRoot, "bins");
  const npmPrefix = path.join(tempRoot, "npm-global");
  const npmCache = path.join(tempRoot, "npm-cache");
  const goBin = path.join(tempRoot, "go-bin");
  const settingsPath = path.join(tempRoot, "settings.json");
  const daemonPort = await pickPort();
  const steps = [];

  let daemon;
  let mockProvider;

  try {
    await fs.mkdir(workspace, { recursive: true });
    await fs.mkdir(skillsDir, { recursive: true });
    await fs.mkdir(binsDir, { recursive: true });
    await fs.mkdir(path.join(npmPrefix, "bin"), { recursive: true });
    await fs.mkdir(npmCache, { recursive: true });
    await fs.mkdir(goBin, { recursive: true });

    await writeExecutable(
      path.join(binsDir, "go"),
      [
        "#!/bin/sh",
        "set -eu",
        'if [ "$1" = "install" ]; then',
        '  mkdir -p "${GOBIN:-${HOME}/go/bin}"',
        '  cat <<\'EOF\' > "${GOBIN:-${HOME}/go/bin}/stringer"',
        "#!/bin/sh",
        "echo stringer mock",
        "EOF",
        '  chmod +x "${GOBIN:-${HOME}/go/bin}/stringer"',
        '  echo "installed $2"',
        "  exit 0",
        "fi",
        'echo "unsupported go invocation" >&2',
        "exit 1",
        "",
      ].join("\n"),
    );
    await writeExecutable(path.join(binsDir, "curl"), "#!/bin/sh\nexit 0\n");

    await writeSkill(
      skillsDir,
      "mock-install",
      `---
name: mock-install
description: Mock installer for compatibility smoke.
metadata:
  openclaw:
    requires:
      anyBins:
        - stringer
    install:
      - id: mock-go
        kind: go
        module: golang.org/x/tools/cmd/stringer
        bins:
          - stringer
        label: Install stringer
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
    await writeSkill(
      skillsDir,
      "weather",
      `---
name: weather
description: Weather helper.
metadata:
  openclaw:
    requires:
      bins:
        - curl
---

# Weather
`,
    );

    await fs.writeFile(settingsPath, JSON.stringify(buildLegacySettings(workspace), null, 2));
    mockProvider = await startMockProvider();
    daemon = startDaemon({
      workspace,
      settingsPath,
      port: daemonPort,
      pathPrefix: [binsDir, goBin, path.join(npmPrefix, "bin")],
      npmPrefix,
      npmCache,
      gobin: goBin,
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "mlx-ollama-daemon");

    const initialConfig = await request(daemon.baseUrl, "GET", "/agent/config");
    const updatedConfig = {
      ...initialConfig,
      provider: "custom",
      model_id: "mock-small",
      base_url: mockProvider.baseUrl,
      api_key: "",
      approval_mode: "auto",
      execution_mode: "full",
      streaming: false,
      workspace_root: workspace,
      enabled_skills: ["weather", "mock-install", "summarize"],
      node_package_manager: "npm",
      security: {
        ...initialConfig.security,
        use_secrets_vault: false,
      },
    };
    const savedConfig = await request(daemon.baseUrl, "POST", "/agent/config", updatedConfig);
    assert.equal(savedConfig.provider, "custom");
    assert.equal(savedConfig.base_url, mockProvider.baseUrl);
    steps.push("onboarding non-interativo atualizado via /agent/config");

    const whatsapp = await request(daemon.baseUrl, "POST", "/agent/channels/upsert-account", {
      channel: "whatsapp",
      account_id: "ops",
      enabled: true,
      metadata: { owner: "compat" },
      routing_defaults: { target: "+5511999999999" },
      adapter_config: {},
      set_as_default: true,
    });
    assert.equal(whatsapp.id, "whatsapp");
    const whatsappLogin = await request(daemon.baseUrl, "POST", "/agent/channels/login", {
      channel: "whatsapp",
      account_id: "ops",
    });
    assert.equal(whatsappLogin.status, "connected");
    const whatsappProbe = await request(daemon.baseUrl, "POST", "/agent/channels/probe", {
      channel: "whatsapp",
      account_id: "ops",
    });
    assert.equal(whatsappProbe[0].status, "healthy");
    const whatsappSend = await request(daemon.baseUrl, "POST", "/agent/message/send", {
      channel: "whatsapp",
      account_id: "ops",
      target: "+5511999999999",
      message: "compat smoke whatsapp",
    });
    assert.equal(whatsappSend.status, "sent");
    steps.push("WhatsApp local: upsert, login, probe e envio de mensagem");

    await request(daemon.baseUrl, "POST", "/agent/channels/upsert-account", {
      channel: "telegram",
      account_id: "bot",
      enabled: true,
      credentials: { token: "tg-secret" },
      metadata: { owner: "compat" },
      routing_defaults: { target: "@compat_bot" },
      adapter_config: {},
      set_as_default: true,
    });
    const telegramLogin = await request(daemon.baseUrl, "POST", "/agent/channels/login", {
      channel: "telegram",
      account_id: "bot",
    });
    assert.equal(telegramLogin.status, "connected");
    const telegramProbe = await request(daemon.baseUrl, "POST", "/agent/channels/probe", {
      channel: "telegram",
      account_id: "bot",
    });
    assert.equal(telegramProbe[0].status, "healthy");
    const telegramSend = await request(daemon.baseUrl, "POST", "/agent/message/send", {
      channel: "telegram",
      account_id: "bot",
      target: "@compat_bot",
      message: "compat smoke telegram",
    });
    assert.equal(telegramSend.status, "sent");
    steps.push("Telegram token-bot: upsert, login, probe e envio de mensagem");

    const plugins = await request(daemon.baseUrl, "GET", "/agent/plugins");
    assert.ok(plugins.some((entry) => entry.id === "memory"));
    const pluginEnabled = await request(daemon.baseUrl, "POST", "/agent/plugins/enable", {
      plugin_id: "memory",
    });
    assert.equal(pluginEnabled.enabled, true);
    const pluginDisabled = await request(daemon.baseUrl, "POST", "/agent/plugins/disable", {
      plugin_id: "memory",
    });
    assert.equal(pluginDisabled.enabled, false);
    steps.push("Plugin memory habilitado e desabilitado sem regressao");

    const skillsBefore = await request(daemon.baseUrl, "GET", "/agent/skills/check");
    assert.ok(skillsBefore.skills.some((skill) => skill.name === "mock-install" && skill.eligible === false));
    assert.ok(skillsBefore.skills.some((skill) => skill.name === "summarize" && skill.eligible === false));

    const installResult = await request(daemon.baseUrl, "POST", "/agent/skills/install", {
      skills: ["mock-install"],
      node_manager: "npm",
    });
    assert.equal(installResult.results[0].installs[0].ok, true);
    await request(daemon.baseUrl, "POST", "/agent/skills/enable", {
      skill: "mock-install",
    });
    const configuredSkill = await request(daemon.baseUrl, "POST", "/agent/skills/config", {
      skill: "summarize",
      enabled: true,
      env: { OPENAI_API_KEY: "sk-test-local" },
      config: { provider: "custom" },
    });
    assert.equal(configuredSkill.active, true);
    const skillsAfter = await request(daemon.baseUrl, "GET", "/agent/skills/check");
    assert.ok(skillsAfter.skills.some((skill) => skill.name === "mock-install" && skill.eligible === true));
    assert.ok(skillsAfter.skills.some((skill) => skill.name === "summarize" && skill.active === true));
    steps.push("Skills check/install/enable/config executados de ponta a ponta");

    const toolPolicy = await request(daemon.baseUrl, "POST", "/agent/tools/profile", {
      profile: "full",
    });
    assert.equal(toolPolicy.profile, "full");
    const effectivePolicy = await request(
      daemon.baseUrl,
      "GET",
      "/agent/tools/effective-policy?agent_id=default",
    );
    assert.equal(effectivePolicy.profile, "full");
    steps.push("Troca de tools profile para full com politica efetiva sincronizada");

    const runResponse = await request(daemon.baseUrl, "POST", "/agent/run", {
      session_id: "compat-smoke-run",
      message: "Executar loop do agente em modo compatibilidade local.",
      provider: "custom",
      model_id: "mock-small",
      base_url: mockProvider.baseUrl,
      approval_mode: "auto",
      execution_mode: "full",
      workspace_root: workspace,
    });
    assert.match(runResponse.final_response, /compat-mock:/);
    const budget = await request(
      daemon.baseUrl,
      "GET",
      "/agent/context/budget?session_id=compat-smoke-run",
    );
    assert.ok(budget.prompt_tokens_estimate > 0);
    steps.push("Agent loop executado com provider OpenAI-compatible local + budget telemetry");

    const channelsStatus = await request(daemon.baseUrl, "GET", "/agent/channels/status");
    assert.ok(channelsStatus.some((entry) => entry.id === "whatsapp"));
    const tools = await request(daemon.baseUrl, "GET", "/agent/tools");
    assert.ok(tools.length > 0);

    const compatReport = await request(daemon.baseUrl, "GET", "/agent/compat/report");
    assert.ok(compatReport.summary.coverage_percent >= 95, `coverage too low: ${compatReport.summary.coverage_percent}`);
    assert.equal(compatReport.migration.schema_version, 2);
    assert.equal(compatReport.tools.selected_profile, "full");
    assert.equal(compatReport.context_benchmark.model_profile, "small_local");
    assert.ok(
      compatReport.endpoint_compatibility.some((entry) => entry.path === "/agent/run" && entry.backward_compatible),
    );
    steps.push("Compatibility matrix consolidada via /agent/compat/report com cobertura >= 95%");

    const savedSettings = JSON.parse(await fs.readFile(settingsPath, "utf8"));
    assert.equal(savedSettings.schema_version, 2);
    assert.equal(savedSettings.agent.provider, "custom");
    assert.equal(savedSettings.agent.tool_policy.profile, "full");

    await fs.writeFile(reportJsonPath, `${JSON.stringify(compatReport, null, 2)}\n`);
    await fs.writeFile(reportMarkdownPath, renderMarkdown(compatReport, steps));

    console.log(`compat report written to ${reportJsonPath}`);
    console.log(`compat markdown written to ${reportMarkdownPath}`);
    console.log(`coverage: ${compatReport.summary.coverage_percent}%`);
    console.log(`mock requests: ${mockProvider.requests.length}`);
  } finally {
    await stopDaemon(daemon);
    await mockProvider?.close?.();
  }
}

main().catch((error) => {
  console.error(error.stack || error.message || String(error));
  process.exitCode = 1;
});
