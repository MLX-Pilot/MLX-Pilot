#!/usr/bin/env node

import fs from "node:fs/promises";
import path from "node:path";

import {
  assertOkJson,
  buildLegacySettings,
  configureCustomProvider,
  createHarness,
  isDirectRun,
  pickPort,
  printStandaloneResult,
  requestJson,
  scanFilesForMarkers,
  seedReleaseGateSkills,
  startDaemon,
  startJsonServer,
  startOpenAiMockServer,
  startWebhookSink,
  listFilesRecursive,
  statMode,
  stopDaemon,
  waitForHealth,
  writeExecutable,
  writeJson,
} from "./_lib.mjs";

async function checkChatInputs(baseUrl, llama, agentProvider) {
  const model = "llama::fixture.gguf";
  const message = (content) => [{ role: "user", content }];
  const calls = () => llama.requests.filter((req) => req.url === "/v1/chat/completions").length
    + agentProvider.requests.filter((req) => req.url === "/chat/completions").length;
  const checks = [];
  async function reject(route, label, body, expectedError) {
    const before = calls();
    const { status, payload } = await requestJson(baseUrl, "POST", route, body);
    const modelCalls = calls() - before;
    checks.push({ route, input: label, status, model_calls: modelCalls,
      passed: status === 400 && payload?.error === expectedError && modelCalls === 0 });
  }
  for (const route of ["/chat", "/chat/stream"]) {
    const before = calls();
    const control = await requestJson(baseUrl, "POST", route, { model_id: model, messages: message("Olá\nOlá") });
    const modelCalls = calls() - before;
    const complete = route === "/chat" ? control.payload?.message?.content === "gate-ready"
      : control.text.split("\n").filter(Boolean).map((line) => JSON.parse(line)).some((frame) => frame.event === "done");
    checks.push({ route, input: "valid multiline", status: control.status, model_calls: modelCalls,
      passed: control.status === 200 && complete && modelCalls === 1 });
    await reject(route, "no messages", { model_id: model, messages: [] }, "messages cannot be empty");
    await reject(route, "blank message", { model_id: model, messages: message("   ") }, "message cannot be empty");
    await reject(route, "NUL in message", { model_id: model, messages: message("Hello\u0000world") }, "message contains a control character");
    await reject(route, "blank model", { model_id: " ", messages: message("Hello") }, "model_id cannot be empty");
    await reject(route, "provider without model", { model_id: "ollama::", messages: message("Hello") }, "model_id cannot be empty");
    await reject(route, "NUL in model", { model_id: `${model}\u0000x`, messages: message("Hello") }, "model_id contains a control character");
  }
  for (const route of ["/agent/run", "/agent/stream", "/agent/gateway/events"]) {
    const blankError = route === "/agent/gateway/events" ? "message/text/prompt/input cannot be empty" : "message cannot be empty";
    await reject(route, "blank message", { message: "   " }, blankError);
    await reject(route, "NUL in message", { message: "Hello\u0000world" }, "message contains a control character");
  }
  return checks;
}

export async function runSecurityAuditSmoke() {
  const harness = await createHarness("release-gate-security");
  const daemonPort = await pickPort();
  const secrets = [
    "sk-release-gate-agent",
    "tg-release-gate-channel",
    "sk-release-gate-skill",
    "release-gate-webhook-secret",
  ];

  let daemon;
  let provider;
  let llama;
  let webhook;

  try {
    await seedReleaseGateSkills(harness.skillsDir);
    await writeJson(harness.settingsPath, buildLegacySettings(harness.workspace));
    provider = await startOpenAiMockServer({
      toolCallName: "exec",
      toolCallArguments: { command: "ls" },
      finalPrefix: "policy-allow",
    });
    // The test owns the loopback model server. The launcher's successful return
    // leaves the already-listening fixture in place on each platform.
    llama = await startJsonServer(({ req }) => {
      if (req.url === "/health") return { body: { status: "ok" } };
      if (req.method === "POST" && req.url === "/v1/chat/completions") {
        return { body: { choices: [{ message: { role: "assistant", content: "gate-ready" } }] } };
      }
      return { status: 404 };
    }, "security-chat-provider");
    const windows = process.platform === "win32";
    const launcher = path.join(harness.root, windows ? "fixture-server.cmd" : "fixture-server");
    await writeExecutable(launcher, windows ? "@exit /b 0\r\n" : "#!/bin/sh\nexit 0\n");
    const modelsDir = path.join(harness.root, "models");
    await fs.mkdir(modelsDir, { recursive: true });
    await fs.writeFile(path.join(modelsDir, "fixture.gguf"), "HTTP model fixture");
    const temporary = path.join(harness.root, "temp");
    await fs.mkdir(temporary);
    webhook = await startWebhookSink();

    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      llamaFixture: { port: llama.port, launcher },
      env: { TMPDIR: temporary, TMP: temporary, TEMP: temporary },
      name: "release-gate-security",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "security audit daemon");

    const config = await configureCustomProvider(provider.baseUrl, harness.workspace);
    config.api_key = secrets[0];
    await assertOkJson(daemon.baseUrl, "POST", "/agent/config", config);
    const inputChecks = await checkChatInputs(daemon.baseUrl, llama, provider);

    await assertOkJson(daemon.baseUrl, "POST", "/agent/skills/config", {
      skill: "summarize",
      enabled: true,
      env: {
        OPENAI_API_KEY: secrets[2],
      },
      config: {
        provider: "custom",
      },
    });

    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/upsert-account", {
      channel: "telegram",
      account_id: "secure",
      enabled: true,
      credentials: {
        token: secrets[1],
      },
      metadata: {
        owner: "security",
      },
      routing_defaults: {
        target: "@secure_target",
      },
      adapter_config: {},
      set_as_default: true,
    });

    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/upsert-account", {
      channel: "googlechat",
      account_id: "secure-webhook",
      enabled: true,
      credentials: {
        webhook_url: `${webhook.baseUrl}/webhook?token=${secrets[3]}`,
      },
      metadata: {
        owner: "security",
      },
      routing_defaults: {
        target: "security-room",
      },
      adapter_config: {},
      set_as_default: true,
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/login", {
      channel: "googlechat",
      account_id: "secure-webhook",
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/probe", {
      channel: "googlechat",
      account_id: "secure-webhook",
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/message/send", {
      channel: "googlechat",
      account_id: "secure-webhook",
      target: "security-room",
      message: "security audit",
    });

    const settingsRaw = await fs.readFile(harness.settingsPath, "utf8");

    const skillConfig = await assertOkJson(daemon.baseUrl, "GET", "/agent/skills/check");
    const savedSettings = JSON.parse(settingsRaw);
    const vaultKeyPath = path.join(harness.root, "agent_secrets.key");
    const vaultDataPath = path.join(harness.root, "agent_secrets.v1.json");
    const keyMode = await statMode(vaultKeyPath);
    const dataMode = await statMode(vaultDataPath);

    const profileEvidence = {};
    for (const profile of ["minimal", "coding", "messaging", "full"]) {
      await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/profile", { profile });
      const effective = await assertOkJson(
        daemon.baseUrl,
        "GET",
        "/agent/tools/effective-policy?agent_id=default",
      );
      profileEvidence[profile] = {
        exec_allowed: effective.entries.find((entry) => entry.name === "exec")?.allowed ?? false,
        message_allowed: effective.entries.find((entry) => entry.name === "message")?.allowed ?? false,
      };
    }

    // Tool policy and skill capability are independent restrictions. Pin the active
    // skills for each control, regardless of which optional binaries the host has.
    async function probeExec(sessionId, settings = {}) {
      const run = await requestJson(daemon.baseUrl, "POST", "/agent/run", {
        session_id: sessionId, message: "Run ls using exec",
        provider: "custom", model_id: "mock-small", base_url: provider.baseUrl,
        workspace_root: harness.workspace, enabled_skills: [], ...settings,
      }, { expectedStatuses: [200, 403] });
      const audit = await assertOkJson(daemon.baseUrl, "GET", `/agent/audit?session_id=${sessionId}&limit=50`);
      return { run, audit };
    }

    await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/profile", { profile: "minimal" });
    const { run: deniedRun, audit: deniedAudit } = await probeExec("security-policy-deny");

    await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/allow-deny", {
      scope: "agent",
      agent_id: "default",
      allow: ["exec"],
      deny: [],
      replace: false,
    });
    const { run: allowedRun, audit: allowedAudit } = await probeExec("security-policy-allow");
    const modeChecks = {};
    const restrictedControls = [
      ["read-only", { execution_mode: "read-only" }, "execution_mode:read_only"],
      ["locked", { execution_mode: "locked" }, "execution_mode:locked"],
      ["summarize", { enabled_skills: ["summarize"] }, "skill:summarize:exec"],
    ];
    for (const [name, settings, rule] of restrictedControls) {
      const { audit } = await probeExec(`controlled-exec-${name}`, settings);
      const events = audit.entries.filter(entry => entry.tool_name === "exec");
      modeChecks[name] = events.some(entry => entry.event_type === "tool_denied" && entry.policy_rule === rule) &&
        !events.some(entry => entry.event_type === "tool_executed");
    }

    await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/profile", { profile: "full" });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/allow-deny", {
      scope: "global",
      allow: [],
      deny: ["exec"],
      replace: true,
    });
    const { run: deniedByOverride, audit: deniedOverrideAudit } = await probeExec("security-policy-global-deny");

    const noExec = (audit) => !audit.entries.some(
      (entry) => entry.event_type === "tool_executed" && entry.tool_name === "exec",
    );
    const minimalDenied = noExec(deniedAudit) && deniedAudit.entries.some(
      (entry) => entry.event_type === "tool_denied" && entry.tool_name === "exec",
    );
    const allowExecuted = allowedAudit.entries.some(
      (entry) => entry.event_type === "tool_executed" && entry.tool_name === "exec",
    );
    const overrideDenied = noExec(deniedOverrideAudit) && deniedOverrideAudit.entries.some(
      (entry) => entry.event_type === "tool_denied" && entry.tool_name === "exec",
    );

    await stopDaemon(daemon);
    const leakedFiles = await scanFilesForMarkers(await listFilesRecursive(harness.root), secrets);
    const findings = [];
    const failedInputs = inputChecks.filter((check) => !check.passed);
    if (failedInputs.length) {
      findings.push({ severity: "high", area: "chat_input", title: "Chat input validation diverged from its contract",
        evidence: failedInputs, action: "Validate input before routing or starting model inference." });
    }
    if (leakedFiles.length) {
      findings.push({
        severity: "high",
        area: "logs_and_state",
        title: "Secrets exposed in filesystem artifacts",
        evidence: leakedFiles,
        action: "Redact or remove sensitive values from logs/state before release.",
      });
    }
    if (!savedSettings.agent.api_key_ref || savedSettings.agent.api_key) {
      findings.push({
        severity: "high",
        area: "vault",
        title: "Agent API key not persisted via vault reference",
        evidence: [savedSettings.agent.api_key_ref || "<missing>"],
        action: "Persist agent api_key exclusively through vault references.",
      });
    }
    const summarizeOverride = savedSettings.agent.skill_overrides?.summarize;
    if (!summarizeOverride?.env_refs?.OPENAI_API_KEY || summarizeOverride?.env?.OPENAI_API_KEY) {
      findings.push({
        severity: "high",
        area: "vault",
        title: "Skill secret env was not moved to vault",
        evidence: ["Missing protected environment reference or plaintext value present"],
        action: "Store secret-like env keys through env_refs backed by the vault.",
      });
    }
    if (keyMode !== 0o600) {
      findings.push({
        severity: "medium",
        area: "permissions",
        title: "Vault key file permissions are wider than expected",
        evidence: [`${vaultKeyPath} mode=${keyMode.toString(8)}`],
        action: "Force chmod 600 on the vault key file during creation and startup.",
      });
    }
    if (!minimalDenied || !allowExecuted || !overrideDenied || Object.values(modeChecks).includes(false)) {
      findings.push({
        severity: "high",
        area: "policy",
        title: "Tool policy allow/deny enforcement diverged from expected behavior",
        evidence: [
          `minimal exec run status=${deniedRun.status} denied_event=${minimalDenied}`,
          `agent allow exec run status=${allowedRun.status} executed_event=${allowExecuted}`,
          ...allowedAudit.entries.filter(entry => entry.tool_name === "exec")
            .map(entry => [entry.event_type, entry.policy_rule, entry.reason, entry.error_summary].filter(Boolean).join(" · ")),
          `execution mode and skill denials=${JSON.stringify(modeChecks)}`,
          `global deny exec run status=${deniedByOverride.status} denied_event=${overrideDenied}`,
        ],
        action: "Revisar precedence entre profile/global/agent overrides antes do release.",
      });
    }

    return {
      block: "security_audit_smoke",
      title: "Bloco 4 - Auditoria pratica de seguranca",
      status: findings.some((entry) => entry.severity === "high") ? "fail" : "pass",
      summary: {
        leaked_secret_files: leakedFiles.length,
        vault_key_mode: keyMode.toString(8),
        vault_data_mode: dataMode.toString(8),
        findings_total: findings.length,
      },
      checklist: {
        chat_input_backend: "llama.cpp-compatible loopback HTTP fixture; agent uses OpenAI-compatible loopback HTTP",
        chat_inputs: inputChecks,
        secrets_not_in_settings_or_logs: leakedFiles.length === 0,
        vault_used_for_agent_api_key: Boolean(savedSettings.agent.api_key_ref) && !savedSettings.agent.api_key,
        vault_used_for_skill_env: Boolean(summarizeOverride?.env_refs?.OPENAI_API_KEY),
        vault_used_for_channel_credentials:
          savedSettings.compatibility.channels.telegram.accounts.secure.credentials_ref ===
          "channels.telegram.secure.credentials",
        profile_policy_effective: profileEvidence,
        allow_deny_runtime: {
          minimal_exec_denied: minimalDenied,
          agent_allow_exec_passed: allowExecuted,
          global_deny_exec_denied: overrideDenied,
          execution_mode_and_skill_denials: modeChecks,
        },
      },
      findings,
      commands: [
        "node scripts/release-gate/security-audit-smoke.mjs",
      ],
      artifacts: {
        settings_path: harness.settingsPath,
        daemon_log: daemon.logPath,
        vault_key_path: vaultKeyPath,
        vault_data_path: vaultDataPath,
        skills_checked: skillConfig.summary.total,
      },
    };
  } finally {
    await stopDaemon(daemon);
    await provider?.close?.();
    await llama?.close?.();
    await webhook?.close?.();
  }
}

if (isDirectRun(import.meta.url)) {
  runSecurityAuditSmoke()
    .then(printStandaloneResult)
    .catch((error) => {
      process.stderr.write(`${error.stack || error.message || String(error)}\n`);
      process.exitCode = 1;
    });
}
