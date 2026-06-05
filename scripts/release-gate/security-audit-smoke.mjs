#!/usr/bin/env node

import fs from "node:fs/promises";
import path from "node:path";

import {
  assertOkJson,
  buildLegacySettings,
  configureCustomProvider,
  containsAnySecret,
  createHarness,
  isDirectRun,
  pickPort,
  printStandaloneResult,
  requestJson,
  scanFilesForMarkers,
  seedReleaseGateSkills,
  startDaemon,
  startOpenAiMockServer,
  startWebhookSink,
  listFilesRecursive,
  statMode,
  stopDaemon,
  waitForHealth,
  writeJson,
} from "./_lib.mjs";

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
  let webhook;

  try {
    await seedReleaseGateSkills(harness.skillsDir);
    await writeJson(harness.settingsPath, buildLegacySettings(harness.workspace));
    provider = await startOpenAiMockServer({
      toolCallName: "exec",
      toolCallArguments: { command: "ls" },
      finalPrefix: "policy-allow",
    });
    webhook = await startWebhookSink();

    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      name: "release-gate-security",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "security audit daemon");

    const config = await configureCustomProvider(provider.baseUrl, harness.workspace);
    config.api_key = secrets[0];
    await assertOkJson(daemon.baseUrl, "POST", "/agent/config", config);

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
    const daemonLogRaw = await fs.readFile(daemon.logPath, "utf8");
    const channelAuditFiles = await scanFilesForMarkers(
      await listFilesRecursive(path.join(harness.root, "channel-audit")),
      secrets,
    );

    const leakedFiles = [
      ...(containsAnySecret(settingsRaw, secrets) ? [harness.settingsPath] : []),
      ...(containsAnySecret(daemonLogRaw, secrets) ? [daemon.logPath] : []),
      ...channelAuditFiles,
    ];

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

    await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/profile", { profile: "minimal" });
    const deniedSessionId = "security-policy-deny";
    const deniedRun = await requestJson(
      daemon.baseUrl,
      "POST",
      "/agent/run",
      {
        session_id: deniedSessionId,
        message: "Run ls using exec",
        provider: "custom",
        model_id: "mock-small",
        base_url: provider.baseUrl,
        workspace_root: harness.workspace,
      },
      {
        expectedStatuses: [200, 403],
      },
    );
    const deniedAudit = await assertOkJson(
      daemon.baseUrl,
      "GET",
      `/agent/audit?session_id=${deniedSessionId}&limit=50`,
    );

    await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/allow-deny", {
      scope: "agent",
      agent_id: "default",
      allow: ["exec"],
      deny: [],
      replace: false,
    });
    const allowedSessionId = "security-policy-allow";
    const allowedRun = await requestJson(
      daemon.baseUrl,
      "POST",
      "/agent/run",
      {
        session_id: allowedSessionId,
        message: "Run ls using exec",
        provider: "custom",
        model_id: "mock-small",
        base_url: provider.baseUrl,
        workspace_root: harness.workspace,
      },
      {
        expectedStatuses: [200, 403],
      },
    );
    const allowedAudit = await assertOkJson(
      daemon.baseUrl,
      "GET",
      `/agent/audit?session_id=${allowedSessionId}&limit=50`,
    );

    await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/profile", { profile: "full" });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/tools/allow-deny", {
      scope: "agent",
      agent_id: "default",
      allow: [],
      deny: ["exec"],
      replace: true,
    });
    const deniedOverrideSessionId = "security-policy-global-deny";
    const deniedByOverride = await requestJson(
      daemon.baseUrl,
      "POST",
      "/agent/run",
      {
        session_id: deniedOverrideSessionId,
        message: "Run ls using exec",
        provider: "custom",
        model_id: "mock-small",
        base_url: provider.baseUrl,
        workspace_root: harness.workspace,
      },
      {
        expectedStatuses: [200, 403],
      },
    );
    const deniedOverrideAudit = await assertOkJson(
      daemon.baseUrl,
      "GET",
      `/agent/audit?session_id=${deniedOverrideSessionId}&limit=50`,
    );

    const minimalDenied = deniedAudit.entries.some(
      (entry) => entry.event_type === "tool_denied" && entry.tool_name === "exec",
    );
    const allowExecuted = allowedAudit.entries.some(
      (entry) => entry.event_type === "tool_executed" && entry.tool_name === "exec",
    );
    const overrideDenied = deniedOverrideAudit.entries.some(
      (entry) => entry.event_type === "tool_denied" && entry.tool_name === "exec",
    );

    const findings = [];
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
        evidence: [JSON.stringify(summarizeOverride || {})],
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
    if (!minimalDenied || !allowExecuted || !overrideDenied) {
      findings.push({
        severity: "high",
        area: "policy",
        title: "Tool policy allow/deny enforcement diverged from expected behavior",
        evidence: [
          `minimal exec run status=${deniedRun.status} denied_event=${minimalDenied}`,
          `agent allow exec run status=${allowedRun.status} executed_event=${allowExecuted}`,
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
