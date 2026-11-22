use std::path::PathBuf;
use std::process::Stdio;
use std::time::Instant;

use chrono::Utc;
use mlx_ollama_core::ModelProvider;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeDoctorRequest {
    #[serde(default)]
    pub apply_fixes: bool,
    #[serde(default)]
    pub allow_updates: bool,
    #[serde(default = "default_true")]
    pub run_validation: bool,
    #[serde(default)]
    pub validation_model: Option<String>,
}

impl Default for RuntimeDoctorRequest {
    fn default() -> Self {
        Self {
            apply_fixes: false,
            allow_updates: false,
            run_validation: true,
            validation_model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDoctorReport {
    pub generated_at: String,
    pub system: SystemInfo,
    pub components: Vec<ComponentStatus>,
    pub issues: Vec<DoctorIssue>,
    pub fixes_applied: Vec<DoctorFix>,
    pub active_backend: ActiveBackend,
    pub validation: ValidationReport,
    pub recommendations: Vec<String>,
    pub log_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub os: String,
    pub os_version: Option<String>,
    pub architecture: String,
    pub gpu: Vec<GpuInfo>,
    pub python_alias_healthy: bool,
    pub backend_priority: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuInfo {
    pub backend: String,
    pub name: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentStatus {
    pub id: String,
    pub installed: bool,
    pub healthy: bool,
    pub configured_path: Option<String>,
    pub local_version: Option<String>,
    pub latest_version: Option<String>,
    pub outdated: bool,
    pub details: Vec<String>,
    pub models: Vec<ModelSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSummary {
    pub id: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorIssue {
    pub severity: String,
    pub component: String,
    pub message: String,
    pub fix_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorFix {
    pub action: String,
    pub status: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveBackend {
    pub provider: String,
    pub execution_backend: String,
    pub reason: String,
    pub fallback_chain: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub attempted: bool,
    pub success: bool,
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub latency_ms: Option<u64>,
    pub output_preview: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
struct LatestVersions {
    ollama: Option<String>,
    mlx: Option<String>,
    mlx_lm: Option<String>,
    llamacpp: Option<String>,
}

#[derive(Debug, Default)]
struct ProbeResult {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
struct PythonProbe {
    healthy: bool,
    details: Vec<String>,
    local_version: Option<String>,
    latest_version: Option<String>,
    outdated: bool,
    configured_path: Option<String>,
}

impl PythonProbe {
    fn into_component(self) -> ComponentStatus {
        ComponentStatus {
            id: "python".to_string(),
            installed: self.configured_path.is_some(),
            healthy: self.healthy,
            configured_path: self.configured_path,
            local_version: self.local_version,
            latest_version: self.latest_version,
            outdated: self.outdated,
            details: self.details,
            models: Vec::new(),
        }
    }
}

pub async fn inspect_runtime(
    state: &crate::AppState,
    request: RuntimeDoctorRequest,
) -> RuntimeDoctorReport {
    let latest = fetch_latest_versions().await;
    let mut fixes_applied = Vec::new();
    let mut issues = Vec::new();
    let mut recommendations = Vec::new();

    let python_probe = python_component(&mut issues, &mut recommendations).await;
    let (ollama_component, ollama_fix_details, ollama_models) =
        ollama_component(&state.ollama_provider, &latest, request.apply_fixes).await;
    if let Some(fix) = ollama_fix_details {
        fixes_applied.push(fix);
    }

    let (mlx_component, mlx_supported) = mlx_component(state, &latest, &mut issues).await;
    let llamacpp_component = llamacpp_component(state, &latest, &mut issues).await;

    if request.allow_updates {
        recommendations.push(
            "Atualizacoes automaticas estao permitidas por request, mas nenhuma foi aplicada porque o backend ativo ja esta estavel.".to_string(),
        );
    }

    let system = SystemInfo {
        os: std::env::consts::OS.to_string(),
        os_version: detect_os_version().await,
        architecture: std::env::consts::ARCH.to_string(),
        gpu: detect_gpus().await,
        python_alias_healthy: python_probe.healthy,
        backend_priority: backend_priority(mlx_supported),
    };

    let active_backend = choose_backend(
        &system,
        &ollama_component,
        &llamacpp_component,
        &mlx_component,
    );

    let validation = if request.run_validation {
        validate_active_backend(
            state,
            &active_backend,
            &ollama_models,
            request.validation_model.as_deref(),
        )
        .await
    } else {
        ValidationReport {
            attempted: false,
            success: false,
            provider: None,
            model_id: None,
            latency_ms: None,
            output_preview: None,
            error: Some("validation skipped by request".to_string()),
        }
    };

    if !validation.success {
        if let Some(error) = validation.error.as_deref() {
            issues.push(DoctorIssue {
                severity: "warn".to_string(),
                component: "validation".to_string(),
                message: format!("validacao do backend ativo nao concluiu: {error}"),
                fix_hint: Some(
                    "Use um modelo local valido no provider selecionado e repita o smoke test."
                        .to_string(),
                ),
            });
        }
    }

    if !ollama_component.healthy {
        recommendations.push(
            "Se Ollama voltar a falhar, execute `ollama serve` e confira `http://127.0.0.1:11434/api/version`."
                .to_string(),
        );
    }
    if !llamacpp_component.installed {
        recommendations.push(
            "Instale `llama.cpp` apenas se precisar de fallback nativo fora do Ollama; no Windows use `winget install --id ggml.llama.cpp -e`."
                .to_string(),
        );
    }
    if !mlx_supported {
        recommendations.push(
            "Nao tente usar MLX neste host; priorize Ollama ou llama.cpp. MLX deve ser reservado a macOS Apple Silicon ou setups Linux explicitamente validados."
                .to_string(),
        );
    }

    let components = vec![
        python_probe.into_component(),
        ollama_component,
        llamacpp_component,
        mlx_component,
    ];

    let mut report = RuntimeDoctorReport {
        generated_at: Utc::now().to_rfc3339(),
        system,
        components,
        issues,
        fixes_applied,
        active_backend,
        validation,
        recommendations,
        log_paths: Vec::new(),
    };

    if let Some(path) = persist_report(&report).await {
        report.log_paths.push(path.display().to_string());
    }

    report
}

fn default_true() -> bool {
    true
}

async fn python_component(
    issues: &mut Vec<DoctorIssue>,
    recommendations: &mut Vec<String>,
) -> PythonProbe {
    let mut details = Vec::new();
    let mut local_version = None;
    let mut configured_path = None;

    if cfg!(target_os = "windows") {
        let launcher = run_capture("py", &["-0p"], 5).await;
        if launcher.success {
            configured_path = launcher
                .stdout
                .lines()
                .find_map(|line| line.split_whitespace().last().map(ToString::to_string));
            details.push("launcher `py` detectado e funcional".to_string());
            let version = run_capture("py", &["--version"], 5).await;
            if version.success {
                local_version = first_line(&version.stdout);
            }
        } else {
            issues.push(DoctorIssue {
                severity: "error".to_string(),
                component: "python".to_string(),
                message: "launcher `py` nao respondeu neste Windows host".to_string(),
                fix_hint: Some(
                    "Reinstale o Python oficial e habilite o launcher `py`.".to_string(),
                ),
            });
        }

        let alias = run_capture("python", &["--version"], 5).await;
        if alias.success {
            details.push("alias `python` funcional".to_string());
            if local_version.is_none() {
                local_version = first_line(&alias.stdout);
            }
        } else {
            details.push(
                "alias `python` do Windows esta indisponivel; usando `py` como fallback"
                    .to_string(),
            );
            recommendations.push(
                "Padronize chamadas internas em `py` ou em um caminho absoluto de `python.exe`; nao dependa do alias da Microsoft Store.".to_string(),
            );
            issues.push(DoctorIssue {
                severity: "warn".to_string(),
                component: "python".to_string(),
                message: "o alias `python` falha neste host, embora `py` funcione".to_string(),
                fix_hint: Some(
                    "Desative o App Execution Alias do Windows ou use `py`/`python.exe` explicito."
                        .to_string(),
                ),
            });
        }
    } else {
        let python3 = run_capture("python3", &["--version"], 5).await;
        if python3.success {
            local_version = first_line(&python3.stdout);
            configured_path = Some("python3".to_string());
            details.push("python3 funcional".to_string());
        }
        let python = run_capture("python", &["--version"], 5).await;
        if python.success {
            details.push("python funcional".to_string());
            if local_version.is_none() {
                local_version = first_line(&python.stdout);
                configured_path = Some("python".to_string());
            }
        }
    }

    PythonProbe {
        healthy: local_version.is_some(),
        details,
        local_version,
        latest_version: None,
        outdated: false,
        configured_path,
    }
}

async fn ollama_component(
    provider: &ollama_provider::OllamaProvider,
    latest: &LatestVersions,
    apply_fixes: bool,
) -> (ComponentStatus, Option<DoctorFix>, Vec<ModelSummary>) {
    let cfg = provider.config();
    let mut fix = None;
    let mut details = vec![format!("base_url configurado: {}", cfg.base_url)];
    let cli = run_capture("ollama", &["--version"], 10).await;
    let mut api_version = fetch_ollama_api_version(&cfg.base_url).await;
    if api_version.is_none() && apply_fixes {
        match provider.prepare_runtime(None).await {
            Ok(()) => {
                api_version = fetch_ollama_api_version(&cfg.base_url).await;
                fix = Some(DoctorFix {
                    action: "start_ollama".to_string(),
                    status: "applied".to_string(),
                    details: "Runtime Ollama preparado por solicitação explícita".to_string(),
                });
            }
            Err(error) => details.push(format!("bootstrap automatico do Ollama falhou: {error}")),
        }
    }
    let runtime = provider.runtime_status().await;
    let local_version = api_version
        .clone()
        .or(runtime.installed_version)
        .or_else(|| first_line(&cli.stdout).map(|line| line.replace("ollama version is ", "")));
    let configured_path = runtime
        .executable_path
        .or_else(|| cli.success.then(|| "ollama".to_string()));
    let healthy = api_version.is_some();
    let models = fetch_ollama_models(&cfg.base_url).await.unwrap_or_default();
    if healthy {
        details.push(format!("API Ollama saudavel em {}", cfg.base_url));
    } else {
        details.push(
            "API Ollama indisponível; inspeção preservou a preparação em andamento".to_string(),
        );
    }
    let latest_version = latest.ollama.clone();
    let outdated = match (local_version.as_deref(), latest_version.as_deref()) {
        (Some(local), Some(latest)) => version_lt(local, latest),
        _ => false,
    };
    (
        ComponentStatus {
            id: "ollama".to_string(),
            installed: configured_path.is_some() || healthy,
            healthy,
            configured_path,
            local_version,
            latest_version,
            outdated,
            details,
            models: models.clone(),
        },
        fix,
        models,
    )
}

/// Observe the running API without calling the provider's preparation or installation path.
async fn fetch_ollama_models(base_url: &str) -> Option<Vec<ModelSummary>> {
    let client = build_http_client()?;
    let payload = client
        .get(format!("{}/api/tags", base_url.trim_end_matches('/')))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Value>()
        .await
        .ok()?;
    Some(
        payload
            .get("models")?
            .as_array()?
            .iter()
            .filter_map(|model| {
                Some(ModelSummary {
                    id: model
                        .get("name")
                        .or_else(|| model.get("model"))?
                        .as_str()?
                        .to_string(),
                    provider: "ollama".to_string(),
                })
            })
            .collect(),
    )
}

async fn validate_ollama_http(base_url: &str, model_id: &str) -> Result<String, String> {
    let client = build_http_client().ok_or("Não foi possível criar o cliente de diagnóstico")?;
    let response = client
        .post(format!("{}/api/chat", base_url.trim_end_matches('/')))
        .json(&serde_json::json!({
            "model": model_id,
            "messages": [{"role": "user", "content": "Reply with exactly OK."}],
            "stream": false,
            "options": {"temperature": 0, "num_predict": 16},
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json::<Value>()
        .await
        .map_err(|error| error.to_string())?;
    response
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "Ollama respondeu sem conteúdo ao diagnóstico".to_string())
}

async fn mlx_component(
    state: &crate::AppState,
    latest: &LatestVersions,
    issues: &mut Vec<DoctorIssue>,
) -> (ComponentStatus, bool) {
    let cfg = state.mlx_provider.config();
    let mut details = vec![
        format!("command configurado: {}", cfg.command),
        format!("python AIRLLM configurado: {}", cfg.airllm_python_command),
    ];

    if cfg!(target_os = "windows") {
        issues.push(DoctorIssue {
            severity: "warn".to_string(),
            component: "mlx".to_string(),
            message: "MLX foi detectado na configuracao, mas este runtime nao suporta o provider nativamente no Windows.".to_string(),
            fix_hint: Some(
                "Use Ollama ou llama.cpp neste host; reserve MLX para macOS Apple Silicon ou Linux validado."
                    .to_string(),
            ),
        });
        details.push("backend marcado como indisponivel por plataforma".to_string());
        return (
            ComponentStatus {
                id: "mlx".to_string(),
                installed: false,
                healthy: false,
                configured_path: Some(cfg.command.clone()),
                local_version: None,
                latest_version: latest.mlx.clone(),
                outdated: false,
                details,
                models: Vec::new(),
            },
            false,
        );
    }

    let python_cmd = cfg.airllm_python_command.trim();
    let version_script = "import importlib.metadata as m, json\nresult={}\nfor name in ('mlx','mlx-lm'):\n    try:\n        result[name]=m.version(name)\n    except Exception:\n        result[name]=None\nprint(json.dumps(result))";
    let probe = run_capture(python_cmd, &["-c", version_script], 12).await;
    let local_versions = if probe.success {
        serde_json::from_str::<Value>(&probe.stdout).ok()
    } else {
        None
    };

    if !probe.success && cfg!(target_os = "macos") {
        let combined = format!("{} {}", probe.stdout, probe.stderr).to_ascii_lowercase();
        if combined.contains("dyld") || combined.contains("dlopen") {
            issues.push(DoctorIssue {
                severity: "error".to_string(),
                component: "mlx".to_string(),
                message: "possivel falha de dylib/dyld detectada no probe do MLX".to_string(),
                fix_hint: Some(
                    "Reinstale `mlx` e `mlx-lm`, valide `DYLD_LIBRARY_PATH` e confira se o Python ativo e Apple Silicon nativo."
                        .to_string(),
                ),
            });
            details.push("sintomas de dylib/dyld detectados no probe".to_string());
        }
    }

    let mlx_version = local_versions
        .as_ref()
        .and_then(|value| value.get("mlx"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let mlx_lm_version = local_versions
        .as_ref()
        .and_then(|value| value.get("mlx-lm"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    if let Some(version) = mlx_lm_version.as_deref() {
        details.push(format!("mlx-lm detectado: {version}"));
    }
    if let Some(version) = mlx_version.as_deref() {
        details.push(format!("mlx detectado: {version}"));
    }

    let healthy = mlx_version.is_some() || mlx_lm_version.is_some();
    (
        ComponentStatus {
            id: "mlx".to_string(),
            installed: healthy,
            healthy,
            configured_path: Some(cfg.command.clone()),
            local_version: mlx_version.or(mlx_lm_version),
            latest_version: latest.mlx_lm.clone().or(latest.mlx.clone()),
            outdated: match (
                local_versions
                    .as_ref()
                    .and_then(|value| value.get("mlx-lm"))
                    .and_then(Value::as_str),
                latest.mlx_lm.as_deref(),
            ) {
                (Some(local), Some(latest)) => version_lt(local, latest),
                _ => false,
            },
            details,
            models: Vec::new(),
        },
        healthy,
    )
}

async fn llamacpp_component(
    state: &crate::AppState,
    latest: &LatestVersions,
    issues: &mut Vec<DoctorIssue>,
) -> ComponentStatus {
    let cfg = state.llamacpp_provider.config();
    let installed_binary = state.llamacpp_provider.installed_runtime_binary().await;
    let resolved_binary = installed_binary
        .as_deref()
        .unwrap_or(cfg.server_binary.trim());
    let version_probe = run_capture(resolved_binary, &["--version"], 8).await;
    let installed = installed_binary.is_some();
    let models = state
        .llamacpp_provider
        .list_models()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|entry| ModelSummary {
            id: entry.id,
            provider: entry.provider,
        })
        .collect::<Vec<_>>();

    if !installed {
        issues.push(DoctorIssue {
            severity: "info".to_string(),
            component: "llama.cpp".to_string(),
            message: "nenhum binario executavel `llama-server` foi detectado nos caminhos configurados e no cache".to_string(),
            fix_hint: Some(
                "Se precisar de fallback nativo, instale `llama.cpp` e adicione um modelo `.gguf` ao diretorio configurado."
                    .to_string(),
            ),
        });
    }

    ComponentStatus {
        id: "llama.cpp".to_string(),
        installed,
        healthy: installed,
        configured_path: Some(resolved_binary.to_string()),
        local_version: first_line(&version_probe.stdout),
        latest_version: latest.llamacpp.clone(),
        outdated: match (
            first_line(&version_probe.stdout),
            latest.llamacpp.as_deref(),
        ) {
            (Some(local), Some(latest)) => version_lt(&local, latest),
            _ => false,
        },
        details: vec![
            format!("server configurado: {}", cfg.server_binary),
            format!("server resolvido: {resolved_binary}"),
            format!("base_url configurado: {}", cfg.base_url),
            format!("modelos GGUF detectados: {}", models.len()),
        ],
        models,
    }
}

fn choose_backend(
    system: &SystemInfo,
    ollama: &ComponentStatus,
    llamacpp: &ComponentStatus,
    mlx: &ComponentStatus,
) -> ActiveBackend {
    if mlx.healthy && system.os == "macos" && system.architecture == "aarch64" {
        return ActiveBackend {
            provider: "mlx".to_string(),
            execution_backend: "metal".to_string(),
            reason: "MLX esta saudavel num host Apple Silicon, portanto recebe prioridade maxima."
                .to_string(),
            fallback_chain: vec![
                "mlx".to_string(),
                "llama.cpp".to_string(),
                "ollama".to_string(),
                "cpu".to_string(),
            ],
        };
    }

    let ollama_has_usable_models = ollama.healthy && !ollama.models.is_empty();
    let prefer_ollama = ollama_has_usable_models && llamacpp.models.is_empty();
    if llamacpp.healthy && !prefer_ollama {
        let path = llamacpp.configured_path.as_deref().unwrap_or("");
        let segments = path.split(['/', '\\']).collect::<Vec<_>>();
        let variant = segments
            .windows(2)
            .find(|pair| pair[0].eq_ignore_ascii_case("engine"))
            .map(|pair| pair[1].to_ascii_lowercase());
        let exec = match variant.as_deref() {
            Some("vulkan") => "vulkan",
            Some("cuda") => "cuda",
            Some("cpu") => "cpu",
            _ => "unverified",
        };
        return ActiveBackend {
            provider: "llama.cpp".to_string(),
            execution_backend: exec.to_string(),
            reason: "llama.cpp está instalado. A variante identifica o executável selecionado; o uso da GPU requer validação com um modelo.".to_string(),
            fallback_chain: vec![
                "llama.cpp".to_string(),
                "ollama".to_string(),
                "cpu".to_string(),
            ],
        };
    }

    if ollama.healthy {
        return ActiveBackend {
            provider: "ollama".to_string(),
            execution_backend: "managed".to_string(),
            reason: "Ollama e o unico backend local saudavel detectado neste host; ele assume o modo estavel por padrao.".to_string(),
            fallback_chain: vec!["ollama".to_string(), "cpu".to_string()],
        };
    }

    ActiveBackend {
        provider: "cpu".to_string(),
        execution_backend: "cpu".to_string(),
        reason: "nenhum backend GPU/local ficou saudavel apos os probes".to_string(),
        fallback_chain: vec!["cpu".to_string()],
    }
}

async fn validate_active_backend(
    state: &crate::AppState,
    active: &ActiveBackend,
    ollama_models: &[ModelSummary],
    preferred_model: Option<&str>,
) -> ValidationReport {
    match active.provider.as_str() {
        "ollama" => {
            let model_id = select_validation_model(ollama_models, preferred_model);
            let Some(model_id) = model_id else {
                return ValidationReport {
                    attempted: false,
                    success: false,
                    provider: Some("ollama".to_string()),
                    model_id: None,
                    latency_ms: None,
                    output_preview: None,
                    error: Some("nenhum modelo elegivel encontrado no Ollama".to_string()),
                };
            };

            let started = Instant::now();
            match validate_ollama_http(&state.ollama_provider.config().base_url, &model_id).await {
                Ok(response) => ValidationReport {
                    attempted: true,
                    success: true,
                    provider: Some("ollama".to_string()),
                    model_id: Some(model_id),
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    output_preview: Some(truncate_preview(&response)),
                    error: None,
                },
                Err(error) => ValidationReport {
                    attempted: true,
                    success: false,
                    provider: Some("ollama".to_string()),
                    model_id: Some(model_id),
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    output_preview: None,
                    error: Some(error.to_string()),
                },
            }
        }
        other => ValidationReport {
            attempted: false,
            success: false,
            provider: Some(other.to_string()),
            model_id: None,
            latency_ms: None,
            output_preview: None,
            error: Some(
                "smoke test automatico implementado neste ciclo apenas para o backend Ollama"
                    .to_string(),
            ),
        },
    }
}

fn select_validation_model(models: &[ModelSummary], preferred: Option<&str>) -> Option<String> {
    if let Some(preferred) = preferred {
        if models.iter().any(|model| model.id == preferred) {
            return Some(preferred.to_string());
        }
    }

    models
        .iter()
        .find(|model| {
            let lower = model.id.to_ascii_lowercase();
            !lower.contains("embed") && !lower.contains("vision") && !lower.contains("-vl")
        })
        .map(|model| model.id.clone())
}

async fn detect_os_version() -> Option<String> {
    if cfg!(target_os = "windows") {
        return first_line(&run_capture("cmd", &["/C", "ver"], 5).await.stdout);
    }
    if cfg!(target_os = "macos") {
        return first_line(&run_capture("sw_vers", &["-productVersion"], 5).await.stdout);
    }
    if cfg!(target_os = "linux") {
        return first_line(&run_capture("uname", &["-r"], 5).await.stdout);
    }
    None
}

async fn detect_gpus() -> Vec<GpuInfo> {
    let mut gpus = Vec::new();
    if cfg!(target_os = "macos") {
        gpus.push(GpuInfo {
            backend: "metal".to_string(),
            name: "Apple Metal".to_string(),
            details: None,
        });
    }

    let nvidia = run_capture(
        "nvidia-smi",
        &[
            "--query-gpu=name,driver_version,memory.total",
            "--format=csv,noheader",
        ],
        8,
    )
    .await;
    if nvidia.success {
        for line in nvidia
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let mut parts = line.split(',').map(|segment| segment.trim());
            let name = parts.next().unwrap_or("NVIDIA GPU").to_string();
            let driver = parts.next().unwrap_or_default();
            let memory = parts.next().unwrap_or_default();
            gpus.push(GpuInfo {
                backend: "cuda".to_string(),
                name,
                details: Some(format!("driver={driver}; memory={memory}")),
            });
        }
    }

    if gpus.is_empty() {
        gpus.push(GpuInfo {
            backend: "none".to_string(),
            name: "CPU only".to_string(),
            details: None,
        });
    }

    gpus
}

fn backend_priority(mlx_supported: bool) -> Vec<String> {
    let mut priority = Vec::new();
    if mlx_supported && cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        priority.push("mlx".to_string());
    }
    priority.push("llama.cpp".to_string());
    priority.push("ollama".to_string());
    priority.push("cpu".to_string());
    priority
}

async fn fetch_latest_versions() -> LatestVersions {
    let client = match build_http_client() {
        Some(client) => client,
        None => return LatestVersions::default(),
    };

    let ollama = fetch_github_tag(
        &client,
        "https://api.github.com/repos/ollama/ollama/releases/latest",
    )
    .await;
    let mlx = fetch_pypi_version(&client, "https://pypi.org/pypi/mlx/json").await;
    let mlx_lm = fetch_pypi_version(&client, "https://pypi.org/pypi/mlx-lm/json").await;
    let llamacpp = fetch_github_tag(
        &client,
        "https://api.github.com/repos/ggml-org/llama.cpp/releases/latest",
    )
    .await;

    LatestVersions {
        ollama,
        mlx,
        mlx_lm,
        llamacpp,
    }
}

fn build_http_client() -> Option<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("mlx-pilot-runtime-doctor/0.1"),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(12))
        .build()
        .ok()
}

async fn fetch_github_tag(client: &reqwest::Client, url: &str) -> Option<String> {
    let value = client
        .get(url)
        .send()
        .await
        .ok()?
        .json::<Value>()
        .await
        .ok()?;
    value
        .get("tag_name")
        .and_then(Value::as_str)
        .map(|value| value.trim_start_matches('v').to_string())
}

async fn fetch_pypi_version(client: &reqwest::Client, url: &str) -> Option<String> {
    let value = client
        .get(url)
        .send()
        .await
        .ok()?
        .json::<Value>()
        .await
        .ok()?;
    value
        .get("info")
        .and_then(|value| value.get("version"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

async fn fetch_ollama_api_version(base_url: &str) -> Option<String> {
    let client = build_http_client()?;
    let endpoint = format!("{}/api/version", base_url.trim_end_matches('/'));
    client
        .get(endpoint)
        .send()
        .await
        .ok()?
        .json::<Value>()
        .await
        .ok()?
        .get("version")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

async fn run_capture(program: &str, args: &[&str], timeout_secs: u64) -> ProbeResult {
    if program.trim().is_empty() {
        return ProbeResult::default();
    }

    let mut command = Command::new(program);
    crate::silence_console(&mut command);
    command
        .args(args)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = match timeout(Duration::from_secs(timeout_secs), command.output()).await {
        Ok(Ok(output)) => output,
        _ => return ProbeResult::default(),
    };

    ProbeResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn first_line(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

fn version_lt(local: &str, latest: &str) -> bool {
    let local = numeric_segments(local);
    let latest = numeric_segments(latest);
    if local.is_empty() || latest.is_empty() {
        return false;
    }

    let max_len = local.len().max(latest.len());
    for idx in 0..max_len {
        let left = *local.get(idx).unwrap_or(&0);
        let right = *latest.get(idx).unwrap_or(&0);
        if left < right {
            return true;
        }
        if left > right {
            return false;
        }
    }
    false
}

fn numeric_segments(raw: &str) -> Vec<u32> {
    raw.trim()
        .trim_start_matches('v')
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|segment| !segment.is_empty())
        .filter_map(|segment| segment.parse::<u32>().ok())
        .collect()
}

fn truncate_preview(raw: &str) -> String {
    let trimmed = raw.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() <= 80 {
        return trimmed.to_string();
    }
    chars.into_iter().take(80).collect::<String>() + "..."
}

async fn persist_report(report: &RuntimeDoctorReport) -> Option<PathBuf> {
    let path = std::env::temp_dir().join("mlx-pilot-runtime-doctor-report.json");
    let body = serde_json::to_vec_pretty(report).ok()?;
    tokio::fs::write(&path, body).await.ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::{numeric_segments, version_lt};

    #[test]
    fn loose_version_compare_handles_semver_like_values() {
        assert!(version_lt("0.18.0", "0.18.1"));
        assert!(!version_lt("0.18.1", "0.18.0"));
        assert!(!version_lt("b123", "0.18.0"));
    }

    #[test]
    fn numeric_segment_parser_extracts_digits() {
        assert_eq!(numeric_segments("v0.18.0"), vec![0, 18, 0]);
        assert_eq!(numeric_segments("ollama version is 0.18.0"), vec![0, 18, 0]);
    }
}

#[cfg(test)]
mod observation_tests {
    use super::*;
    use axum::{
        extract::State,
        http::{Method, Uri},
        routing::any,
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    #[test]
    fn nvidia_hardware_does_not_mislabel_the_installed_vulkan_engine_as_cuda() {
        let system = SystemInfo {
            os: "windows".to_string(),
            os_version: None,
            architecture: "x86_64".to_string(),
            gpu: vec![GpuInfo {
                backend: "cuda".to_string(),
                name: "NVIDIA RTX 3060".to_string(),
                details: None,
            }],
            python_alias_healthy: false,
            backend_priority: vec![],
        };
        let unavailable = ComponentStatus {
            id: "mlx".to_string(),
            installed: false,
            healthy: false,
            configured_path: None,
            local_version: None,
            latest_version: None,
            outdated: false,
            details: vec![],
            models: vec![],
        };
        let mut engine = unavailable.clone();
        engine.id = "llama.cpp".to_string();
        engine.installed = true;
        engine.healthy = true;
        engine.configured_path = Some(
            r"C:\Users\tester\AppData\Local\MLX Pilot\engine\vulkan\llama-server.exe".to_string(),
        );
        let chosen = choose_backend(&system, &unavailable, &engine, &unavailable);
        assert_eq!(chosen.provider, "llama.cpp");
        assert_eq!(chosen.execution_backend, "vulkan");
        let mut ollama = unavailable.clone();
        ollama.id = "ollama".to_string();
        ollama.installed = true;
        ollama.healthy = true;
        ollama.models = vec![ModelSummary {
            id: "qwen3:0.6b".to_string(),
            provider: "ollama".to_string(),
        }];
        assert_eq!(
            choose_backend(&system, &ollama, &engine, &unavailable).provider,
            "ollama"
        );
        engine.models = vec![ModelSummary {
            id: "test.gguf".to_string(),
            provider: "llama.cpp".to_string(),
        }];
        assert_eq!(
            choose_backend(&system, &ollama, &engine, &unavailable).provider,
            "llama.cpp"
        );
        engine.configured_path = Some("llama-server".to_string());
        assert_eq!(
            choose_backend(&system, &unavailable, &engine, &unavailable).execution_backend,
            "unverified"
        );
        assert_eq!(
            choose_backend(&system, &engine, &unavailable, &unavailable).execution_backend,
            "managed"
        );
    }

    #[tokio::test]
    async fn observation_and_validation_only_use_the_existing_ollama_http_api() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new().fallback(any(|State(requests): State<Arc<Mutex<Vec<String>>>>, method: Method, uri: Uri| async move {
            requests.lock().unwrap().push(format!("{method} {}", uri.path()));
            Json(match uri.path() {
                "/api/version" => serde_json::json!({"version":"0.30.7"}),
                "/api/tags" => serde_json::json!({"models":[{"name":"fixture-model"}]}),
                "/api/chat" => serde_json::json!({"message":{"content":"OK"}}),
                _ => panic!("unexpected request {uri}"),
            })
        })).with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let provider =
            ollama_provider::OllamaProvider::new(ollama_provider::OllamaProviderConfig {
                base_url: base.clone(),
                auto_start: false,
                auto_install: true,
                ..Default::default()
            });
        let before = provider.runtime_status().await;
        let (component, fix, models) =
            ollama_component(&provider, &LatestVersions::default(), false).await;
        assert!(component.healthy && component.installed && fix.is_none());
        assert_eq!(models[0].id, "fixture-model");
        assert_eq!(
            validate_ollama_http(&base, &models[0].id).await.unwrap(),
            "OK"
        );
        let after = provider.runtime_status().await;
        assert_eq!(before.phase, after.phase);
        assert_eq!(before.operation_id, after.operation_id);
        assert_eq!(before.selected_model, after.selected_model);
        assert_eq!(
            *requests.lock().unwrap(),
            ["GET /api/version", "GET /api/tags", "POST /api/chat"]
        );
        server.abort();
    }
}
