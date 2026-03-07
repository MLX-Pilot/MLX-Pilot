#!/usr/bin/env node

import {
  assertOkJson,
  buildLegacySettings,
  configureCustomProvider,
  createHarness,
  isDirectRun,
  pickPort,
  printStandaloneResult,
  seedReleaseGateSkills,
  startDaemon,
  startOpenAiMockServer,
  stopDaemon,
  waitForHealth,
  writeJson,
} from "./_lib.mjs";

export async function runReleaseDryRunChecklist() {
  const harness = await createHarness("release-gate-dry-run");
  const daemonPort = await pickPort();
  let daemon;
  let provider;

  try {
    await seedReleaseGateSkills(harness.skillsDir);
    await writeJson(harness.settingsPath, buildLegacySettings(harness.workspace));
    provider = await startOpenAiMockServer({ finalPrefix: "dry-run" });

    daemon = startDaemon({
      workspace: harness.workspace,
      settingsPath: harness.settingsPath,
      port: daemonPort,
      name: "release-gate-dry-run",
    });
    await waitForHealth(daemon.baseUrl, daemon.proc, "dry run daemon");

    const config = await configureCustomProvider(provider.baseUrl, harness.workspace);
    config.enabled_skills = ["summarize"];
    await assertOkJson(daemon.baseUrl, "POST", "/agent/config", config);
    await assertOkJson(daemon.baseUrl, "POST", "/agent/skills/config", {
      skill: "summarize",
      enabled: true,
      env: {
        OPENAI_API_KEY: "sk-dry-run",
      },
      config: {
        provider: "custom",
      },
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/upsert-account", {
      channel: "telegram",
      account_id: "bot",
      enabled: true,
      credentials: {
        token: "tg-dry-run",
      },
      metadata: {
        owner: "dry-run",
      },
      routing_defaults: {
        target: "@dry_run",
      },
      adapter_config: {},
      set_as_default: true,
    });
    await assertOkJson(daemon.baseUrl, "POST", "/agent/channels/login", {
      channel: "telegram",
      account_id: "bot",
    });
    const send = await assertOkJson(daemon.baseUrl, "POST", "/agent/message/send", {
      channel: "telegram",
      account_id: "bot",
      target: "@dry_run",
      message: "release dry run send",
    });
    const compatReport = await assertOkJson(daemon.baseUrl, "GET", "/agent/compat/report");

    const checklist = [
      {
        item: "Instalar artefato/binario em ambiente limpo ou VM isolada",
        status: "manual_required",
      },
      {
        item: "Abrir app desktop e confirmar health visual/daemon online",
        status: "manual_required",
      },
      {
        item: "Configurar uma skill minima pelo onboarding",
        status: "validated_in_simulation",
      },
      {
        item: "Conectar WhatsApp ou Telegram; neste ambiente foi usado Telegram mock/token local",
        status: "validated_in_simulation",
      },
      {
        item: "Enviar mensagem teste e conferir compat/report atualizado",
        status: "validated_in_simulation",
      },
    ];

    return {
      block: "release_dry_run",
      title: "Bloco 6 - Release dry-run",
      status: "pass",
      summary: {
        mode: "simulated_clean_env_with_guided_checklist",
        clean_settings_path: harness.settingsPath,
        compat_coverage_percent: compatReport.summary.coverage_percent,
        send_status: send.status,
      },
      checklist,
      notes: [
        "Nao houve acesso a uma maquina limpa dedicada neste workspace; o dry-run foi executado em diretorio temporario isolado.",
        "O fluxo minimo foi validado com provider local OpenAI-compatible mock e Telegram token-bot declarado como mock equivalente.",
      ],
      commands: [
        "node scripts/release-gate/release-dry-run-checklist.mjs",
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
  runReleaseDryRunChecklist()
    .then(printStandaloneResult)
    .catch((error) => {
      process.stderr.write(`${error.stack || error.message || String(error)}\n`);
      process.exitCode = 1;
    });
}
