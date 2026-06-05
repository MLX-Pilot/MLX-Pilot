#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";

import {
  assertOkJson,
  createHarness,
  isDirectRun,
  pickPort,
  printStandaloneResult,
  startDaemon,
  stopDaemon,
  waitForHealth,
  writeJson,
} from "./_lib.mjs";

function legacyConfig(workspace) {
  return {
    bind_addr: "127.0.0.1:11435",
    agent: {
      provider: "custom",
      model_id: "mock-small",
      enabled_tools: ["read_file", "exec"],
      enabled_skills: ["mock-install", "summarize"],
      workspace_root: workspace,
      approval_mode: "auto",
      execution_mode: "full",
      skill_overrides: {
        summarize: {
          enabled: true,
          env_refs: {
            OPENAI_API_KEY: "vault://skills.summarize.openai_api_key",
          },
          config: {
            provider: "custom",
          },
        },
      },
    },
    compatibility: {
      plugins: {
        memory: {
          enabled: true,
          config: {
            backend: "local",
          },
        },
      },
      channels: {
        telegram: {
          default_account_id: "ops",
          accounts: {
            ops: {
              enabled: true,
              credentials_ref: "channels.telegram.ops.credentials",
              metadata: {
                owner: "legacy",
              },
              routing_defaults: {
                target: "@legacy_ops",
              },
              adapter_config: {
                mode: "bot",
              },
            },
          },
        },
        whatsapp: {
          default_account_id: "personal",
          accounts: {
            personal: {
              enabled: true,
              metadata: {
                owner: "legacy",
              },
              routing_defaults: {
                target: "+5511999999999",
              },
              adapter_config: {
                locale: "BR",
              },
            },
          },
        },
      },
    },
  };
}

export async function runMigrationUpgradeRollback() {
  const harness = await createHarness("release-gate-migration");
  const daemonPort = await pickPort();
  const backupDir = path.join(harness.root, "backup");
  const sessionStatePath = path.join(
    harness.root,
    "channel-sessions",
    "telegram",
    "ops",
    "session.json",
  );

  let daemon;
  const originalLegacy = legacyConfig(harness.workspace);

  try {
    await writeJson(harness.settingsPath, originalLegacy);
    await fs.mkdir(path.dirname(sessionStatePath), { recursive: true });
    await writeJson(sessionStatePath, {
      status: "connected",
      session_dir: path.dirname(sessionStatePath),
      connected_at_epoch_ms: Date.now(),
    });

    await fs.mkdir(path.join(backupDir, "channel-sessions", "telegram"), { recursive: true });
    await fs.cp(harness.settingsPath, path.join(backupDir, "settings.json"), { recursive: false });
    await fs.cp(path.dirname(sessionStatePath), path.join(backupDir, "channel-sessions", "telegram", "ops"), {
      recursive: true,
    });

    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      name: "release-gate-migration",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "migration daemon");

    const migratedConfig = await assertOkJson(daemon.baseUrl, "GET", "/agent/config");
    assert.deepEqual(migratedConfig.enabled_skills, ["mock-install", "summarize"]);

    const savedMigrated = await assertOkJson(daemon.baseUrl, "POST", "/agent/config", migratedConfig);
    const settingsAfterUpgrade = JSON.parse(await fs.readFile(harness.settingsPath, "utf8"));
    const effectivePolicy = await assertOkJson(
      daemon.baseUrl,
      "GET",
      "/agent/tools/effective-policy?agent_id=default",
    );
    const channelsStatus = await assertOkJson(daemon.baseUrl, "GET", "/agent/channels/status");
    const pluginsBefore = await assertOkJson(daemon.baseUrl, "GET", "/agent/plugins");
    const pluginDisabled = await assertOkJson(daemon.baseUrl, "POST", "/agent/plugins/disable", {
      plugin_id: "memory",
    });
    const pluginEnabled = await assertOkJson(daemon.baseUrl, "POST", "/agent/plugins/enable", {
      plugin_id: "memory",
    });
    const legacyChannelUpsert = await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/upsert", {
      channel_id: "whatsapp",
      enabled: true,
      alias: "legacy-whatsapp",
      config: {},
      metadata: {
        owner: "legacy-endpoint",
      },
    });

    const sessionAfterUpgrade = JSON.parse(await fs.readFile(sessionStatePath, "utf8"));

    await stopDaemon(daemon);
    daemon = null;

    await fs.cp(path.join(backupDir, "settings.json"), harness.settingsPath, { force: true });
    await fs.rm(path.join(harness.root, "channel-sessions"), { recursive: true, force: true });
    await fs.cp(path.join(backupDir, "channel-sessions"), path.join(harness.root, "channel-sessions"), {
      recursive: true,
    });

    const rawRestored = await fs.readFile(harness.settingsPath, "utf8");
    const rawOriginal = JSON.stringify(originalLegacy, null, 2) + "\n";

    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      name: "release-gate-migration-rollback",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "rollback daemon");

    const configAfterRollback = await assertOkJson(daemon.baseUrl, "GET", "/agent/config");
    const effectiveAfterRollback = await assertOkJson(
      daemon.baseUrl,
      "GET",
      "/agent/tools/effective-policy?agent_id=default",
    );
    const legacyRemove = await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/remove", {
      channel_id: "telegram",
    }).catch((error) => ({ error: error.message }));
    const channelsAfterRollback = await assertOkJson(daemon.baseUrl, "GET", "/agent/channels/status");

    const blockers = [];
    if (settingsAfterUpgrade.schema_version !== 2) {
      blockers.push("Upgrade nao persistiu schema_version=2.");
    }
    if (rawRestored !== rawOriginal) {
      blockers.push("Rollback nao restaurou exatamente o backup do settings.json.");
    }
    if (sessionAfterUpgrade.status !== "connected") {
      blockers.push("Sessao de canal nao permaneceu integra no upgrade.");
    }

    return {
      block: "migration_upgrade_rollback",
      title: "Bloco 3 - Upgrade e rollback de migracao",
      status: blockers.length === 0 ? "pass" : "fail",
      summary: {
        backup_created: true,
        upgraded_schema_version: settingsAfterUpgrade.schema_version,
        rollback_restored_exact_backup: rawRestored === rawOriginal,
        migrated_enabled_skills: savedMigrated.enabled_skills,
        migrated_tool_profile: settingsAfterUpgrade.agent.tool_policy.profile,
      },
      validation: {
        upgrade: {
          effective_policy_profile: effectivePolicy.profile,
          default_agent_allow_count: effectivePolicy.entries.filter((entry) => entry.allowed).length,
          channels: channelsStatus
            .filter((entry) => ["telegram", "whatsapp"].includes(entry.id))
            .map((entry) => ({
              id: entry.id,
              default_account_id: entry.default_account_id,
              accounts: entry.accounts.map((account) => account.account_id),
            })),
          plugins_before: pluginsBefore
            .filter((entry) => entry.id === "memory")
            .map((entry) => ({ id: entry.id, enabled: entry.enabled })),
          plugin_toggle: {
            disabled: pluginDisabled.enabled,
            reenabled: pluginEnabled.enabled,
          },
          legacy_endpoint_upsert: {
            id: legacyChannelUpsert.id,
            default_account_id: legacyChannelUpsert.default_account_id,
          },
        },
        rollback: {
          config_provider: configAfterRollback.provider,
          config_enabled_skills: configAfterRollback.enabled_skills,
          effective_policy_profile: effectiveAfterRollback.profile,
          legacy_endpoint_remove: legacyRemove,
          channels_remaining: channelsAfterRollback.map((entry) => entry.id),
        },
      },
      blockers,
      commands: [
        "node scripts/release-gate/migration-upgrade-rollback.mjs",
      ],
      artifacts: {
        settings_path: harness.settingsPath,
        backup_dir: backupDir,
        session_state_path: sessionStatePath,
      },
    };
  } finally {
    await stopDaemon(daemon);
  }
}

if (isDirectRun(import.meta.url)) {
  runMigrationUpgradeRollback()
    .then(printStandaloneResult)
    .catch((error) => {
      process.stderr.write(`${error.stack || error.message || String(error)}\n`);
      process.exitCode = 1;
    });
}
