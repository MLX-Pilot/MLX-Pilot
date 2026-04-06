use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::header::CONTENT_TYPE;
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use uuid::Uuid;

const DEFAULT_OUTPUT: &str = "AI_FOR_MLX_PILOT/REPORTS/06_benchmark_report.md";
const DEFAULT_ITERATIONS: usize = 10;
const PROVIDER_PRIORITY: [&str; 5] = ["deepseek", "openrouter", "groq", "anthropic", "openai"];
const SAFE_MODE_PREFIX: &str = "BENCHMARK SAFE MODE: proibido usar canais externos (whatsapp/telegram/discord), enviar mensagens, executar comandos, abrir browser, web_search, web_fetch ou qualquer acao fora de leitura local. ";

#[derive(Debug, Clone)]
struct CliOptions {
    daemon_url: String,
    iterations: usize,
    output_path: PathBuf,
    timeout_ms: u64,
    skip_remote: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
enum AgentKind {
    OpenClaw,
    NanoBot,
    RustAgent,
}

impl AgentKind {
    fn label(self) -> &'static str {
        match self {
            Self::OpenClaw => "OpenClaw",
            Self::NanoBot => "NanoBot",
            Self::RustAgent => "Rust Agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
enum BenchMode {
    LocalSmall,
    Remote,
}

impl BenchMode {
    fn label(self) -> &'static str {
        match self {
            Self::LocalSmall => "local_small",
            Self::Remote => "remote",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
enum ScenarioKind {
    BaselineNoTool,
    ToolReadFileSimple,
    ToolReadFileFinal,
}

impl ScenarioKind {
    fn label(self) -> &'static str {
        match self {
            Self::BaselineNoTool => "baseline_no_tool",
            Self::ToolReadFileSimple => "tool_read_file_simple",
            Self::ToolReadFileFinal => "tool_read_file_plus_final",
        }
    }
}

#[derive(Debug, Clone)]
struct ScenarioResultGroup {
    mode: BenchMode,
    scenario: ScenarioKind,
    agent: AgentKind,
    runs: Vec<RunRecord>,
}

#[derive(Debug, Clone)]
struct RunRecord {
    run_index: usize,
    success: bool,
    validated: bool,
    wall_ms: Option<u64>,
    reported_ms: Option<u64>,
    rss_before_kb: Option<u64>,
    rss_after_kb: Option<u64>,
    iterations: Option<usize>,
    tool_calls: Option<usize>,
    total_tokens: Option<usize>,
    provider: Option<String>,
    model: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct GroupSummary {
    attempts: usize,
    successes: usize,
    validated: usize,
    avg_wall_ms: Option<f64>,
    avg_reported_ms: Option<f64>,
    avg_rss_delta_kb: Option<f64>,
    avg_iterations: Option<f64>,
    avg_tool_calls: Option<f64>,
    avg_total_tokens: Option<f64>,
}

#[derive(Debug, Clone)]
struct ModelChoice {
    mode: BenchMode,
    agent: AgentKind,
    provider: String,
    model: String,
    notes: String,
}

#[derive(Debug, Clone)]
struct BenchmarkOutcome {
    daemon_url: String,
    generated_at: DateTime<Utc>,
    iterations: usize,
    model_choices: Vec<ModelChoice>,
    groups: Vec<ScenarioResultGroup>,
    notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct LocalSelection {
    openclaw_model_id: String,
    nanobot_model_id: String,
    rust_provider: String,
    rust_model: String,
}

#[derive(Debug, Clone)]
struct RemoteSelection {
    provider: String,
    openclaw_model_reference: String,
    nanobot_model_reference: String,
    rust_model: String,
    api_key: String,
    base_url: Option<String>,
}

#[derive(Debug, Clone)]
struct FixturePaths {
    openclaw: PathBuf,
    nanobot: PathBuf,
    rust: PathBuf,
}

#[derive(Debug, Clone)]
struct RuntimePids {
    openclaw: Option<u32>,
    nanobot: Option<u32>,
    rust_daemon: Option<u32>,
}

#[derive(Debug, Clone)]
struct BenchmarkContext {
    openclaw_models: OpenClawModelsResponse,
    nanobot_models: OpenClawModelsResponse,
    openclaw_status: OpenClawStatusResponse,
    nanobot_status: NanoBotStatusResponse,
    agent_providers: Vec<AgentProviderInfo>,
    agent_config: AgentConfigResponse,
    env_values: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct RestorePlan {
    openclaw_current: OpenClawCurrentModel,
    nanobot_current: OpenClawCurrentModel,
    openclaw_locals: Vec<OpenClawLocalModel>,
    nanobot_locals: Vec<OpenClawLocalModel>,
}

#[derive(Debug, Clone)]
enum RustModeConfig {
    Local {
        provider: String,
        model_id: String,
    },
    Remote {
        provider: String,
        model_id: String,
        api_key: String,
        base_url: Option<String>,
    },
}

#[derive(Debug)]
struct RawCallResult {
    reply: String,
    reported_ms: Option<u64>,
    iterations: Option<usize>,
    tool_calls: Option<usize>,
    total_tokens: Option<usize>,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct OpenClawStatusResponse {
    state_dir: String,
}

#[derive(Debug, Deserialize, Clone)]
struct NanoBotStatusResponse {
    workspace_path: String,
}

#[derive(Debug, Deserialize, Clone)]
struct OpenClawModelsResponse {
    current: OpenClawCurrentModel,
    cloud_models: Vec<OpenClawCloudModel>,
    local_models: Vec<OpenClawLocalModel>,
}

#[derive(Debug, Deserialize, Clone)]
struct OpenClawCurrentModel {
    source: String,
    reference: String,
    provider: String,
    model: String,
}

#[derive(Debug, Deserialize, Clone)]
struct OpenClawCloudModel {
    reference: String,
    provider: String,
    model: String,
}

#[derive(Debug, Deserialize, Clone)]
struct OpenClawLocalModel {
    id: String,
    name: String,
    path: String,
}

#[derive(Debug, Deserialize, Clone)]
struct AgentProviderInfo {
    id: String,
    kind: String,
    #[serde(default)]
    supports_tool_calling: bool,
    models: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct AgentConfigResponse {
    provider: String,
    model_id: String,
    api_key: String,
    base_url: String,
    #[serde(default)]
    custom_headers: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct EnvironmentResponse {
    variables: Vec<EnvironmentVariable>,
}

#[derive(Debug, Deserialize)]
struct EnvironmentVariable {
    key: String,
    #[serde(default)]
    value: String,
    present: bool,
}

#[derive(Debug, Deserialize)]
struct RuntimeStateResponse {
    pid: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OpenClawChatResponse {
    reply: String,
    duration_ms: Option<u64>,
    provider: Option<String>,
    model: Option<String>,
    usage: Option<UsageResponse>,
}

#[derive(Debug, Deserialize)]
struct NanoBotChatResponse {
    reply: String,
    duration_ms: Option<u64>,
    provider: Option<String>,
    model: Option<String>,
    usage: Option<UsageResponse>,
}

#[derive(Debug, Deserialize)]
struct UsageResponse {
    total: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AgentRunResponse {
    provider: String,
    model_id: String,
    final_response: String,
    latency_ms: u64,
    iterations: usize,
    tool_calls_made: usize,
    total_tokens: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = parse_cli_options()?;
    let http_timeout_ms = opts.timeout_ms.saturating_add(10_000);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(http_timeout_ms))
        .build()
        .context("falha ao construir cliente HTTP")?;

    println!(
        "[bench_agent] iniciando benchmark em {} (iteracoes por cenario: {})",
        opts.daemon_url, opts.iterations
    );

    let mut ctx = fetch_benchmark_context(&client, &opts.daemon_url).await?;
    let restore_plan = RestorePlan {
        openclaw_current: ctx.openclaw_models.current.clone(),
        nanobot_current: ctx.nanobot_models.current.clone(),
        openclaw_locals: ctx.openclaw_models.local_models.clone(),
        nanobot_locals: ctx.nanobot_models.local_models.clone(),
    };

    let benchmark_result = run_benchmarks(&client, &opts, &mut ctx).await;

    if let Err(error) = restore_plan.apply(&client, &opts.daemon_url).await {
        eprintln!("[bench_agent] aviso: falha ao restaurar modelos originais: {error:#}");
    }

    let outcome = benchmark_result?;
    let report = render_report(&outcome);

    if let Some(parent) = opts.output_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("falha ao criar diretorio de relatorio {}", parent.display())
        })?;
    }
    fs::write(&opts.output_path, report)
        .with_context(|| format!("falha ao gravar relatorio {}", opts.output_path.display()))?;

    println!(
        "[bench_agent] relatorio gerado em {}",
        opts.output_path.display()
    );
    Ok(())
}

async fn run_benchmarks(
    client: &reqwest::Client,
    opts: &CliOptions,
    ctx: &mut BenchmarkContext,
) -> Result<BenchmarkOutcome> {
    assert_daemon_health(client, &opts.daemon_url).await?;

    let local_selection = select_local_models(ctx)?;
    let remote_selection = if opts.skip_remote {
        None
    } else {
        select_remote_models(ctx)
    };

    let runtime_pids = prepare_runtime_pids(client, &opts.daemon_url).await;
    let fixture_token = format!("BENCH_TOKEN_{}", Uuid::new_v4());
    let fixtures = prepare_fixtures(ctx, &fixture_token)?;

    let mut notes = Vec::new();
    if remote_selection.is_none() && !opts.skip_remote {
        notes.push(
            "Benchmark remoto foi pulado: nenhum provider remoto comum com credencial ativa foi encontrado."
                .to_string(),
        );
    }
    if runtime_pids.openclaw.is_none() {
        notes.push(
            "Nao foi possivel capturar PID do runtime OpenClaw para amostra de memoria."
                .to_string(),
        );
    }
    if runtime_pids.nanobot.is_none() {
        notes.push(
            "Nao foi possivel capturar PID do runtime NanoBot para amostra de memoria.".to_string(),
        );
    }
    if runtime_pids.rust_daemon.is_none() {
        notes.push(
            "Nao foi possivel capturar PID do daemon para amostra de memoria do Rust Agent."
                .to_string(),
        );
    }

    configure_openclaw_local(client, &opts.daemon_url, &local_selection.openclaw_model_id)
        .await
        .context("falha ao configurar OpenClaw em modelo local para benchmark")?;
    configure_nanobot_local(client, &opts.daemon_url, &local_selection.nanobot_model_id)
        .await
        .context("falha ao configurar NanoBot em modelo local para benchmark")?;

    println!(
        "[bench_agent] local_small: OpenClaw={}, NanoBot={}, Rust={}::{}",
        local_selection.openclaw_model_id,
        local_selection.nanobot_model_id,
        local_selection.rust_provider,
        local_selection.rust_model
    );

    let mut groups = Vec::new();
    let mut model_choices = vec![
        ModelChoice {
            mode: BenchMode::LocalSmall,
            agent: AgentKind::OpenClaw,
            provider: "mlx-local".to_string(),
            model: local_selection.openclaw_model_id.clone(),
            notes: "Selecionado automaticamente pelo menor porte detectado.".to_string(),
        },
        ModelChoice {
            mode: BenchMode::LocalSmall,
            agent: AgentKind::NanoBot,
            provider: "mlx-local".to_string(),
            model: local_selection.nanobot_model_id.clone(),
            notes: "Selecionado automaticamente pelo menor porte detectado.".to_string(),
        },
        ModelChoice {
            mode: BenchMode::LocalSmall,
            agent: AgentKind::RustAgent,
            provider: local_selection.rust_provider.clone(),
            model: local_selection.rust_model.clone(),
            notes: "Selecionado automaticamente entre providers locais registrados.".to_string(),
        },
    ];

    for scenario in [
        ScenarioKind::BaselineNoTool,
        ScenarioKind::ToolReadFileSimple,
        ScenarioKind::ToolReadFileFinal,
    ] {
        for agent in [
            AgentKind::OpenClaw,
            AgentKind::NanoBot,
            AgentKind::RustAgent,
        ] {
            let rust_mode = RustModeConfig::Local {
                provider: local_selection.rust_provider.clone(),
                model_id: local_selection.rust_model.clone(),
            };
            let group = run_group(
                client,
                opts,
                BenchMode::LocalSmall,
                scenario,
                agent,
                &fixtures,
                &fixture_token,
                &runtime_pids,
                Some(&rust_mode),
            )
            .await;
            groups.push(group);
        }
    }

    if let Some(remote) = remote_selection.clone() {
        println!(
            "[bench_agent] remote: provider={}, OpenClaw={}, NanoBot={}, Rust model={}",
            remote.provider,
            remote.openclaw_model_reference,
            remote.nanobot_model_reference,
            remote.rust_model
        );

        configure_openclaw_remote(client, &opts.daemon_url, &remote.openclaw_model_reference)
            .await
            .context("falha ao configurar OpenClaw em modelo remoto para benchmark")?;
        configure_nanobot_remote(client, &opts.daemon_url, &remote.nanobot_model_reference)
            .await
            .context("falha ao configurar NanoBot em modelo remoto para benchmark")?;

        model_choices.push(ModelChoice {
            mode: BenchMode::Remote,
            agent: AgentKind::OpenClaw,
            provider: remote.provider.clone(),
            model: remote.openclaw_model_reference.clone(),
            notes: "Selecionado por interseccao provider/modelo remoto entre os 3 agentes."
                .to_string(),
        });
        model_choices.push(ModelChoice {
            mode: BenchMode::Remote,
            agent: AgentKind::NanoBot,
            provider: remote.provider.clone(),
            model: remote.nanobot_model_reference.clone(),
            notes: "Selecionado por interseccao provider/modelo remoto entre os 3 agentes."
                .to_string(),
        });
        model_choices.push(ModelChoice {
            mode: BenchMode::Remote,
            agent: AgentKind::RustAgent,
            provider: remote.provider.clone(),
            model: remote.rust_model.clone(),
            notes: "Selecionado automaticamente no registry do Rust Agent.".to_string(),
        });

        for agent in [
            AgentKind::OpenClaw,
            AgentKind::NanoBot,
            AgentKind::RustAgent,
        ] {
            let rust_mode = RustModeConfig::Remote {
                provider: remote.provider.clone(),
                model_id: remote.rust_model.clone(),
                api_key: remote.api_key.clone(),
                base_url: remote.base_url.clone(),
            };

            let group = run_group(
                client,
                opts,
                BenchMode::Remote,
                ScenarioKind::ToolReadFileFinal,
                agent,
                &fixtures,
                &fixture_token,
                &runtime_pids,
                Some(&rust_mode),
            )
            .await;
            groups.push(group);
        }
    }

    Ok(BenchmarkOutcome {
        daemon_url: opts.daemon_url.clone(),
        generated_at: Utc::now(),
        iterations: opts.iterations,
        model_choices,
        groups,
        notes,
    })
}

async fn run_group(
    client: &reqwest::Client,
    opts: &CliOptions,
    mode: BenchMode,
    scenario: ScenarioKind,
    agent: AgentKind,
    fixtures: &FixturePaths,
    token: &str,
    pids: &RuntimePids,
    rust_mode: Option<&RustModeConfig>,
) -> ScenarioResultGroup {
    println!(
        "[bench_agent] executando {} / {} / {}",
        mode.label(),
        scenario.label(),
        agent.label()
    );

    let mut runs = Vec::with_capacity(opts.iterations);
    for run_index in 1..=opts.iterations {
        let memory_pid = match agent {
            AgentKind::OpenClaw => pids.openclaw,
            AgentKind::NanoBot => pids.nanobot,
            AgentKind::RustAgent => pids.rust_daemon,
        };

        let rss_before = memory_pid.and_then(read_rss_kb);
        let started = Instant::now();

        let prompt = match agent {
            AgentKind::OpenClaw => build_prompt(scenario, &fixtures.openclaw, token),
            AgentKind::NanoBot => build_prompt(scenario, &fixtures.nanobot, token),
            AgentKind::RustAgent => build_prompt(scenario, &fixtures.rust, token),
        };

        let raw_result = match agent {
            AgentKind::OpenClaw => {
                call_openclaw(client, &opts.daemon_url, &prompt, opts.timeout_ms).await
            }
            AgentKind::NanoBot => {
                call_nanobot(client, &opts.daemon_url, &prompt, opts.timeout_ms).await
            }
            AgentKind::RustAgent => match rust_mode {
                Some(mode_cfg) => {
                    call_rust_agent(client, &opts.daemon_url, &prompt, mode_cfg, &fixtures.rust)
                        .await
                }
                None => Err(anyhow!("configuracao do Rust Agent ausente")),
            },
        };

        let wall_ms = started.elapsed().as_millis() as u64;
        let rss_after = memory_pid.and_then(read_rss_kb);

        let record = match raw_result {
            Ok(raw) => {
                let validated = validate_reply(scenario, &raw.reply, token);
                RunRecord {
                    run_index,
                    success: true,
                    validated,
                    wall_ms: Some(wall_ms),
                    reported_ms: raw.reported_ms,
                    rss_before_kb: rss_before,
                    rss_after_kb: rss_after,
                    iterations: raw.iterations,
                    tool_calls: raw.tool_calls,
                    total_tokens: raw.total_tokens,
                    provider: raw.provider,
                    model: raw.model,
                    error: if validated {
                        None
                    } else {
                        Some("resposta nao validou o criterio do cenario".to_string())
                    },
                }
            }
            Err(error) => RunRecord {
                run_index,
                success: false,
                validated: false,
                wall_ms: Some(wall_ms),
                reported_ms: None,
                rss_before_kb: rss_before,
                rss_after_kb: rss_after,
                iterations: None,
                tool_calls: None,
                total_tokens: None,
                provider: None,
                model: None,
                error: Some(format!("{error:#}")),
            },
        };

        runs.push(record);
        sleep(Duration::from_millis(120)).await;
    }

    ScenarioResultGroup {
        mode,
        scenario,
        agent,
        runs,
    }
}

fn build_prompt(scenario: ScenarioKind, fixture_path: &Path, token: &str) -> String {
    let path = fixture_path.display();
    match scenario {
        ScenarioKind::BaselineNoTool => {
            format!("{SAFE_MODE_PREFIX}Nao use nenhuma tool. Responda exatamente com BENCH_OK.")
        }
        ScenarioKind::ToolReadFileSimple => format!(
            "{SAFE_MODE_PREFIX}Use exclusivamente a tool read_file para abrir o arquivo {path}. \
O arquivo contem a linha BENCH_AGENT_TOKEN=<valor>. \
Responda somente com o valor exato do token ({token}), sem explicacoes."
        ),
        ScenarioKind::ToolReadFileFinal => format!(
            "{SAFE_MODE_PREFIX}Use somente read_file para ler o arquivo {path}. \
Depois responda em duas linhas curtas: \
TOKEN: {token} \
RESUMO: uma frase sobre o conteudo do arquivo."
        ),
    }
}

fn validate_reply(scenario: ScenarioKind, reply: &str, token: &str) -> bool {
    let normalized = reply.trim().to_ascii_uppercase();
    match scenario {
        ScenarioKind::BaselineNoTool => normalized.contains("BENCH_OK"),
        ScenarioKind::ToolReadFileSimple | ScenarioKind::ToolReadFileFinal => reply.contains(token),
    }
}

fn summarize_group(group: &ScenarioResultGroup) -> GroupSummary {
    let attempts = group.runs.len();
    let successes = group.runs.iter().filter(|r| r.success).count();
    let validated = group.runs.iter().filter(|r| r.validated).count();
    let valid_runs = group
        .runs
        .iter()
        .filter(|r| r.validated)
        .collect::<Vec<_>>();

    GroupSummary {
        attempts,
        successes,
        validated,
        avg_wall_ms: mean_u64(valid_runs.iter().filter_map(|r| r.wall_ms)),
        avg_reported_ms: mean_u64(valid_runs.iter().filter_map(|r| r.reported_ms)),
        avg_rss_delta_kb: mean_i64(valid_runs.iter().filter_map(|r| {
            let before = r.rss_before_kb?;
            let after = r.rss_after_kb?;
            Some(after as i64 - before as i64)
        })),
        avg_iterations: mean_usize(valid_runs.iter().filter_map(|r| r.iterations)),
        avg_tool_calls: mean_usize(valid_runs.iter().filter_map(|r| r.tool_calls)),
        avg_total_tokens: mean_usize(valid_runs.iter().filter_map(|r| r.total_tokens)),
    }
}

fn render_report(outcome: &BenchmarkOutcome) -> String {
    let mut md = String::new();
    md.push_str("# 06 Benchmark Report\n\n");
    md.push_str("## Escopo\n");
    md.push_str(
        "- Agentes: OpenClaw, NanoBot e Rust Agent (mlx-ollama-pilot).\n\
- Cenarios: baseline sem tool, read_file simples, read_file + resposta final.\n\
- Modo local: modelo local pequeno selecionado automaticamente.\n\
- Modo remoto: provider comum com credencial valida (quando disponivel).\n\
- Iteracoes: 10 por cenario/agent (ou valor informado por CLI).\n\n",
    );

    md.push_str("## Metadados\n");
    md.push_str(&format!(
        "- Gerado em: {}\n- Daemon: `{}`\n- Iteracoes por cenario: `{}`\n\n",
        outcome.generated_at.to_rfc3339(),
        outcome.daemon_url,
        outcome.iterations
    ));

    md.push_str("## Selecao de Modelos\n");
    md.push_str("| Modo | Agente | Provider | Modelo | Observacao |\n");
    md.push_str("| --- | --- | --- | --- | --- |\n");
    for choice in &outcome.model_choices {
        md.push_str(&format!(
            "| {} | {} | `{}` | `{}` | {} |\n",
            choice.mode.label(),
            choice.agent.label(),
            choice.provider,
            choice.model,
            choice.notes
        ));
    }
    md.push('\n');

    if !outcome.notes.is_empty() {
        md.push_str("## Notas de Execucao\n");
        for note in &outcome.notes {
            md.push_str(&format!("- {note}\n"));
        }
        md.push('\n');
    }

    md.push_str("## Resultados (medias)\n");
    md.push_str("| Modo | Cenario | Agente | Validadas/Total | Avg wall (ms) | Avg API (ms) | Avg RSS delta (KB) | Avg loops | Avg tool_calls | Avg tokens |\n");
    md.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");

    let mut groups = outcome.groups.clone();
    groups.sort_by_key(|g| (g.mode, g.scenario, g.agent));
    for group in &groups {
        let summary = summarize_group(group);
        md.push_str(&format!(
            "| {} | {} | {} | {}/{} | {} | {} | {} | {} | {} | {} |\n",
            group.mode.label(),
            group.scenario.label(),
            group.agent.label(),
            summary.validated,
            summary.attempts,
            format_opt(summary.avg_wall_ms, 1),
            format_opt(summary.avg_reported_ms, 1),
            format_opt(summary.avg_rss_delta_kb, 1),
            format_opt(summary.avg_iterations, 2),
            format_opt(summary.avg_tool_calls, 2),
            format_opt(summary.avg_total_tokens, 1),
        ));
    }
    md.push('\n');

    md.push_str("## Overhead do Loop (modo local_small)\n");
    md.push_str("| Agente | baseline (ms) | read_file (ms) | read_file+final (ms) | Overhead loop (read_file-baseline) | Overhead finalizacao (final-read_file) |\n");
    md.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for agent in [
        AgentKind::OpenClaw,
        AgentKind::NanoBot,
        AgentKind::RustAgent,
    ] {
        let baseline = summary_avg_wall(
            &groups,
            BenchMode::LocalSmall,
            ScenarioKind::BaselineNoTool,
            agent,
        );
        let read_file = summary_avg_wall(
            &groups,
            BenchMode::LocalSmall,
            ScenarioKind::ToolReadFileSimple,
            agent,
        );
        let final_resp = summary_avg_wall(
            &groups,
            BenchMode::LocalSmall,
            ScenarioKind::ToolReadFileFinal,
            agent,
        );
        let loop_overhead = diff_opt(read_file, baseline);
        let final_overhead = diff_opt(final_resp, read_file);
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            agent.label(),
            format_opt(baseline, 1),
            format_opt(read_file, 1),
            format_opt(final_resp, 1),
            format_opt(loop_overhead, 1),
            format_opt(final_overhead, 1),
        ));
    }
    md.push('\n');

    md.push_str("## Latencia Local x Remoto (cenario read_file+final)\n");
    md.push_str("| Agente | Local (ms) | Remoto (ms) | Delta remoto-local (ms) |\n");
    md.push_str("| --- | --- | --- | --- |\n");
    for agent in [
        AgentKind::OpenClaw,
        AgentKind::NanoBot,
        AgentKind::RustAgent,
    ] {
        let local = summary_avg_wall(
            &groups,
            BenchMode::LocalSmall,
            ScenarioKind::ToolReadFileFinal,
            agent,
        );
        let remote = summary_avg_wall(
            &groups,
            BenchMode::Remote,
            ScenarioKind::ToolReadFileFinal,
            agent,
        );
        md.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            agent.label(),
            format_opt(local, 1),
            format_opt(remote, 1),
            format_opt(diff_opt(remote, local), 1),
        ));
    }
    md.push('\n');

    let rust_local_final = summary_avg_wall(
        &groups,
        BenchMode::LocalSmall,
        ScenarioKind::ToolReadFileFinal,
        AgentKind::RustAgent,
    );
    let openclaw_local_final = summary_avg_wall(
        &groups,
        BenchMode::LocalSmall,
        ScenarioKind::ToolReadFileFinal,
        AgentKind::OpenClaw,
    );
    let nanobot_local_final = summary_avg_wall(
        &groups,
        BenchMode::LocalSmall,
        ScenarioKind::ToolReadFileFinal,
        AgentKind::NanoBot,
    );

    md.push_str("## Analise Critica\n");
    if let Some(rust_ms) = rust_local_final {
        let mut competitors = Vec::new();
        if let Some(v) = openclaw_local_final {
            competitors.push(v);
        }
        if let Some(v) = nanobot_local_final {
            competitors.push(v);
        }
        if let Some(fastest) = competitors
            .into_iter()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        {
            if rust_ms > fastest {
                let gap = rust_ms - fastest;
                let pct = (gap / fastest) * 100.0;
                md.push_str(&format!(
                    "- O Rust Agent ficou **mais lento** no cenario local `read_file+final`: {:.1} ms vs {:.1} ms (gap {:.1} ms / {:.1}%).\n",
                    rust_ms, fastest, gap, pct
                ));
                md.push_str("- Gargalos provaveis com base nos dados coletados:\n");

                let rust_overhead = diff_opt(
                    summary_avg_wall(
                        &groups,
                        BenchMode::LocalSmall,
                        ScenarioKind::ToolReadFileSimple,
                        AgentKind::RustAgent,
                    ),
                    summary_avg_wall(
                        &groups,
                        BenchMode::LocalSmall,
                        ScenarioKind::BaselineNoTool,
                        AgentKind::RustAgent,
                    ),
                );
                let oc_overhead = diff_opt(
                    summary_avg_wall(
                        &groups,
                        BenchMode::LocalSmall,
                        ScenarioKind::ToolReadFileSimple,
                        AgentKind::OpenClaw,
                    ),
                    summary_avg_wall(
                        &groups,
                        BenchMode::LocalSmall,
                        ScenarioKind::BaselineNoTool,
                        AgentKind::OpenClaw,
                    ),
                );
                let nb_overhead = diff_opt(
                    summary_avg_wall(
                        &groups,
                        BenchMode::LocalSmall,
                        ScenarioKind::ToolReadFileSimple,
                        AgentKind::NanoBot,
                    ),
                    summary_avg_wall(
                        &groups,
                        BenchMode::LocalSmall,
                        ScenarioKind::BaselineNoTool,
                        AgentKind::NanoBot,
                    ),
                );

                if let Some(rust_ov) = rust_overhead {
                    let mut other_overheads = Vec::new();
                    if let Some(v) = oc_overhead {
                        other_overheads.push(v);
                    }
                    if let Some(v) = nb_overhead {
                        other_overheads.push(v);
                    }
                    if let Some(best_other) = other_overheads
                        .into_iter()
                        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    {
                        if rust_ov > best_other {
                            md.push_str(&format!(
                                "  - Overhead de loop de tool acima dos concorrentes ({:.1} ms vs {:.1} ms).\n",
                                rust_ov, best_other
                            ));
                        }
                    }
                }

                let rust_loops = summary_field(
                    &groups,
                    BenchMode::LocalSmall,
                    ScenarioKind::ToolReadFileFinal,
                    AgentKind::RustAgent,
                    SummaryField::Iterations,
                );
                if let Some(loops) = rust_loops {
                    if loops > 2.0 {
                        md.push_str(&format!(
                            "  - Numero medio de iteracoes alto ({:.2}), indicando turnos extras no AgentLoop.\n",
                            loops
                        ));
                    }
                }

                md.push_str("- Otimizacoes recomendadas:\n");
                md.push_str(
                    "  - Reduzir `max_iterations` efetivo para cenarios simples e encerrar cedo apos primeira resposta valida.\n",
                );
                md.push_str(
                    "  - Aumentar agressividade de filtragem de tools por cenario (`enabled_tools` minimo e ranking por relevancia).\n",
                );
                md.push_str(
                    "  - Cachear prompt base por sessao (identity/rules/skills) para reduzir montagem repetida no loop.\n",
                );
                md.push_str(
                    "  - Instrumentar tempo por etapa (prompt-build, provider roundtrip, parse tool call, execute tool) para cortar o maior hotspot real.\n",
                );
            } else {
                md.push_str("- O Rust Agent nao ficou mais lento no cenario local principal `read_file+final`.\n");
            }
        } else {
            md.push_str("- Dados insuficientes para comparar Rust Agent com OpenClaw/NanoBot no cenario local principal.\n");
        }
    } else {
        md.push_str(
            "- Dados insuficientes para avaliar o Rust Agent no cenario local principal.\n",
        );
    }
    md.push('\n');

    md.push_str("## Observacoes de Confiabilidade\n");
    md.push_str(
        "- As medias usam apenas execucoes validadas (resposta compatível com o criterio do cenario).\n\
- Quando um run falha ou nao valida, o evento permanece contabilizado em `Validadas/Total`.\n\
- Amostra de memoria e aproximada por RSS de processo antes/depois de cada chamada.\n",
    );
    md.push('\n');

    md
}

fn summary_avg_wall(
    groups: &[ScenarioResultGroup],
    mode: BenchMode,
    scenario: ScenarioKind,
    agent: AgentKind,
) -> Option<f64> {
    groups
        .iter()
        .find(|g| g.mode == mode && g.scenario == scenario && g.agent == agent)
        .map(summarize_group)
        .and_then(|summary| summary.avg_wall_ms)
}

enum SummaryField {
    Iterations,
}

fn summary_field(
    groups: &[ScenarioResultGroup],
    mode: BenchMode,
    scenario: ScenarioKind,
    agent: AgentKind,
    field: SummaryField,
) -> Option<f64> {
    let summary = groups
        .iter()
        .find(|g| g.mode == mode && g.scenario == scenario && g.agent == agent)
        .map(summarize_group)?;
    match field {
        SummaryField::Iterations => summary.avg_iterations,
    }
}

fn format_opt(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(v) if v.is_finite() => format!("{:.*}", decimals, v),
        _ => "n/a".to_string(),
    }
}

fn diff_opt(lhs: Option<f64>, rhs: Option<f64>) -> Option<f64> {
    Some(lhs? - rhs?)
}

fn mean_u64(values: impl Iterator<Item = u64>) -> Option<f64> {
    let vec = values.collect::<Vec<_>>();
    if vec.is_empty() {
        return None;
    }
    let sum: u128 = vec.iter().map(|v| *v as u128).sum();
    Some(sum as f64 / vec.len() as f64)
}

fn mean_i64(values: impl Iterator<Item = i64>) -> Option<f64> {
    let vec = values.collect::<Vec<_>>();
    if vec.is_empty() {
        return None;
    }
    let sum: i128 = vec.iter().map(|v| *v as i128).sum();
    Some(sum as f64 / vec.len() as f64)
}

fn mean_usize(values: impl Iterator<Item = usize>) -> Option<f64> {
    let vec = values.collect::<Vec<_>>();
    if vec.is_empty() {
        return None;
    }
    let sum: u128 = vec.iter().map(|v| *v as u128).sum();
    Some(sum as f64 / vec.len() as f64)
}

async fn call_openclaw(
    client: &reqwest::Client,
    daemon_url: &str,
    prompt: &str,
    timeout_ms: u64,
) -> Result<RawCallResult> {
    let url = format!("{daemon_url}/openclaw/chat");
    let body = json!({
        "message": prompt,
        "session_key": format!("bench-openclaw-{}", Uuid::new_v4()),
        "timeout_ms": timeout_ms,
    });
    let response: OpenClawChatResponse = post_json(client, &url, &body).await?;

    Ok(RawCallResult {
        reply: response.reply,
        reported_ms: response.duration_ms,
        iterations: None,
        tool_calls: None,
        total_tokens: response
            .usage
            .and_then(|usage| usage.total.map(|value| value as usize)),
        provider: response.provider,
        model: response.model,
    })
}

async fn call_nanobot(
    client: &reqwest::Client,
    daemon_url: &str,
    prompt: &str,
    timeout_ms: u64,
) -> Result<RawCallResult> {
    let url = format!("{daemon_url}/nanobot/chat");
    let body = json!({
        "message": prompt,
        "session_key": format!("bench-nanobot-{}", Uuid::new_v4()),
        "timeout_ms": timeout_ms,
    });
    let response: NanoBotChatResponse = post_json(client, &url, &body).await?;

    Ok(RawCallResult {
        reply: response.reply,
        reported_ms: response.duration_ms,
        iterations: None,
        tool_calls: None,
        total_tokens: response
            .usage
            .and_then(|usage| usage.total.map(|value| value as usize)),
        provider: response.provider,
        model: response.model,
    })
}

async fn call_rust_agent(
    client: &reqwest::Client,
    daemon_url: &str,
    prompt: &str,
    mode: &RustModeConfig,
    workspace_root: &Path,
) -> Result<RawCallResult> {
    let (provider, model_id, api_key, base_url) = match mode {
        RustModeConfig::Local { provider, model_id } => {
            (provider.clone(), model_id.clone(), None, None)
        }
        RustModeConfig::Remote {
            provider,
            model_id,
            api_key,
            base_url,
        } => (
            provider.clone(),
            model_id.clone(),
            Some(api_key.clone()),
            base_url.clone(),
        ),
    };

    let mut body = json!({
        "message": prompt,
        "provider": provider,
        "model_id": model_id,
        "execution_mode": "read_only",
        "approval_mode": "auto",
        "max_iterations": 8,
        "max_prompt_tokens": 1500,
        "max_history_messages": 6,
        "max_tools_in_prompt": 2,
        "temperature": 0.1,
        "aggressive_tool_filtering": true,
        "enable_tool_call_fallback": true,
        "enabled_tools": ["read_file"],
        "enabled_skills": [],
        "workspace_root": workspace_root.display().to_string(),
        "fallback_enabled": false,
        "streaming": false
    });
    if let Some(key) = api_key {
        body["api_key"] = Value::String(key);
    }
    if let Some(base) = base_url {
        body["base_url"] = Value::String(base);
    }

    let url = format!("{daemon_url}/agent/run");
    let response: AgentRunResponse = post_json(client, &url, &body).await?;

    Ok(RawCallResult {
        reply: response.final_response,
        reported_ms: Some(response.latency_ms),
        iterations: Some(response.iterations),
        tool_calls: Some(response.tool_calls_made),
        total_tokens: Some(response.total_tokens),
        provider: Some(response.provider),
        model: Some(response.model_id),
    })
}

fn prepare_fixtures(ctx: &BenchmarkContext, token: &str) -> Result<FixturePaths> {
    let openclaw_workspace = PathBuf::from(&ctx.openclaw_status.state_dir).join("workspace");
    let nanobot_workspace = PathBuf::from(&ctx.nanobot_status.workspace_path);
    let rust_workspace = env::current_dir().context("falha ao resolver cwd para fixture Rust")?;

    let fixture_name = "bench_agent_fixture.txt";
    let content =
        format!("BENCH_AGENT_TOKEN={token}\nMESSAGE=Benchmark fixture for read_file timing.\n");

    let openclaw_fixture = openclaw_workspace.join(fixture_name);
    let nanobot_fixture = nanobot_workspace.join(fixture_name);
    let rust_fixture = rust_workspace.join(fixture_name);

    write_fixture(&openclaw_fixture, &content)?;
    write_fixture(&nanobot_fixture, &content)?;
    write_fixture(&rust_fixture, &content)?;

    Ok(FixturePaths {
        openclaw: openclaw_fixture,
        nanobot: nanobot_fixture,
        rust: rust_fixture,
    })
}

fn write_fixture(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("falha ao criar diretorio da fixture {}", parent.display()))?;
    }
    fs::write(path, content)
        .with_context(|| format!("falha ao escrever fixture {}", path.display()))?;
    Ok(())
}

fn select_local_models(ctx: &BenchmarkContext) -> Result<LocalSelection> {
    let shared_local = smallest_shared_local_model(
        &ctx.openclaw_models.local_models,
        &ctx.nanobot_models.local_models,
    );

    let (openclaw_model_id, nanobot_model_id) = if let Some(shared) = shared_local {
        (shared.clone(), shared)
    } else {
        let openclaw = pick_smallest_local_model(&ctx.openclaw_models.local_models)
            .ok_or_else(|| anyhow!("OpenClaw sem modelos locais disponiveis"))?;
        let nanobot = pick_smallest_local_model(&ctx.nanobot_models.local_models)
            .ok_or_else(|| anyhow!("NanoBot sem modelos locais disponiveis"))?;
        (openclaw.id, nanobot.id)
    };

    let rust_local = preferred_local_from_agent_config(&ctx.agent_providers, &ctx.agent_config)
        .or_else(|| pick_smallest_local_provider_model(&ctx.agent_providers))
        .ok_or_else(|| anyhow!("Rust Agent sem provider local com modelos disponiveis"))?;

    Ok(LocalSelection {
        openclaw_model_id,
        nanobot_model_id,
        rust_provider: rust_local.0,
        rust_model: rust_local.1,
    })
}

fn preferred_local_from_agent_config(
    providers: &[AgentProviderInfo],
    agent_cfg: &AgentConfigResponse,
) -> Option<(String, String)> {
    let provider_id = agent_cfg.provider.trim();
    let model_id = agent_cfg.model_id.trim();
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }

    let provider = providers
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(provider_id) && entry.kind == "local")?;

    if provider.models.is_empty()
        || provider
            .models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(model_id))
    {
        return Some((provider.id.clone(), model_id.to_string()));
    }
    None
}

fn select_remote_models(ctx: &BenchmarkContext) -> Option<RemoteSelection> {
    let env_values = &ctx.env_values;
    let remote_registry: HashMap<String, AgentProviderInfo> = ctx
        .agent_providers
        .iter()
        .filter(|provider| provider.kind == "remote")
        .cloned()
        .map(|provider| (provider.id.clone(), provider))
        .collect();

    for provider in PROVIDER_PRIORITY {
        let registry_entry = match remote_registry.get(provider) {
            Some(entry) => entry,
            None => continue,
        };
        let api_key = provider_api_key(provider, env_values, &ctx.agent_config)?;
        if api_key.trim().is_empty() {
            continue;
        }

        let openclaw_cloud = ctx
            .openclaw_models
            .cloud_models
            .iter()
            .find(|model| provider_matches(provider, model))
            .cloned();
        let nanobot_cloud = ctx
            .nanobot_models
            .cloud_models
            .iter()
            .find(|model| provider_matches(provider, model))
            .cloned();
        let (Some(openclaw_cloud), Some(nanobot_cloud)) = (openclaw_cloud, nanobot_cloud) else {
            continue;
        };

        let rust_model = pick_remote_model_for_provider(
            registry_entry,
            provider,
            &openclaw_cloud.reference,
            &nanobot_cloud.reference,
        )?;
        let base_url = provider_base_url(provider, env_values, &ctx.agent_config);

        return Some(RemoteSelection {
            provider: provider.to_string(),
            openclaw_model_reference: openclaw_cloud.reference,
            nanobot_model_reference: nanobot_cloud.reference,
            rust_model,
            api_key,
            base_url,
        });
    }
    None
}

fn provider_matches(provider: &str, model: &OpenClawCloudModel) -> bool {
    let normalized = model.provider.trim().to_ascii_lowercase();
    if normalized == provider {
        return true;
    }
    model
        .reference
        .split('/')
        .next()
        .map(|head| head.eq_ignore_ascii_case(provider))
        .unwrap_or(false)
}

fn provider_api_key(
    provider: &str,
    env_values: &HashMap<String, String>,
    agent_cfg: &AgentConfigResponse,
) -> Option<String> {
    let key = match provider {
        "deepseek" => first_non_empty(env_values, &["DEEPSEEK_API_KEY"]),
        "openrouter" => first_non_empty(env_values, &["OPENROUTER_API_KEY"]),
        "groq" => first_non_empty(env_values, &["GROQ_API_KEY"]),
        "anthropic" => first_non_empty(env_values, &["ANTHROPIC_API_KEY"]),
        "openai" => first_non_empty(env_values, &["OPENAI_API_KEY"]),
        _ => None,
    };
    if key.is_some() {
        return key;
    }
    if agent_cfg.provider.eq_ignore_ascii_case(provider) && !agent_cfg.api_key.trim().is_empty() {
        return Some(agent_cfg.api_key.clone());
    }
    None
}

fn provider_base_url(
    provider: &str,
    env_values: &HashMap<String, String>,
    agent_cfg: &AgentConfigResponse,
) -> Option<String> {
    let from_env = match provider {
        "deepseek" => first_non_empty(env_values, &["DEEPSEEK_BASE_URL"]),
        "openrouter" => first_non_empty(env_values, &["OPENROUTER_BASE_URL"]),
        "groq" => first_non_empty(env_values, &["GROQ_BASE_URL"]),
        "anthropic" => first_non_empty(env_values, &["ANTHROPIC_BASE_URL"]),
        "openai" => first_non_empty(env_values, &["OPENAI_BASE_URL"]),
        _ => None,
    };
    if from_env.is_some() {
        return from_env;
    }
    if agent_cfg.provider.eq_ignore_ascii_case(provider) && !agent_cfg.base_url.trim().is_empty() {
        return Some(agent_cfg.base_url.clone());
    }
    None
}

fn pick_remote_model_for_provider(
    registry_entry: &AgentProviderInfo,
    provider: &str,
    openclaw_reference: &str,
    nanobot_reference: &str,
) -> Option<String> {
    if registry_entry.models.is_empty() {
        let fallback = openclaw_reference
            .strip_prefix(&format!("{provider}/"))
            .unwrap_or(openclaw_reference)
            .to_string();
        return Some(fallback);
    }

    let candidates = &registry_entry.models;
    if candidates.iter().any(|model| model == openclaw_reference) {
        return Some(openclaw_reference.to_string());
    }
    let trimmed = openclaw_reference
        .strip_prefix(&format!("{provider}/"))
        .unwrap_or(openclaw_reference);
    if candidates.iter().any(|model| model == trimmed) {
        return Some(trimmed.to_string());
    }
    if candidates.iter().any(|model| model == nanobot_reference) {
        return Some(nanobot_reference.to_string());
    }
    candidates.first().cloned()
}

fn pick_smallest_local_provider_model(providers: &[AgentProviderInfo]) -> Option<(String, String)> {
    let mut best: Option<(String, String, f64)> = None;
    for provider in providers.iter().filter(|provider| provider.kind == "local") {
        for model in &provider.models {
            let tool_penalty = if provider.supports_tool_calling {
                0.0
            } else {
                1000.0
            };
            let score = small_local_score(model) + tool_penalty;
            let candidate = (provider.id.clone(), model.clone(), score);
            match &best {
                Some(current) if current.2 <= candidate.2 => {}
                _ => best = Some(candidate),
            }
        }
    }
    best.map(|(provider, model, _)| (provider, model))
}

fn smallest_shared_local_model(
    openclaw_models: &[OpenClawLocalModel],
    nanobot_models: &[OpenClawLocalModel],
) -> Option<String> {
    let nano_ids = nanobot_models
        .iter()
        .map(|model| model.id.clone())
        .collect::<HashSet<_>>();

    openclaw_models
        .iter()
        .filter(|model| nano_ids.contains(&model.id))
        .min_by(|left, right| {
            small_local_score(&left.id)
                .partial_cmp(&small_local_score(&right.id))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|model| model.id.clone())
}

fn pick_smallest_local_model(models: &[OpenClawLocalModel]) -> Option<OpenClawLocalModel> {
    models
        .iter()
        .min_by(|left, right| {
            small_local_score(&format!("{} {}", left.id, left.name))
                .partial_cmp(&small_local_score(&format!("{} {}", right.id, right.name)))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

fn small_local_score(text: &str) -> f64 {
    let size = model_size_score(text);
    if (4.0..=13.0).contains(&size) {
        (size - 8.0).abs()
    } else if size > 13.0 && size < 10_000.0 {
        100.0 + size
    } else if size < 4.0 {
        50.0 + (4.0 - size)
    } else {
        500.0 + size
    }
}

fn model_size_score(text: &str) -> f64 {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut best: Option<f64> = None;
    for idx in 0..bytes.len() {
        if bytes[idx] != b'b' {
            continue;
        }
        if idx + 1 < bytes.len() && bytes[idx + 1] == b'i' {
            // Ignore quantization markers like "4bit" or "8bit".
            continue;
        }
        let mut start = idx;
        while start > 0 {
            let ch = bytes[start - 1] as char;
            if ch.is_ascii_digit() || ch == '.' {
                start -= 1;
            } else {
                break;
            }
        }
        if start == idx {
            continue;
        }
        let number = &lower[start..idx];
        if let Ok(value) = number.parse::<f64>() {
            best = Some(match best {
                Some(current) => current.min(value),
                None => value,
            });
        }
    }
    best.unwrap_or(10_000.0)
}

async fn prepare_runtime_pids(client: &reqwest::Client, daemon_url: &str) -> RuntimePids {
    let openclaw = ensure_runtime_pid(client, daemon_url, "openclaw").await;
    let nanobot = ensure_runtime_pid(client, daemon_url, "nanobot").await;
    let rust_daemon = daemon_listener_pid(daemon_url).ok().flatten();
    RuntimePids {
        openclaw,
        nanobot,
        rust_daemon,
    }
}

async fn ensure_runtime_pid(
    client: &reqwest::Client,
    daemon_url: &str,
    runtime_name: &str,
) -> Option<u32> {
    let runtime_url = format!("{daemon_url}/{runtime_name}/runtime");
    let start_payload = json!({ "action": "start" });
    let _ = post_json::<Value>(client, &runtime_url, &start_payload).await;
    let status = get_json::<RuntimeStateResponse>(client, &runtime_url)
        .await
        .ok()?;
    status.pid.map(|pid| pid as u32)
}

fn daemon_listener_pid(daemon_url: &str) -> Result<Option<u32>> {
    let parsed = Url::parse(daemon_url)
        .with_context(|| format!("daemon_url invalida para resolver PID: {daemon_url}"))?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| anyhow!("daemon_url sem porta conhecida: {daemon_url}"))?;
    let query = format!("TCP:{port}");
    let output = Command::new("lsof")
        .args(["-nP", "-i", &query, "-sTCP:LISTEN", "-t"])
        .output()
        .context("falha ao executar lsof para PID do daemon")?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pid = stdout
        .lines()
        .find_map(|line| line.trim().parse::<u32>().ok());
    Ok(pid)
}

fn read_rss_kb(pid: u32) -> Option<u64> {
    let pid_value = pid.to_string();
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid_value])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .find_map(|token| token.trim().parse::<u64>().ok())
}

impl RestorePlan {
    async fn apply(&self, client: &reqwest::Client, daemon_url: &str) -> Result<()> {
        restore_openclaw_model(
            client,
            daemon_url,
            &self.openclaw_current,
            &self.openclaw_locals,
        )
        .await
        .context("falha ao restaurar modelo OpenClaw")?;
        restore_nanobot_model(
            client,
            daemon_url,
            &self.nanobot_current,
            &self.nanobot_locals,
        )
        .await
        .context("falha ao restaurar modelo NanoBot")?;
        Ok(())
    }
}

async fn restore_openclaw_model(
    client: &reqwest::Client,
    daemon_url: &str,
    current: &OpenClawCurrentModel,
    local_models: &[OpenClawLocalModel],
) -> Result<()> {
    if current.source.eq_ignore_ascii_case("cloud") {
        configure_openclaw_remote(client, daemon_url, &current.reference).await
    } else {
        let local_id = resolve_local_model_id(current, local_models).ok_or_else(|| {
            anyhow!("modelo local original OpenClaw nao encontrado para restauracao")
        })?;
        configure_openclaw_local(client, daemon_url, &local_id).await
    }
}

async fn restore_nanobot_model(
    client: &reqwest::Client,
    daemon_url: &str,
    current: &OpenClawCurrentModel,
    local_models: &[OpenClawLocalModel],
) -> Result<()> {
    if current.source.eq_ignore_ascii_case("cloud") {
        configure_nanobot_remote(client, daemon_url, &current.reference).await
    } else {
        let local_id = resolve_local_model_id(current, local_models).ok_or_else(|| {
            anyhow!("modelo local original NanoBot nao encontrado para restauracao")
        })?;
        configure_nanobot_local(client, daemon_url, &local_id).await
    }
}

fn resolve_local_model_id(
    current: &OpenClawCurrentModel,
    local_models: &[OpenClawLocalModel],
) -> Option<String> {
    let reference = current.reference.trim();
    if let Some(model) = local_models.iter().find(|model| {
        model.id == reference
            || model.path == reference
            || model.path == current.model
            || model.name == current.model
    }) {
        return Some(model.id.clone());
    }

    if let Some(reference_path) = reference.strip_prefix("openai/") {
        if let Some(model) = local_models
            .iter()
            .find(|model| model.path.trim() == reference_path.trim())
        {
            return Some(model.id.clone());
        }
    }

    None
}

async fn configure_openclaw_local(
    client: &reqwest::Client,
    daemon_url: &str,
    local_model_id: &str,
) -> Result<()> {
    let url = format!("{daemon_url}/openclaw/model");
    let body = json!({
        "source": "local",
        "local_model_id": local_model_id,
    });
    let _: OpenClawCurrentModel = post_json(client, &url, &body).await?;
    Ok(())
}

async fn configure_openclaw_remote(
    client: &reqwest::Client,
    daemon_url: &str,
    model_reference: &str,
) -> Result<()> {
    let url = format!("{daemon_url}/openclaw/model");
    let body = json!({
        "source": "cloud",
        "model_reference": model_reference,
    });
    let _: OpenClawCurrentModel = post_json(client, &url, &body).await?;
    Ok(())
}

async fn configure_nanobot_local(
    client: &reqwest::Client,
    daemon_url: &str,
    local_model_id: &str,
) -> Result<()> {
    let url = format!("{daemon_url}/nanobot/model");
    let body = json!({
        "source": "local",
        "local_model_id": local_model_id,
    });
    let _: OpenClawCurrentModel = post_json(client, &url, &body).await?;
    Ok(())
}

async fn configure_nanobot_remote(
    client: &reqwest::Client,
    daemon_url: &str,
    model_reference: &str,
) -> Result<()> {
    let url = format!("{daemon_url}/nanobot/model");
    let body = json!({
        "source": "cloud",
        "model_reference": model_reference,
    });
    let _: OpenClawCurrentModel = post_json(client, &url, &body).await?;
    Ok(())
}

async fn fetch_benchmark_context(
    client: &reqwest::Client,
    daemon_url: &str,
) -> Result<BenchmarkContext> {
    let openclaw_models =
        get_json::<OpenClawModelsResponse>(client, &format!("{daemon_url}/openclaw/models"))
            .await
            .context("falha ao carregar /openclaw/models")?;
    let nanobot_models =
        get_json::<OpenClawModelsResponse>(client, &format!("{daemon_url}/nanobot/models"))
            .await
            .context("falha ao carregar /nanobot/models")?;
    let openclaw_status =
        get_json::<OpenClawStatusResponse>(client, &format!("{daemon_url}/openclaw/status"))
            .await
            .context("falha ao carregar /openclaw/status")?;
    let nanobot_status =
        get_json::<NanoBotStatusResponse>(client, &format!("{daemon_url}/nanobot/status"))
            .await
            .context("falha ao carregar /nanobot/status")?;
    let agent_providers =
        get_json::<Vec<AgentProviderInfo>>(client, &format!("{daemon_url}/agent/providers"))
            .await
            .context(
                "falha ao carregar /agent/providers (daemon possivelmente desatualizado, reinicie com o codigo atual)",
            )?;
    let agent_config =
        get_json::<AgentConfigResponse>(client, &format!("{daemon_url}/agent/config"))
            .await
            .context("falha ao carregar /agent/config")?;

    let env_values = match get_json::<EnvironmentResponse>(
        client,
        &format!("{daemon_url}/openclaw/environment?reveal=true"),
    )
    .await
    {
        Ok(response) => response
            .variables
            .into_iter()
            .filter(|variable| variable.present)
            .filter(|variable| !variable.value.trim().is_empty())
            .map(|variable| (variable.key, variable.value))
            .collect::<HashMap<_, _>>(),
        Err(_) => HashMap::new(),
    };

    Ok(BenchmarkContext {
        openclaw_models,
        nanobot_models,
        openclaw_status,
        nanobot_status,
        agent_providers,
        agent_config,
        env_values,
    })
}

async fn assert_daemon_health(client: &reqwest::Client, daemon_url: &str) -> Result<()> {
    let response = client
        .get(format!("{daemon_url}/health"))
        .send()
        .await
        .with_context(|| format!("falha ao consultar health do daemon em {daemon_url}"))?;
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(anyhow!(
        "health check retornou HTTP {status}: {}",
        body.trim()
    ))
}

async fn get_json<T: DeserializeOwned>(client: &reqwest::Client, url: &str) -> Result<T> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("falha em GET {url}"))?;
    decode_response(response, "GET", url).await
}

async fn post_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    body: &Value,
) -> Result<T> {
    let response = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .with_context(|| format!("falha em POST {url}"))?;
    decode_response(response, "POST", url).await
}

async fn decode_response<T: DeserializeOwned>(
    response: reqwest::Response,
    method: &str,
    url: &str,
) -> Result<T> {
    let status = response.status();
    let text = response
        .text()
        .await
        .unwrap_or_else(|_| "<body indisponivel>".to_string());
    if !status.is_success() {
        return Err(anyhow!(
            "{method} {url} retornou HTTP {}: {}",
            status,
            text.trim()
        ));
    }
    serde_json::from_str::<T>(&text)
        .with_context(|| format!("falha ao decodificar JSON de {method} {url}: {text}"))
}

fn first_non_empty(values: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| values.get(*key))
        .find(|value| !value.trim().is_empty())
        .cloned()
}

fn parse_cli_options() -> Result<CliOptions> {
    let mut daemon_url = default_daemon_url();
    let mut iterations = DEFAULT_ITERATIONS;
    let mut output_path = PathBuf::from(DEFAULT_OUTPUT);
    let mut timeout_ms: u64 = 30_000;
    let mut skip_remote = false;

    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--daemon-url" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow!("--daemon-url requer um valor"))?;
                daemon_url = value.clone();
            }
            "--iterations" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow!("--iterations requer um valor"))?;
                iterations = value
                    .parse::<usize>()
                    .with_context(|| format!("iteracoes invalidas: {value}"))?;
            }
            "--output" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow!("--output requer um caminho"))?;
                output_path = PathBuf::from(value);
            }
            "--timeout-ms" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| anyhow!("--timeout-ms requer um valor"))?;
                timeout_ms = value
                    .parse::<u64>()
                    .with_context(|| format!("timeout invalido: {value}"))?;
            }
            "--skip-remote" => {
                skip_remote = true;
            }
            "--help" | "-h" => {
                print_help_and_exit();
            }
            unknown => {
                return Err(anyhow!(
                    "argumento desconhecido: {unknown}. Use --help para ver opcoes."
                ));
            }
        }
        idx += 1;
    }

    if iterations == 0 {
        return Err(anyhow!("--iterations deve ser maior que zero"));
    }
    daemon_url = daemon_url.trim_end_matches('/').to_string();

    Ok(CliOptions {
        daemon_url,
        iterations,
        output_path,
        timeout_ms,
        skip_remote,
    })
}

fn print_help_and_exit() -> ! {
    println!(
        "bench-agent\n\
Uso:\n\
  cargo run -p bench-agent -- [opcoes]\n\n\
Opcoes:\n\
  --daemon-url <url>    URL do daemon (default: valor de settings.json ou http://127.0.0.1:11435)\n\
  --iterations <n>      Iteracoes por cenario (default: 10)\n\
  --timeout-ms <n>      Timeout por chamada de chat em ms (default: 30000)\n\
  --output <path>       Caminho do relatorio markdown\n\
  --skip-remote         Pula benchmark remoto\n\
  -h, --help            Mostra ajuda\n"
    );
    std::process::exit(0);
}

fn default_daemon_url() -> String {
    if let Ok(url) = env::var("BENCH_DAEMON_URL") {
        if !url.trim().is_empty() {
            return url;
        }
    }

    if let Some(settings_url) = daemon_url_from_settings() {
        return settings_url;
    }
    "http://127.0.0.1:11435".to_string()
}

fn daemon_url_from_settings() -> Option<String> {
    let home = env::var("HOME")
        .ok()
        .or_else(|| env::var("USERPROFILE").ok())?;
    let path = PathBuf::from(home)
        .join(".config")
        .join("mlx-ollama-pilot")
        .join("settings.json");
    let content = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&content).ok()?;
    let bind = value.get("bind_addr")?.as_str()?.trim();
    if bind.is_empty() {
        return None;
    }
    Some(format!("http://{bind}"))
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}
