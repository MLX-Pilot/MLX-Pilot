#!/usr/bin/env node

import { startBotFixture, startIrcFixture, startMatrixFixture } from "./channel-fixtures.mjs";

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

const expectedChannels = [
  "whatsapp", "telegram", "discord", "slack", "irc", "matrix", "googlechat", "feishu",
  "msteams", "mattermost", "synology-chat", "signal", "imessage", "bluebubbles", "nostr",
  "nextcloud-talk", "line", "zalo", "zalouser", "tlon",
];

function transportFixture(channel, peers) {
  const { id, protocol_family: family } = channel;
  const message = `release gate send ${id}`;
  const observed = server => server.requests.some(request => JSON.stringify(request.body).includes(message));
  switch (family) {
    case "bridge_http_v1":
      return { credentials: { token: `fixture-${id}`, base_url: peers.bridge.baseUrl },
        target: `target-${id}`, received: () => observed(peers.bridge) };
    case "webhook_http_v1":
      return { credentials: { webhook_url: peers.webhook.baseUrl + "/webhook" },
        target: `room-${id}`, received: () => observed(peers.webhook) };
    case "token_bot_v1":
      return { credentials: { token: `fixture-${id}` },
        target: { slack: "C123", discord: "212345678901234567", telegram: "12345" }[id],
        received: () => observed(peers.bot) };
    case "matrix_http_v1":
      return { credentials: { token: "fixture-matrix", homeserver: peers.matrix.baseUrl },
        target: "#room:example.org", received: () => observed(peers.matrix) };
    case "irc_tcp_v1":
      return { credentials: { server: "127.0.0.1", port: peers.irc.port, nick: "mlx", username: "mlx" },
        target: "#room", received: () => peers.irc.lines.includes(`PRIVMSG #room :${message}`) };
    case "native_runtime_v1":
      return { credentials: null, target: "+15550100000", adapter: { backend: "mock" } };
    default: throw new Error(`Missing transport fixture: ${id} (${family})`);
  }
}

async function callChannelOp(baseUrl, route, body) {
  const response = await requestJson(baseUrl, "POST", route, body, {
    expectedStatuses: [200, 400, 401, 403, 404, 408, 409, 429, 500, 502, 503],
  });
  if (!response.ok) {
    return { ok: false, status: response.status, canonical_error: canonicalChannelError(response.payload) };
  }
  const value = route.endsWith("/probe")
    ? response.payload.find(entry => entry.account_id === body.account_id)
    : response.payload;
  if (route.endsWith("/upsert-account")) {
    const account = value.accounts.find(entry => entry.account_id === body.account_id);
    return { ok: value.id === body.channel && account?.enabled === true,
      status: response.status, account_id: account?.account_id, enabled: account?.enabled };
  }
  const expected = { login: "connected", probe: "healthy", resolve: "resolved", send: "sent", logout: "logged_out" };
  const op = route.split("/").at(-1);
  const valid = value?.channel === body.channel && value.account_id === body.account_id
    && value.status === expected[op] && value.protocol_version === "v1"
    && (op !== "send" || Boolean(value.message_id)) && (op !== "resolve" || Boolean(value.resolved_target));
  return { ok: valid, status: response.status, result: value?.status, message_id: value?.message_id,
    resolved_target: value?.resolved_target };
}

export async function runChannelsRealValidation() {
  const harness = await createHarness("release-gate-channels");
  const daemonPort = await pickPort();
  let daemon;
  const peers = {};

  try {
    await seedReleaseGateSkills(harness.skillsDir);

    const seededSettings = buildLegacySettings(harness.workspace);
    await writeJson(harness.settingsPath, seededSettings);

    peers.bridge = await startBridgeMockServer();
    peers.webhook = await startWebhookSink();
    peers.bot = await startBotFixture("fixture-telegram");
    peers.matrix = await startMatrixFixture();
    peers.irc = await startIrcFixture();
    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      env: { MLX_CHANNEL_BOT_FIXTURE_URL: peers.bot.baseUrl },
      name: "release-gate-channels",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "channels validation daemon");
    await ensureHealthy(daemon);

    const compat = await assertOkJson(daemon.baseUrl, "GET", "/agent/compat/report");
    const channels = compat.channels;
    const missing = expectedChannels.filter(id => !channels.some(channel => channel.id === id));

    const results = [];
    for (const channel of channels) {
      const accountId = "release-gate";
      const family = channel.protocol_family;
      const { credentials, target, received, adapter = {} } = transportFixture(channel, peers);

      const evidence = {
        channel: channel.id,
        protocol_family: family,
        activation_mode: received ? "real_transport_loopback_peer" : "isolated_whatsapp_session_fixture",
        operations: {},
        recommendation:
          "A ativação em conta de produção requer credenciais do provedor ou pareamento de um aparelho.",
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
          adapter_config: adapter,
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

      if (received) {
        evidence.operations.delivery = { ok: received(), observed_at: "loopback_peer" };
      }

      const failedOperation = Object.entries(evidence.operations).find(([, value]) => !value.ok);
      results.push({
        channel: channel.id,
        family,
        status: failedOperation ? "fail" : "pass",
        failed_operation: failedOperation?.[0] || null,
        canonical_error: failedOperation?.[1]?.canonical_error || null,
        reason:
          failedOperation ? (failedOperation[1].canonical_error || "Resultado esperado não observado") : null,
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

    blockers.push(...missing.map(id => `Required channel missing from catalog: ${id}`));
    return {
      block: "channels_real_validation",
      title: "Bloco 1 - Canais e controle de contas",
      status: blockers.length === 0 ? "pass" : "fail",
      summary: {
        pending_channels_detected: channels.filter(entry => entry.requires_external_activation).length,
        expected_channels: expectedChannels.length,
        skipped_external: "Contas de produção e pareamento físico do WhatsApp",
        missing_channels: missing,
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
        bridge_requests: peers.bridge.requests.length,
        webhook_requests: peers.webhook.requests.length,
      },
    };
  } finally {
    await stopDaemon(daemon);
    await Promise.all(Object.values(peers).map(peer => peer.close()));
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
