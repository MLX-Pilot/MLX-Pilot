#!/usr/bin/env node

import path from "node:path";

import {
  assertOkJson,
  buildLegacySettings,
  canonicalChannelError,
  createHarness,
  ensureHealthy,
  isDirectRun,
  pickPort,
  printStandaloneResult,
  requestJson,
  seedReleaseGateSkills,
  startBridgeMockServer,
  startDaemon,
  startWebhookSink,
  stopDaemon,
  waitForHealth,
  writeJson,
} from "./_lib.mjs";

function channelCredentials(channel, family, bridgeBaseUrl, webhookUrl) {
  if (family === "bridge_http_v1") {
    return {
      token: `bridge-token-${channel}`,
      base_url: bridgeBaseUrl,
    };
  }
  return {
    webhook_url: webhookUrl,
  };
}

function safeTarget(channel, family) {
  if (family === "bridge_http_v1") {
    return `target-${channel}`;
  }
  return `room-${channel}`;
}

async function callChannelOp(baseUrl, route, body) {
  const response = await requestJson(baseUrl, "POST", route, body, {
    expectedStatuses: [200, 400, 401, 403, 404, 408, 409, 429, 500, 502, 503],
  });
  return {
    ok: response.ok,
    status: response.status,
    payload: response.payload,
    canonical_error: response.ok ? null : canonicalChannelError(response.payload),
  };
}

export async function runChannelsRealValidation() {
  const harness = await createHarness("release-gate-channels");
  const daemonPort = await pickPort();
  let daemon;
  let bridgeServer;
  let webhookServer;

  try {
    await seedReleaseGateSkills(harness.skillsDir);

    const seededSettings = buildLegacySettings(harness.workspace);
    await writeJson(harness.settingsPath, seededSettings);

    bridgeServer = await startBridgeMockServer();
    webhookServer = await startWebhookSink();
    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      name: "release-gate-channels",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "channels validation daemon");
    await ensureHealthy(daemon);

    const compat = await assertOkJson(daemon.baseUrl, "GET", "/agent/compat/report");
    const pendingChannels = compat.channels.filter((entry) => entry.requires_external_activation);

    const results = [];
    for (const channel of pendingChannels) {
      const accountId = "release-gate";
      const family = channel.protocol_family;
      const credentials = channelCredentials(
        channel.id,
        family,
        bridgeServer.baseUrl,
        `${webhookServer.baseUrl}/webhook`,
      );
      const target = safeTarget(channel.id, family);

      const evidence = {
        channel: channel.id,
        protocol_family: family,
        activation_mode: family === "bridge_http_v1" ? "local_bridge_mock" : "local_webhook_mock",
        operations: {},
        recommendation:
          "Provisionar bridge/webhook do provedor alvo e repetir o script com endpoints reais no ambiente final.",
      };

      evidence.operations.upsert_account = await callChannelOp(
        daemon.baseUrl,
        "/agent/channels/upsert-account",
        {
          channel: channel.id,
          account_id: accountId,
          enabled: true,
          credentials,
          metadata: { owner: "release-gate" },
          routing_defaults: { target },
          adapter_config: {},
          set_as_default: true,
        },
      );

      evidence.operations.login = await callChannelOp(daemon.baseUrl, "/agent/channels/login", {
        channel: channel.id,
        account_id: accountId,
      });

      evidence.operations.probe = await callChannelOp(daemon.baseUrl, "/agent/channels/probe", {
        channel: channel.id,
        account_id: accountId,
      });

      evidence.operations.resolve = await callChannelOp(
        daemon.baseUrl,
        "/agent/channels/resolve",
        {
          channel: channel.id,
          account_id: accountId,
          target,
        },
      );

      evidence.operations.send = await callChannelOp(daemon.baseUrl, "/agent/message/send", {
        channel: channel.id,
        account_id: accountId,
        target,
        message: `release gate send ${channel.id}`,
      });

      evidence.operations.logout = await callChannelOp(daemon.baseUrl, "/agent/channels/logout", {
        channel: channel.id,
        account_id: accountId,
      });

      const failedOperation = Object.entries(evidence.operations).find(([, value]) => !value.ok);
      results.push({
        channel: channel.id,
        family,
        status: failedOperation ? "fail" : "pass",
        failed_operation: failedOperation?.[0] || null,
        canonical_error: failedOperation?.[1]?.canonical_error || null,
        reason:
          failedOperation?.[1]?.payload?.details ||
          failedOperation?.[1]?.payload?.error ||
          null,
        evidence,
      });
    }

    const passCount = results.filter((entry) => entry.status === "pass").length;
    const failCount = results.length - passCount;
    const blockers = results
      .filter((entry) => entry.status !== "pass")
      .map(
        (entry) =>
          `Canal ${entry.channel} falhou em ${entry.failed_operation} (${entry.canonical_error || "provider_error"}).`,
      );

    return {
      block: "channels_real_validation",
      title: "Bloco 1 - Canais bridge/webhook",
      status: failCount === 0 ? "pass" : "fail",
      summary: {
        pending_channels_detected: pendingChannels.length,
        tested_channels: results.length,
        passed: passCount,
        failed: failCount,
      },
      blockers,
      commands: [
        "node scripts/release-gate/channels-real-validation.mjs",
      ],
      results,
      artifacts: {
        daemon_log: daemon.logPath,
        bridge_requests: bridgeServer.requests.length,
        webhook_requests: webhookServer.requests.length,
      },
    };
  } finally {
    await stopDaemon(daemon);
    await bridgeServer?.close?.();
    await webhookServer?.close?.();
  }
}

if (isDirectRun(import.meta.url)) {
  runChannelsRealValidation()
    .then(printStandaloneResult)
    .catch((error) => {
      process.stderr.write(`${error.stack || error.message || String(error)}\n`);
      process.exitCode = 1;
    });
}
