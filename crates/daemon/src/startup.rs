use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use llamacpp_provider::LlamaCppProvider;
use mlx_provider::MlxProvider;
use ollama_provider::{OllamaProvider, OllamaRuntimeStatus, RuntimePhase};
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct ProviderStartupStatus {
    pub provider: String,
    pub phase: String,
    pub step: String,
    pub message: String,
    pub ready: bool,
    pub applicable: bool,
    pub error: Option<String>,
    pub executable_path: Option<String>,
    pub version: Option<String>,
    pub pid: Option<u32>,
    pub process_origin: Option<String>,
    pub external_server_detected: Option<bool>,
    pub external_server_version: Option<String>,
    pub external_server_pid: Option<u32>,
    pub system_executable_path: Option<String>,
    pub system_client_version: Option<String>,
    pub system_payload_valid: Option<bool>,
    pub gpu_expected: Option<bool>,
    pub gpu_detected: Option<bool>,
    pub gpu_name: Option<String>,
    pub vram_bytes: Option<u64>,
    pub backend: Option<String>,
}

impl ProviderStartupStatus {
    fn checking(provider: &str, message: &str) -> Self {
        Self {
            provider: provider.to_string(),
            phase: "checking".to_string(),
            step: "checking".to_string(),
            message: message.to_string(),
            ready: false,
            applicable: true,
            error: None,
            executable_path: None,
            version: None,
            pid: None,
            process_origin: None,
            external_server_detected: None,
            external_server_version: None,
            external_server_pid: None,
            system_executable_path: None,
            system_client_version: None,
            system_payload_valid: None,
            gpu_expected: None,
            gpu_detected: None,
            gpu_name: None,
            vram_bytes: None,
            backend: None,
        }
    }

    fn ready(provider: &str, message: &str, executable_path: Option<String>) -> Self {
        let mut status = Self::checking(provider, message);
        status.phase = "ready".to_string();
        status.step = "ready".to_string();
        status.ready = true;
        status.executable_path = executable_path;
        status
    }

    fn unavailable(provider: &str, error: String, applicable: bool) -> Self {
        let mut status = Self::checking(
            provider,
            if applicable {
                "Provider indisponivel"
            } else {
                "Nao aplicavel nesta plataforma"
            },
        );
        status.phase = if applicable {
            "failed".to_string()
        } else {
            "unsupported".to_string()
        };
        status.step = status.phase.clone();
        status.applicable = applicable;
        status.error = Some(error);
        status
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StartupSnapshot {
    pub phase: String,
    pub step: String,
    pub message: String,
    pub progress_percent: Option<f64>,
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub bytes_per_second: Option<u64>,
    pub can_cancel: bool,
    pub app_ready: bool,
    pub degraded: bool,
    pub operation_id: Option<String>,
    pub providers: Vec<ProviderStartupStatus>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StartupCoordinator {
    mlx: Arc<RwLock<ProviderStartupStatus>>,
    llamacpp: Arc<RwLock<ProviderStartupStatus>>,
    running: Arc<AtomicBool>,
}

impl Default for StartupCoordinator {
    fn default() -> Self {
        Self {
            mlx: Arc::new(RwLock::new(ProviderStartupStatus::checking(
                "mlx",
                "Verificando o MLX",
            ))),
            llamacpp: Arc::new(RwLock::new(ProviderStartupStatus::checking(
                "llamacpp",
                "Verificando o llama.cpp",
            ))),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl StartupCoordinator {
    pub fn start(
        &self,
        mlx: Arc<MlxProvider>,
        llamacpp: Arc<LlamaCppProvider>,
        ollama: Arc<OllamaProvider>,
        selected_ollama_model: Option<String>,
    ) -> bool {
        if self.running.swap(true, Ordering::SeqCst) {
            return false;
        }

        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator.reset_generic_states().await;
            let mlx_task = {
                let status = coordinator.mlx.clone();
                tokio::spawn(async move {
                    match mlx.prepare_runtime().await {
                        Ok(command) => {
                            *status.write().await =
                                ProviderStartupStatus::ready("mlx", "MLX pronto", Some(command));
                        }
                        Err(error) => {
                            let applicable = !cfg!(target_os = "windows");
                            *status.write().await = ProviderStartupStatus::unavailable(
                                "mlx",
                                error.to_string(),
                                applicable,
                            );
                        }
                    }
                })
            };
            let llamacpp_task = {
                let status = coordinator.llamacpp.clone();
                tokio::spawn(async move {
                    match llamacpp.prepare_runtime().await {
                        Ok(path) => {
                            info!(executable = %path, "llama.cpp runtime ready");
                            *status.write().await = ProviderStartupStatus::ready(
                                "llamacpp",
                                "llama.cpp pronto",
                                Some(path),
                            );
                        }
                        Err(error) => {
                            warn!(%error, "llama.cpp runtime unavailable");
                            *status.write().await = ProviderStartupStatus::unavailable(
                                "llamacpp",
                                error.to_string(),
                                true,
                            );
                        }
                    }
                })
            };
            let ollama_task = tokio::spawn(async move {
                if let Err(error) = ollama.prepare_runtime(selected_ollama_model).await {
                    warn!(%error, "Ollama startup failed");
                }
            });

            let _ = tokio::join!(mlx_task, llamacpp_task, ollama_task);
            coordinator.running.store(false, Ordering::SeqCst);
        });
        true
    }

    pub async fn snapshot(&self, ollama: &OllamaProvider) -> StartupSnapshot {
        let ollama_status = ollama.runtime_status().await;
        let mlx = self.mlx.read().await.clone();
        let llamacpp = self.llamacpp.read().await.clone();
        let ollama_view = ollama_provider_view(&ollama_status);
        let providers = vec![ollama_view, llamacpp, mlx];
        let applicable = providers
            .iter()
            .filter(|provider| provider.applicable)
            .collect::<Vec<_>>();
        let terminal = applicable.iter().all(|provider| {
            matches!(
                provider.phase.as_str(),
                "ready" | "failed" | "degraded" | "cancelled"
            )
        });
        let failures = applicable
            .iter()
            .filter(|provider| !provider.ready)
            .collect::<Vec<_>>();
        let first_error = failures.first().and_then(|provider| provider.error.clone());
        let any_ready = applicable.iter().any(|provider| provider.ready);
        let app_ready = terminal;
        let degraded = terminal && !failures.is_empty() && any_ready;
        let failed = terminal && !any_ready;

        let (phase, step, message) = if !terminal {
            (
                ollama_status_phase(&ollama_status),
                ollama_status.step.clone(),
                ollama_status.message.clone(),
            )
        } else if failed {
            (
                "failed".to_string(),
                "failed".to_string(),
                "Nenhum provider local esta disponivel".to_string(),
            )
        } else if degraded {
            (
                "degraded".to_string(),
                "degraded".to_string(),
                "Aplicacao pronta em modo degradado".to_string(),
            )
        } else {
            (
                "ready".to_string(),
                "ready".to_string(),
                "Pronto".to_string(),
            )
        };

        StartupSnapshot {
            phase,
            step,
            message,
            progress_percent: ollama_status.progress_percent,
            bytes_downloaded: ollama_status.bytes_downloaded,
            bytes_total: ollama_status.bytes_total,
            bytes_per_second: ollama_status.bytes_per_second,
            can_cancel: ollama_status.can_cancel,
            app_ready,
            degraded,
            operation_id: ollama_status.operation_id,
            providers,
            error: first_error,
        }
    }

    async fn reset_generic_states(&self) {
        *self.mlx.write().await = ProviderStartupStatus::checking("mlx", "Verificando o MLX");
        *self.llamacpp.write().await =
            ProviderStartupStatus::checking("llamacpp", "Verificando o llama.cpp");
    }
}

fn ollama_status_phase(status: &OllamaRuntimeStatus) -> String {
    match status.phase {
        RuntimePhase::Checking => "checking",
        RuntimePhase::Downloading => "downloading",
        RuntimePhase::Installing => "installing",
        RuntimePhase::Updating => "updating",
        RuntimePhase::Starting => "starting",
        RuntimePhase::Validating => "validating",
        RuntimePhase::Ready => "ready",
        RuntimePhase::Degraded => "degraded",
        RuntimePhase::Failed => "failed",
        RuntimePhase::Cancelled => "cancelled",
    }
    .to_string()
}

fn ollama_provider_view(status: &OllamaRuntimeStatus) -> ProviderStartupStatus {
    ProviderStartupStatus {
        provider: "ollama".to_string(),
        phase: ollama_status_phase(status),
        step: status.step.clone(),
        message: status.message.clone(),
        ready: status.provider_ready,
        applicable: true,
        error: status.error.clone(),
        executable_path: status.executable_path.clone(),
        version: status.installed_version.clone(),
        pid: status.pid,
        process_origin: Some(status.process_origin.clone()),
        external_server_detected: Some(status.external_server_detected),
        external_server_version: status.external_server_version.clone(),
        external_server_pid: status.external_server_pid,
        system_executable_path: status.system_executable_path.clone(),
        system_client_version: status.system_client_version.clone(),
        system_payload_valid: status.system_payload_valid,
        gpu_expected: Some(status.gpu.expected),
        gpu_detected: Some(status.gpu.detected),
        gpu_name: status.gpu.name.clone(),
        vram_bytes: status.gpu.vram_bytes,
        backend: status.gpu.backend.clone(),
    }
}
