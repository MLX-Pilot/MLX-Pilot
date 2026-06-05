#!/usr/bin/env node

import fs from "node:fs/promises";

import {
  assertOkJson,
  buildLegacySettings,
  configureCustomProvider,
  createHarness,
  ensureHealthy,
  isDirectRun,
  pickPort,
  printStandaloneResult,
  requestJson,
  seedReleaseGateSkills,
  startDaemon,
  startOpenAiMockServer,
  stopDaemon,
  summarizeLatencies,
  waitForHealth,
  writeJson,
} from "./_lib.mjs";

const CONCURRENCY_LEVELS = [10, 25, 50];

async function timedRequest(baseUrl, route, body, timeoutMs = 8_000) {
  const started = performance.now();
  try {
    const response = await requestJson(baseUrl, "POST", route, body, {
      expectedStatuses: [200, 400, 403, 408, 429, 500, 502, 503],
      timeoutMs,
    });
    return {
      ok: response.ok,
      status: response.status,
      latency_ms: Math.round(performance.now() - started),
      timeout: false,
    };
  } catch (error) {
    return {
      ok: false,
      status: 0,
      latency_ms: Math.round(performance.now() - started),
      timeout: String(error.message || error).includes("abort"),
      error: error.message,
    };
  }
}

async function runBatch({ baseUrl, route, makeBody, concurrency, requestsPerWorker }) {
  const totalRequests = concurrency * requestsPerWorker;
  const results = [];
  let nextIndex = 0;

  async function worker() {
    while (nextIndex < totalRequests) {
      const current = nextIndex;
      nextIndex += 1;
      results.push(await timedRequest(baseUrl, route, makeBody(current)));
    }
  }

  const started = Date.now();
  await Promise.all(Array.from({ length: concurrency }, () => worker()));
  const completedAt = Date.now();
  const latencies = results.map((entry) => entry.latency_ms);
  const statusCodes = {};
  let timeouts = 0;
  for (const entry of results) {
    const key = String(entry.status || (entry.timeout ? "timeout" : "exception"));
    statusCodes[key] = (statusCodes[key] || 0) + 1;
    if (entry.timeout) {
      timeouts += 1;
    }
  }

  return {
    concurrency,
    total_requests: totalRequests,
    duration_ms: completedAt - started,
    error_rate: Number(
      (results.filter((entry) => !entry.ok).length / Math.max(totalRequests, 1)).toFixed(4),
    ),
    timeouts,
    status_codes: statusCodes,
    ...summarizeLatencies(latencies),
  };
}

export async function runLoadSmoke() {
  const harness = await createHarness("release-gate-load");
  const daemonPort = await pickPort();
  let daemon;
  let provider;

  try {
    await seedReleaseGateSkills(harness.skillsDir);
    await writeJson(harness.settingsPath, buildLegacySettings(harness.workspace));
    provider = await startOpenAiMockServer({ delayMs: 20, finalPrefix: "load-smoke" });
    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      name: "release-gate-load",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "load smoke daemon");

    const config = await configureCustomProvider(provider.baseUrl, harness.workspace);
    await assertOkJson(daemon.baseUrl, "POST", "/agent/config", config);
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/upsert-account", {
      channel: "telegram",
      account_id: "load",
      enabled: true,
      credentials: { token: "tg-load-secret" },
      metadata: { owner: "release-gate" },
      routing_defaults: { target: "@load_target" },
      limits: {
        rate_limit_per_minute: 10_000,
        timeout_ms: 8_000,
        max_retries: 0,
        backoff_base_ms: 25,
        circuit_breaker_threshold: 100,
        circuit_breaker_open_ms: 100,
      },
      adapter_config: {},
      set_as_default: true,
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/login", {
      channel: "telegram",
      account_id: "load",
    });

    const endpointRuns = [];
    for (const concurrency of CONCURRENCY_LEVELS) {
      endpointRuns.push({
        endpoint: "/agent/run",
        metrics: await runBatch({
          baseUrl: daemon.baseUrl,
          route: "/agent/run",
          concurrency,
          requestsPerWorker: 2,
          makeBody(index) {
            return {
              session_id: `load-run-${concurrency}-${index}`,
              message: `load smoke request ${index}`,
              provider: "custom",
              model_id: "mock-small",
              base_url: provider.baseUrl,
              approval_mode: "auto",
              execution_mode: "full",
              workspace_root: harness.workspace,
            };
          },
        }),
      });

      endpointRuns.push({
        endpoint: "/agent/message/send",
        metrics: await runBatch({
          baseUrl: daemon.baseUrl,
          route: "/agent/message/send",
          concurrency,
          requestsPerWorker: 2,
          makeBody(index) {
            return {
              channel: "telegram",
              account_id: "load",
              target: `@load_target_${index}`,
              message: `load send ${index}`,
            };
          },
        }),
      });

      endpointRuns.push({
        endpoint: "/agent/channels/probe",
        metrics: await runBatch({
          baseUrl: daemon.baseUrl,
          route: "/agent/channels/probe",
          concurrency,
          requestsPerWorker: 2,
          makeBody() {
            return {
              channel: "telegram",
              account_id: "load",
            };
          },
        }),
      });

      await ensureHealthy(daemon);
    }

    const daemonLog = await fs.readFile(daemon.logPath, "utf8");
    const criticalPatterns = [/panic/i, /panicked at/i, /deadlock/i, /fatal/i];
    const criticalLogHit = criticalPatterns.find((pattern) => pattern.test(daemonLog))?.source || null;
    const crashDetected = daemon.proc.exitCode !== null;
    const erroringRuns = endpointRuns.filter((entry) => entry.metrics.error_rate > 0);

    const finalProbe = await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/probe", {
      channel: "telegram",
      account_id: "load",
    });

    return {
      block: "load_smoke",
      title: "Bloco 2 - Carga curta e concorrencia",
      status: !crashDetected && !criticalLogHit ? "pass" : "fail",
      summary: {
        endpoint_batches: endpointRuns.length,
        crash_detected: crashDetected,
        critical_log_hit: criticalLogHit,
        functional_health_after_load: finalProbe[0]?.status || "unknown",
      },
      blockers: [
        ...(crashDetected ? ["Daemon encerrou durante o teste de carga."] : []),
        ...(criticalLogHit ? [`Log do daemon contem marcador critico: ${criticalLogHit}.`] : []),
      ],
      observations: {
        batches_with_errors: erroringRuns.length,
        batches: endpointRuns,
      },
      commands: [
        "node scripts/release-gate/load-smoke.mjs",
      ],
      artifacts: {
        daemon_log: daemon.logPath,
        provider_requests: provider.requests.length,
      },
    };
  } finally {
    await stopDaemon(daemon);
    await provider?.close?.();
  }
}

if (isDirectRun(import.meta.url)) {
  runLoadSmoke()
    .then(printStandaloneResult)
    .catch((error) => {
      process.stderr.write(`${error.stack || error.message || String(error)}\n`);
      process.exitCode = 1;
    });
}
