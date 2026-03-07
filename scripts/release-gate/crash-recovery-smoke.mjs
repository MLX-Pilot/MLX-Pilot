#!/usr/bin/env node

import fs from "node:fs/promises";

import {
  assertOkJson,
  buildLegacySettings,
  configureCustomProvider,
  createHarness,
  isDirectRun,
  pickPort,
  printStandaloneResult,
  requestJson,
  seedReleaseGateSkills,
  startDaemon,
  startOpenAiMockServer,
  stopDaemon,
  waitForHealth,
  writeJson,
} from "./_lib.mjs";

export async function runCrashRecoverySmoke() {
  const harness = await createHarness("release-gate-crash");
  const daemonPort = await pickPort();
  let daemon;
  let provider;

  try {
    await seedReleaseGateSkills(harness.skillsDir);
    await writeJson(harness.settingsPath, buildLegacySettings(harness.workspace));
    provider = await startOpenAiMockServer({ delayMs: 4_000, finalPrefix: "crash-recovery" });

    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      name: "release-gate-crash",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "crash recovery daemon");

    const config = await configureCustomProvider(provider.baseUrl, harness.workspace);
    config.enabled_skills = ["mock-install", "summarize"];
    config.tool_policy.profile = "full";
    await assertOkJson(daemon.baseUrl, "POST", "/agent/config", config);
    await assertOkJson(daemon.baseUrl, "POST", "/agent/skills/config", {
      skill: "summarize",
      enabled: true,
      env: {
        OPENAI_API_KEY: "sk-crash-recovery",
      },
      config: {
        provider: "custom",
      },
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/upsert-account", {
      channel: "whatsapp",
      account_id: "ops",
      enabled: true,
      metadata: {
        owner: "crash-recovery",
      },
      routing_defaults: {
        target: "+5511999999999",
      },
      adapter_config: {},
      set_as_default: true,
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/login", {
      channel: "whatsapp",
      account_id: "ops",
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/upsert-account", {
      channel: "telegram",
      account_id: "bot",
      enabled: true,
      credentials: {
        token: "tg-crash-recovery",
      },
      metadata: {
        owner: "crash-recovery",
      },
      routing_defaults: {
        target: "@crash_recovery",
      },
      adapter_config: {},
      set_as_default: true,
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/login", {
      channel: "telegram",
      account_id: "bot",
    });

    const inFlightRun = requestJson(daemon.baseUrl, "POST", "/agent/run", {
      session_id: "crash-recovery-session",
      message: "Long running request during forced shutdown",
      provider: "custom",
      model_id: "mock-small",
      base_url: provider.baseUrl,
      workspace_root: harness.workspace,
    }, {
      expectedStatuses: [200, 500, 502],
      timeoutMs: 10_000,
    }).catch((error) => ({ ok: false, status: 0, error: error.message }));

    await new Promise((resolve) => setTimeout(resolve, 700));
    await stopDaemon(daemon, "SIGKILL");
    daemon = null;

    const inFlightResult = await inFlightRun;

    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      name: "release-gate-crash-restart",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "crash recovery restart daemon");

    const configAfterRestart = await assertOkJson(daemon.baseUrl, "GET", "/agent/config");
    const skillsAfterRestart = await assertOkJson(daemon.baseUrl, "GET", "/agent/skills/check");
    const channelsAfterRestart = await assertOkJson(daemon.baseUrl, "GET", "/agent/channels/status");
    const probeAfterRestart = await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/probe", {
      channel: "telegram",
      account_id: "bot",
    });
    const sendAfterRestart = await assertOkJson(daemon.baseUrl, "POST", "/agent/message/send", {
      channel: "telegram",
      account_id: "bot",
      target: "@crash_recovery",
      message: "idempotence after restart",
    });
    const sendAfterRestartAgain = await assertOkJson(daemon.baseUrl, "POST", "/agent/message/send", {
      channel: "telegram",
      account_id: "bot",
      target: "@crash_recovery",
      message: "idempotence after restart",
    });
    const settingsSaved = JSON.parse(await fs.readFile(harness.settingsPath, "utf8"));

    const blockers = [];
    if (configAfterRestart.tool_policy.profile !== "full") {
      blockers.push("Tool profile nao foi reidratado apos restart.");
    }
    if (!skillsAfterRestart.skills.some((entry) => entry.name === "summarize" && entry.enabled)) {
      blockers.push("Toggle de skill summarize foi perdido apos restart.");
    }
    if (!channelsAfterRestart.some((entry) => entry.id === "whatsapp" && entry.default_account_id === "ops")) {
      blockers.push("Default account de WhatsApp nao foi preservado.");
    }
    if (!channelsAfterRestart.some((entry) => entry.id === "telegram" && entry.default_account_id === "bot")) {
      blockers.push("Default account de Telegram nao foi preservado.");
    }
    if (probeAfterRestart[0]?.status !== "healthy") {
      blockers.push("Probe do canal Telegram falhou apos restart.");
    }
    if (sendAfterRestart.status !== "sent" || sendAfterRestartAgain.status !== "sent") {
      blockers.push("Envio idempotente apos restart nao permaneceu operacional.");
    }

    return {
      block: "crash_recovery_smoke",
      title: "Bloco 5 - Recuperacao de falha",
      status: blockers.length === 0 ? "pass" : "fail",
      summary: {
        in_flight_request_status: inFlightResult.status || 0,
        config_rehydrated: configAfterRestart.provider === "custom",
        channels_rehydrated: channelsAfterRestart.length,
        saved_schema_version: settingsSaved.schema_version,
      },
      validation: {
        config_after_restart: {
          provider: configAfterRestart.provider,
          model_id: configAfterRestart.model_id,
          tool_profile: configAfterRestart.tool_policy.profile,
          enabled_skills: configAfterRestart.enabled_skills,
        },
        channels_after_restart: channelsAfterRestart
          .filter((entry) => ["whatsapp", "telegram"].includes(entry.id))
          .map((entry) => ({
            id: entry.id,
            default_account_id: entry.default_account_id,
            accounts: entry.accounts.map((account) => ({
              account_id: account.account_id,
              session_status: account.session.status,
            })),
          })),
        idempotence: {
          first_message_id: sendAfterRestart.message_id,
          second_message_id: sendAfterRestartAgain.message_id,
          first_status: sendAfterRestart.status,
          second_status: sendAfterRestartAgain.status,
        },
      },
      blockers,
      commands: [
        "node scripts/release-gate/crash-recovery-smoke.mjs",
      ],
      artifacts: {
        settings_path: harness.settingsPath,
      },
    };
  } finally {
    await stopDaemon(daemon);
    await provider?.close?.();
  }
}

if (isDirectRun(import.meta.url)) {
  runCrashRecoverySmoke()
    .then(printStandaloneResult)
    .catch((error) => {
      process.stderr.write(`${error.stack || error.message || String(error)}\n`);
      process.exitCode = 1;
    });
}
