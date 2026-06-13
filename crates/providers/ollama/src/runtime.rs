use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use futures_util::StreamExt;
use mlx_ollama_core::ProviderError;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};
use tokio::time::sleep;
use tracing::{info, warn};
use uuid::Uuid;

use crate::silence_console;

pub const MANAGED_OLLAMA_VERSION: &str = "0.30.7";
const MIN_OLLAMA_VERSION: &str = "0.30.0";
const MAX_OLLAMA_VERSION_EXCLUSIVE: &str = "0.31.0";
const DEFAULT_MANAGED_URL: &str = "http://127.0.0.1:11438";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePhase {
    Checking,
    Downloading,
    Installing,
    Updating,
    Starting,
    Validating,
    Ready,
    Degraded,
    Failed,
    Cancelled,
}

impl RuntimePhase {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Ready | Self::Degraded | Self::Failed | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuStatus {
    pub expected: bool,
    pub detected: bool,
    pub name: Option<String>,
    pub vram_bytes: Option<u64>,
    pub backend: Option<String>,
    pub driver_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaRuntimeStatus {
    pub provider: String,
    pub phase: RuntimePhase,
    pub step: String,
    pub message: String,
    pub progress_percent: Option<f64>,
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub bytes_per_second: Option<u64>,
    pub can_cancel: bool,
    pub provider_ready: bool,
    pub operation_id: Option<String>,
    pub executable_path: Option<String>,
    pub installed_version: Option<String>,
    pub required_version: String,
    pub compatible_range: String,
    pub pid: Option<u32>,
    pub process_origin: String,
    pub external_server_detected: bool,
    pub external_server_version: Option<String>,
    pub external_server_pid: Option<u32>,
    pub gpu: GpuStatus,
    pub selected_model: Option<String>,
    pub model_available: Option<bool>,
    pub recovery_attempted: bool,
    pub error: Option<String>,
    pub log_path: Option<String>,
    pub updated_at_epoch_ms: u128,
}

impl Default for OllamaRuntimeStatus {
    fn default() -> Self {
        Self {
            provider: "ollama".to_string(),
            phase: RuntimePhase::Checking,
            step: "checking".to_string(),
            message: "Verificando o Ollama".to_string(),
            progress_percent: None,
            bytes_downloaded: 0,
            bytes_total: None,
            bytes_per_second: None,
            can_cancel: false,
            provider_ready: false,
            operation_id: None,
            executable_path: None,
            installed_version: None,
            required_version: MANAGED_OLLAMA_VERSION.to_string(),
            compatible_range: format!(">={MIN_OLLAMA_VERSION}, <{MAX_OLLAMA_VERSION_EXCLUSIVE}"),
            pid: None,
            process_origin: "none".to_string(),
            external_server_detected: false,
            external_server_version: None,
            external_server_pid: None,
            gpu: GpuStatus::default(),
            selected_model: None,
            model_available: None,
            recovery_attempted: false,
            error: None,
            log_path: None,
            updated_at_epoch_ms: epoch_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveRuntimeManifest {
    schema_version: u32,
    version: String,
    executable_relative_path: String,
    executable_sha256: String,
    source_url: String,
    installed_at_epoch_ms: u128,
    validated_at_epoch_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProcessMarker {
    pid: u32,
    executable_path: String,
    base_url: String,
    owner_pid: u32,
    started_at_epoch_ms: u128,
}

#[derive(Debug)]
struct ManagedProcess {
    child: Child,
    executable: PathBuf,
    #[cfg(windows)]
    _job: Option<WindowsJob>,
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsJob(usize);

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(
                    self.0 as windows_sys::Win32::Foundation::HANDLE,
                );
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaRuntimeConfig {
    pub root: PathBuf,
    pub base_url: String,
    pub models_dir: PathBuf,
    pub startup_timeout: Duration,
    pub auto_install: bool,
    pub auto_start: bool,
}

impl OllamaRuntimeConfig {
    pub fn from_provider(
        base_url: String,
        startup_timeout: Duration,
        auto_install: bool,
        auto_start: bool,
    ) -> Self {
        let root = runtime_root();
        let models_dir = std::env::var("OLLAMA_MODELS")
            .ok()
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| root.join("models"));
        Self {
            root,
            base_url: if base_url.trim().is_empty() {
                DEFAULT_MANAGED_URL.to_string()
            } else {
                base_url
            },
            models_dir,
            startup_timeout,
            auto_install,
            auto_start,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaRuntime {
    cfg: OllamaRuntimeConfig,
    client: reqwest::Client,
    status: Arc<RwLock<OllamaRuntimeStatus>>,
    operation_lock: Arc<Mutex<()>>,
    cancel: Arc<AtomicBool>,
    process: Arc<Mutex<Option<ManagedProcess>>>,
}

impl OllamaRuntime {
    pub fn new(cfg: OllamaRuntimeConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            cfg,
            client,
            status: Arc::new(RwLock::new(OllamaRuntimeStatus::default())),
            operation_lock: Arc::new(Mutex::new(())),
            cancel: Arc::new(AtomicBool::new(false)),
            process: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn status(&self) -> OllamaRuntimeStatus {
        self.status.read().await.clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub async fn diagnostic_detail(&self, response_detail: &str) -> String {
        let status = self.status().await;
        let log_tail = status
            .log_path
            .as_deref()
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|content| {
                content
                    .lines()
                    .rev()
                    .take(20)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default();
        format!(
            "runner Ollama falhou. resposta_http={}; executable={}; pid={}; gpu_expected={}; gpu_detected={}; backend={}; log_tail={}",
            response_detail.trim(),
            status.executable_path.as_deref().unwrap_or("unknown"),
            status.pid.map(|value| value.to_string()).unwrap_or_else(|| "unknown".to_string()),
            status.gpu.expected,
            status.gpu.detected,
            status.gpu.backend.as_deref().unwrap_or("unknown"),
            if log_tail.is_empty() { "unavailable" } else { &log_tail },
        )
    }

    pub async fn prepare(&self, selected_model: Option<String>) -> Result<(), ProviderError> {
        let _operation = self.operation_lock.lock().await;
        self.cancel.store(false, Ordering::SeqCst);
        self.reset_status(selected_model.clone()).await;

        let result = self.prepare_inner(selected_model.as_deref()).await;
        if let Err(error) = &result {
            let cancelled = self.cancel.load(Ordering::SeqCst);
            self.update_status(|status| {
                status.phase = if cancelled {
                    RuntimePhase::Cancelled
                } else {
                    RuntimePhase::Failed
                };
                status.step = if cancelled {
                    "cancelled".to_string()
                } else {
                    "failed".to_string()
                };
                status.message = if cancelled {
                    "Operacao cancelada com seguranca".to_string()
                } else {
                    "Ollama indisponivel".to_string()
                };
                status.can_cancel = false;
                status.provider_ready = false;
                status.error = Some(error.to_string());
            })
            .await;
        }
        result
    }

    pub async fn ensure_ready_for_model(&self, model: Option<&str>) -> Result<(), ProviderError> {
        let status = self.status().await;
        if status.provider_ready {
            if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
                if status.selected_model.as_deref() != Some(model)
                    || status.model_available != Some(true)
                {
                    return self.validate_model(model).await;
                }
            }
            return Ok(());
        }
        self.prepare(model.map(ToString::to_string)).await
    }

    async fn prepare_inner(&self, selected_model: Option<&str>) -> Result<(), ProviderError> {
        fs::create_dir_all(&self.cfg.root).map_err(|source| ProviderError::Io {
            context: format!("criando runtime Ollama em {}", self.cfg.root.display()),
            source,
        })?;
        fs::create_dir_all(&self.cfg.models_dir).map_err(|source| ProviderError::Io {
            context: format!(
                "criando diretorio persistente de modelos {}",
                self.cfg.models_dir.display()
            ),
            source,
        })?;

        let gpu = discover_nvidia_gpu().await;
        let external = detect_external_ollama(&self.client, &self.cfg.base_url).await;
        self.update_status(|status| {
            status.gpu = gpu.clone();
            status.external_server_detected = external.detected;
            status.external_server_version = external.version.clone();
            status.external_server_pid = external.pid;
            status.step = "checking".to_string();
            status.message = "Verificando instalacao e processos do Ollama".to_string();
        })
        .await;
        info!(
            gpu_expected = gpu.expected,
            gpu_name = gpu.name.as_deref().unwrap_or("none"),
            vram_bytes = gpu.vram_bytes.unwrap_or_default(),
            external_server_detected = external.detected,
            external_server_pid = external.pid.unwrap_or_default(),
            external_server_version = external.version.as_deref().unwrap_or("none"),
            "Ollama runtime discovery completed"
        );

        let executable = match self.valid_active_runtime().await {
            Ok(Some(runtime)) => runtime,
            Ok(None) if self.cfg.auto_install => self.install_or_update().await?,
            Ok(None) => {
                return Err(ProviderError::Unavailable {
                    details: "runtime Ollama gerenciado ausente e instalacao automatica desativada"
                        .to_string(),
                })
            }
            Err(error) if self.cfg.auto_install => {
                warn!("runtime Ollama ativo invalido, preparando substituto: {error}");
                self.install_or_update().await?
            }
            Err(error) => return Err(error),
        };

        if !self.cfg.auto_start {
            return Err(ProviderError::Unavailable {
                details: "runtime Ollama pronto, mas inicializacao automatica esta desativada"
                    .to_string(),
            });
        }

        self.start_and_validate(&executable, selected_model, false)
            .await
    }

    async fn reset_status(&self, selected_model: Option<String>) {
        let status = OllamaRuntimeStatus {
            operation_id: Some(Uuid::new_v4().to_string()),
            selected_model,
            ..Default::default()
        };
        *self.status.write().await = status.clone();
        let _ = write_json_atomic(&self.cfg.root.join("runtime-state.json"), &status);
    }

    async fn update_status(&self, update: impl FnOnce(&mut OllamaRuntimeStatus)) {
        let mut status = self.status.write().await;
        update(&mut status);
        status.updated_at_epoch_ms = epoch_ms();
        let persisted = status.clone();
        drop(status);
        let _ = write_json_atomic(&self.cfg.root.join("runtime-state.json"), &persisted);
    }

    async fn valid_active_runtime(&self) -> Result<Option<PathBuf>, ProviderError> {
        let path = self.cfg.root.join("active.json");
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path).map_err(|source| ProviderError::Io {
            context: format!("lendo {}", path.display()),
            source,
        })?;
        let manifest: ActiveRuntimeManifest =
            serde_json::from_slice(&bytes).map_err(|error| ProviderError::Unavailable {
                details: format!("manifesto do runtime Ollama invalido: {error}"),
            })?;
        if !version_is_compatible(&manifest.version) {
            return Ok(None);
        }
        let executable = self
            .cfg
            .root
            .join("versions")
            .join(&manifest.version)
            .join(&manifest.executable_relative_path);
        validate_runtime_payload(&executable)?;
        let hash = sha256_file(&executable)?;
        if hash != manifest.executable_sha256 {
            return Err(ProviderError::Unavailable {
                details: format!(
                    "integridade do Ollama falhou em {}: hash divergente",
                    executable.display()
                ),
            });
        }
        let detected_version = executable_version(&executable).await?;
        if detected_version != manifest.version {
            return Err(ProviderError::Unavailable {
                details: format!(
                    "versao do executavel ({detected_version}) difere do manifesto ({})",
                    manifest.version
                ),
            });
        }
        self.update_status(|status| {
            status.executable_path = Some(executable.display().to_string());
            status.installed_version = Some(manifest.version.clone());
        })
        .await;
        Ok(Some(executable))
    }

    async fn install_or_update(&self) -> Result<PathBuf, ProviderError> {
        let lock_path = self.cfg.root.join("install.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|source| ProviderError::Io {
                context: format!("abrindo lock {}", lock_path.display()),
                source,
            })?;
        lock.try_lock_exclusive()
            .map_err(|_| ProviderError::Unavailable {
                details: "outra instalacao do runtime Ollama esta em andamento".to_string(),
            })?;

        match self.valid_active_runtime().await {
            Ok(Some(executable)) => {
                let _ = lock.unlock();
                return Ok(executable);
            }
            Ok(None) => {}
            Err(error) => {
                warn!(%error, "ignoring invalid active Ollama manifest while preparing replacement");
            }
        }

        let url = std::env::var("APP_OLLAMA_RUNTIME_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "https://github.com/ollama/ollama/releases/download/v{0}/ollama-windows-amd64.zip",
                    MANAGED_OLLAMA_VERSION
                )
            });
        let staging = self.cfg.root.join("staging").join(format!(
            "{}-{}",
            MANAGED_OLLAMA_VERSION,
            Uuid::new_v4()
        ));
        fs::create_dir_all(&staging).map_err(|source| ProviderError::Io {
            context: format!("criando staging {}", staging.display()),
            source,
        })?;
        let archive_path = staging.join("ollama.zip.part");

        if let Err(error) = self.download_archive(&url, &archive_path).await {
            let _ = fs::remove_dir_all(&staging);
            let _ = lock.unlock();
            return Err(error);
        }
        self.ensure_not_cancelled()?;
        self.update_status(|status| {
            status.phase = RuntimePhase::Installing;
            status.step = "installing".to_string();
            status.message = "Instalando o motor local".to_string();
            status.can_cancel = true;
            status.progress_percent = None;
        })
        .await;

        let extracted = staging.join("payload");
        let archive_for_extract = archive_path.clone();
        let extracted_for_task = extracted.clone();
        let extraction = tokio::task::spawn_blocking(move || {
            extract_zip(&archive_for_extract, &extracted_for_task)
        })
        .await
        .map_err(|error| ProviderError::Unavailable {
            details: format!("falha aguardando extracao do runtime Ollama: {error}"),
        })?;
        if let Err(error) = extraction {
            let _ = fs::remove_dir_all(&staging);
            let _ = lock.unlock();
            return Err(error);
        }
        if let Err(error) = self.ensure_not_cancelled() {
            let _ = fs::remove_dir_all(&staging);
            let _ = lock.unlock();
            return Err(error);
        }
        let staged_executable = match find_file_recursive(&extracted, executable_name()) {
            Some(path) => path,
            None => {
                let _ = fs::remove_dir_all(&staging);
                let _ = lock.unlock();
                return Err(ProviderError::Unavailable {
                    details: "pacote do Ollama nao contem o executavel esperado".to_string(),
                });
            }
        };
        if let Err(error) = validate_runtime_payload(&staged_executable) {
            let _ = fs::remove_dir_all(&staging);
            let _ = lock.unlock();
            return Err(error);
        }

        let installed_version = executable_version(&staged_executable).await?;
        if installed_version != MANAGED_OLLAMA_VERSION {
            let _ = fs::remove_dir_all(&staging);
            let _ = lock.unlock();
            return Err(ProviderError::Unavailable {
                details: format!(
                    "pacote baixado possui Ollama {installed_version}; esperado {MANAGED_OLLAMA_VERSION}"
                ),
            });
        }

        let relative = staged_executable
            .strip_prefix(&extracted)
            .map_err(|_| ProviderError::Unavailable {
                details: "layout interno do pacote Ollama invalido".to_string(),
            })?
            .to_path_buf();
        let executable_hash = sha256_file(&staged_executable)?;
        let versions = self.cfg.root.join("versions");
        fs::create_dir_all(&versions).map_err(|source| ProviderError::Io {
            context: format!("criando {}", versions.display()),
            source,
        })?;
        let final_dir = versions.join(MANAGED_OLLAMA_VERSION);
        if final_dir.exists() {
            let quarantine =
                versions.join(format!("{}.invalid-{}", MANAGED_OLLAMA_VERSION, epoch_ms()));
            fs::rename(&final_dir, &quarantine).map_err(|source| ProviderError::Io {
                context: format!("isolando runtime invalido {}", final_dir.display()),
                source,
            })?;
        }
        fs::rename(&extracted, &final_dir).map_err(|source| ProviderError::Io {
            context: format!("ativando runtime {}", final_dir.display()),
            source,
        })?;

        let manifest = ActiveRuntimeManifest {
            schema_version: 1,
            version: installed_version.clone(),
            executable_relative_path: relative.display().to_string(),
            executable_sha256: executable_hash,
            source_url: url,
            installed_at_epoch_ms: epoch_ms(),
            validated_at_epoch_ms: epoch_ms(),
        };
        write_json_atomic(&self.cfg.root.join("active.json"), &manifest)?;
        let _ = fs::remove_dir_all(&staging);
        let _ = lock.unlock();

        let executable = final_dir.join(relative);
        self.update_status(|status| {
            status.executable_path = Some(executable.display().to_string());
            status.installed_version = Some(installed_version);
            status.can_cancel = false;
            status.progress_percent = Some(100.0);
        })
        .await;
        Ok(executable)
    }

    async fn download_archive(&self, url: &str, destination: &Path) -> Result<(), ProviderError> {
        self.update_status(|status| {
            status.phase = RuntimePhase::Downloading;
            status.step = "downloading".to_string();
            status.message = "Atualizando o motor local".to_string();
            status.can_cancel = true;
            status.progress_percent = None;
            status.bytes_downloaded = 0;
            status.bytes_total = None;
            status.bytes_per_second = None;
        })
        .await;

        let response =
            self.client
                .get(url)
                .send()
                .await
                .map_err(|error| ProviderError::Unavailable {
                    details: format!("falha baixando runtime Ollama: {error}"),
                })?;
        if !response.status().is_success() {
            return Err(ProviderError::Unavailable {
                details: format!(
                    "download do runtime Ollama retornou HTTP {}",
                    response.status()
                ),
            });
        }
        let total = response.content_length();
        let mut stream = response.bytes_stream();
        let mut file = tokio::fs::File::create(destination)
            .await
            .map_err(|source| ProviderError::Io {
                context: format!("criando {}", destination.display()),
                source,
            })?;
        let started = Instant::now();
        let mut downloaded = 0_u64;
        while let Some(chunk) = stream.next().await {
            self.ensure_not_cancelled()?;
            let chunk = chunk.map_err(|error| ProviderError::Unavailable {
                details: format!("download do runtime Ollama foi interrompido: {error}"),
            })?;
            file.write_all(&chunk)
                .await
                .map_err(|source| ProviderError::Io {
                    context: format!("escrevendo {}", destination.display()),
                    source,
                })?;
            downloaded += chunk.len() as u64;
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            self.update_status(|status| {
                status.bytes_downloaded = downloaded;
                status.bytes_total = total;
                status.bytes_per_second = Some((downloaded as f64 / elapsed) as u64);
                status.progress_percent =
                    total.map(|value| (downloaded as f64 / value as f64 * 100.0).min(100.0));
            })
            .await;
        }
        file.flush().await.map_err(|source| ProviderError::Io {
            context: format!("finalizando {}", destination.display()),
            source,
        })?;
        Ok(())
    }

    async fn start_and_validate(
        &self,
        executable: &Path,
        selected_model: Option<&str>,
        recovery: bool,
    ) -> Result<(), ProviderError> {
        self.ensure_not_cancelled()?;
        self.stop_owned_process().await;
        self.resolve_port_conflict(executable).await?;

        let log_path = self.cfg.root.join("logs").join("ollama.log");
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ProviderError::Io {
                context: format!("criando {}", parent.display()),
                source,
            })?;
        }
        let stdout = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .map_err(|source| ProviderError::Io {
                context: format!("abrindo {}", log_path.display()),
                source,
            })?;
        let stderr = stdout.try_clone().map_err(|source| ProviderError::Io {
            context: format!("duplicando handle de {}", log_path.display()),
            source,
        })?;

        self.update_status(|status| {
            status.phase = RuntimePhase::Starting;
            status.step = "starting".to_string();
            status.message = "Iniciando o servidor".to_string();
            status.can_cancel = false;
            status.log_path = Some(log_path.display().to_string());
            status.recovery_attempted = recovery;
        })
        .await;

        let mut command = Command::new(executable);
        silence_console(&mut command);
        command
            .arg("serve")
            .current_dir(executable.parent().unwrap_or_else(|| Path::new(".")))
            .env("OLLAMA_HOST", host_without_scheme(&self.cfg.base_url))
            .env("OLLAMA_MODELS", &self.cfg.models_dir)
            .env("OLLAMA_DEBUG", "INFO")
            .env_remove("GPU_DEVICE_ORDINAL")
            .env_remove("HIP_VISIBLE_DEVICES")
            .env_remove("ROCR_VISIBLE_DEVICES")
            .env_remove("GGML_VK_VISIBLE_DEVICES")
            .env_remove("OLLAMA_LLM_LIBRARY")
            .env_remove("OLLAMA_VULKAN")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);
        if recovery {
            command
                .env("CUDA_VISIBLE_DEVICES", "0")
                .env("OLLAMA_LLM_LIBRARY", "cuda_v13")
                .env("OLLAMA_VULKAN", "0");
        } else {
            command.env_remove("CUDA_VISIBLE_DEVICES");
        }

        let child = command.spawn().map_err(|source| ProviderError::Io {
            context: format!("iniciando Ollama gerenciado em {}", executable.display()),
            source,
        })?;
        let pid = child.id().ok_or_else(|| ProviderError::Unavailable {
            details: "processo Ollama iniciou sem PID observavel".to_string(),
        })?;
        #[cfg(windows)]
        let job = match attach_kill_on_close_job(&child) {
            Ok(job) => Some(job),
            Err(error) => {
                warn!(%error, pid, "failed to attach Ollama to Windows kill-on-close job");
                None
            }
        };
        *self.process.lock().await = Some(ManagedProcess {
            child,
            executable: executable.to_path_buf(),
            #[cfg(windows)]
            _job: job,
        });
        let marker = ProcessMarker {
            pid,
            executable_path: executable.display().to_string(),
            base_url: self.cfg.base_url.clone(),
            owner_pid: std::process::id(),
            started_at_epoch_ms: epoch_ms(),
        };
        write_json_atomic(&self.cfg.root.join("process.json"), &marker)?;
        info!(
            executable = %executable.display(),
            pid,
            process_origin = "managed",
            base_url = %self.cfg.base_url,
            models_dir = %self.cfg.models_dir.display(),
            recovery,
            "started managed Ollama process"
        );
        self.update_status(|status| {
            status.pid = Some(pid);
            status.process_origin = "managed".to_string();
            status.executable_path = Some(executable.display().to_string());
        })
        .await;

        self.wait_for_api().await?;
        self.update_status(|status| {
            status.phase = RuntimePhase::Validating;
            status.step = "validating_gpu".to_string();
            status.message = "Verificando a GPU".to_string();
        })
        .await;

        let gpu_expected = self.status.read().await.gpu.expected;
        let log_probe = wait_for_compute_backend(&log_path, Duration::from_secs(12)).await;
        if let Some((backend, name, vram)) = log_probe {
            self.update_status(|status| {
                status.gpu.backend = Some(backend.clone());
                status.gpu.detected = is_accelerated_backend(&backend);
                if status.gpu.name.is_none() {
                    status.gpu.name = name;
                }
                if status.gpu.vram_bytes.is_none() {
                    status.gpu.vram_bytes = vram;
                }
            })
            .await;
        }

        let gpu_detected = self.status.read().await.gpu.detected;
        if gpu_expected && !gpu_detected && !recovery {
            warn!(
                executable = %executable.display(),
                pid,
                "Ollama iniciou sem detectar a GPU esperada; executando unica recuperacao"
            );
            self.update_status(|status| {
                status.message =
                    "GPU nao detectada; reiniciando o servidor com ambiente corrigido".to_string();
                status.recovery_attempted = true;
            })
            .await;
            return Box::pin(self.start_and_validate(executable, selected_model, true)).await;
        }
        if gpu_expected && !gpu_detected {
            return Err(ProviderError::Unavailable {
                details: gpu_failure_diagnostic(&log_path, executable),
            });
        }

        if let Some(model) = selected_model.filter(|value| !value.trim().is_empty()) {
            self.validate_model(model).await?;
        }

        let version = api_version(&self.client, &self.cfg.base_url).await?;
        self.persist_last_validation().await;
        self.update_status(|status| {
            status.phase = RuntimePhase::Ready;
            status.step = "ready".to_string();
            status.message = "Pronto".to_string();
            status.provider_ready = true;
            status.can_cancel = false;
            status.error = None;
            status.installed_version = Some(version);
        })
        .await;
        Ok(())
    }

    async fn persist_last_validation(&self) {
        let path = self.cfg.root.join("active.json");
        let Some(mut manifest) = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ActiveRuntimeManifest>(&bytes).ok())
        else {
            return;
        };
        manifest.validated_at_epoch_ms = epoch_ms();
        if let Err(error) = write_json_atomic(&path, &manifest) {
            warn!(%error, "failed to persist Ollama validation timestamp");
        }
    }

    async fn validate_model(&self, model: &str) -> Result<(), ProviderError> {
        self.update_status(|status| {
            status.phase = RuntimePhase::Validating;
            status.step = "validating_model".to_string();
            status.message = "Validando o modelo".to_string();
            status.selected_model = Some(model.to_string());
            status.model_available = None;
        })
        .await;
        let tags_url = format!("{}/api/tags", self.cfg.base_url.trim_end_matches('/'));
        let tags: serde_json::Value = self
            .client
            .get(tags_url)
            .send()
            .await
            .map_err(network_error)?
            .json()
            .await
            .map_err(network_error)?;
        let available = tags
            .get("models")
            .and_then(serde_json::Value::as_array)
            .map(|models| {
                models.iter().any(|entry| {
                    entry
                        .get("model")
                        .or_else(|| entry.get("name"))
                        .and_then(serde_json::Value::as_str)
                        .map(|value| value == model)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        self.update_status(|status| status.model_available = Some(available))
            .await;
        if !available {
            return Err(ProviderError::ModelNotFound {
                model_id: model.to_string(),
            });
        }

        let generate_url = format!("{}/api/generate", self.cfg.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(generate_url)
            .json(&serde_json::json!({
                "model": model,
                "prompt": "",
                "stream": false,
                "keep_alive": "5m",
                "options": { "num_predict": 1 }
            }))
            .send()
            .await
            .map_err(network_error)?;
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().await.unwrap_or_default();
            return Err(ProviderError::Unavailable {
                details: format!(
                    "runner Ollama falhou ao validar '{model}' (HTTP {status}): {}",
                    detail.trim()
                ),
            });
        }

        if self.status.read().await.gpu.expected {
            let ps_url = format!("{}/api/ps", self.cfg.base_url.trim_end_matches('/'));
            let ps: serde_json::Value = self
                .client
                .get(ps_url)
                .send()
                .await
                .map_err(network_error)?
                .json()
                .await
                .map_err(network_error)?;
            let size_vram = ps
                .get("models")
                .and_then(serde_json::Value::as_array)
                .and_then(|models| {
                    models.iter().find(|entry| {
                        entry
                            .get("name")
                            .or_else(|| entry.get("model"))
                            .and_then(serde_json::Value::as_str)
                            .map(|value| value == model)
                            .unwrap_or(false)
                    })
                })
                .and_then(|entry| entry.get("size_vram"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if size_vram == 0 {
                return Err(ProviderError::Unavailable {
                    details: format!(
                        "modelo '{model}' seria executado somente em CPU, embora uma GPU NVIDIA compativel tenha sido detectada; operacao bloqueada"
                    ),
                });
            }
        }
        self.update_status(|status| {
            status.model_available = Some(true);
            status.provider_ready = true;
            status.phase = RuntimePhase::Ready;
            status.step = "ready".to_string();
            status.message = "Pronto".to_string();
        })
        .await;
        Ok(())
    }

    async fn wait_for_api(&self) -> Result<(), ProviderError> {
        let started = Instant::now();
        let timeout = self.cfg.startup_timeout.max(Duration::from_secs(5));
        loop {
            self.ensure_not_cancelled()?;
            if let Some(process) = self.process.lock().await.as_mut() {
                if let Some(status) =
                    process
                        .child
                        .try_wait()
                        .map_err(|source| ProviderError::Io {
                            context: "verificando processo Ollama".to_string(),
                            source,
                        })?
                {
                    return Err(ProviderError::Unavailable {
                        details: format!("processo Ollama encerrou durante o startup: {status}"),
                    });
                }
            }
            if api_version(&self.client, &self.cfg.base_url).await.is_ok() {
                return Ok(());
            }
            if started.elapsed() >= timeout {
                return Err(ProviderError::Unavailable {
                    details: format!("API do Ollama nao ficou pronta em {}s", timeout.as_secs()),
                });
            }
            sleep(Duration::from_millis(350)).await;
        }
    }

    async fn resolve_port_conflict(&self, executable: &Path) -> Result<(), ProviderError> {
        if api_version(&self.client, &self.cfg.base_url).await.is_err() {
            return Ok(());
        }
        let marker_path = self.cfg.root.join("process.json");
        let marker = fs::read(&marker_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ProcessMarker>(&bytes).ok());
        if let Some(marker) = marker {
            if paths_equal(Path::new(&marker.executable_path), executable)
                && process_executable_matches(marker.pid, executable).await
            {
                warn!(
                    pid = marker.pid,
                    "encerrando processo Ollama obsoleto pertencente ao MLX Pilot"
                );
                terminate_pid(marker.pid).await?;
                for _ in 0..20 {
                    if api_version(&self.client, &self.cfg.base_url).await.is_err() {
                        return Ok(());
                    }
                    sleep(Duration::from_millis(150)).await;
                }
            }
        }
        Err(ProviderError::Unavailable {
            details: format!(
                "a porta gerenciada {} ja esta ocupada por um servidor que nao pertence ao processo atual; nenhum processo externo foi encerrado",
                self.cfg.base_url
            ),
        })
    }

    async fn stop_owned_process(&self) {
        let mut process = self.process.lock().await;
        if let Some(managed) = process.as_mut() {
            info!(
                executable = %managed.executable.display(),
                pid = managed.child.id().unwrap_or_default(),
                "encerrando processo Ollama pertencente ao MLX Pilot"
            );
            let _ = managed.child.kill().await;
            let _ = managed.child.wait().await;
        }
        *process = None;
        let _ = fs::remove_file(self.cfg.root.join("process.json"));
    }

    fn ensure_not_cancelled(&self) -> Result<(), ProviderError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(ProviderError::Unavailable {
                details: "operacao cancelada pelo usuario".to_string(),
            });
        }
        Ok(())
    }
}

impl Drop for OllamaRuntime {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

fn runtime_root() -> PathBuf {
    if let Ok(path) = std::env::var("APP_OLLAMA_RUNTIME_ROOT") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    if cfg!(windows) {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            if !local.trim().is_empty() {
                return PathBuf::from(local)
                    .join("MLX Pilot")
                    .join("runtime")
                    .join("ollama");
            }
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("mlx-pilot")
        .join("runtime")
        .join("ollama")
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "ollama.exe"
    } else {
        "ollama"
    }
}

fn version_is_compatible(version: &str) -> bool {
    let Some(version) = parse_version(version) else {
        return false;
    };
    let min = parse_version(MIN_OLLAMA_VERSION).unwrap();
    let max = parse_version(MAX_OLLAMA_VERSION_EXCLUSIVE).unwrap();
    version >= min && version < max
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let normalized = value.trim().trim_start_matches('v');
    let mut parts = normalized.split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.split('-').next()?.parse().ok()?,
    ))
}

fn extract_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .map(|part| part.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.'))
        .find(|part| parse_version(part).is_some())
        .map(ToString::to_string)
}

async fn executable_version(executable: &Path) -> Result<String, ProviderError> {
    let mut command = Command::new(executable);
    silence_console(&mut command);
    let output = command
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|source| ProviderError::Io {
            context: format!("executando {} --version", executable.display()),
            source,
        })?;
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    extract_version(&combined).ok_or_else(|| ProviderError::Unavailable {
        details: format!(
            "nao foi possivel identificar a versao de {}",
            executable.display()
        ),
    })
}

fn validate_runtime_payload(executable: &Path) -> Result<(), ProviderError> {
    if !executable.is_file() {
        return Err(ProviderError::Unavailable {
            details: format!("executavel Ollama ausente em {}", executable.display()),
        });
    }
    if cfg!(windows) {
        let root = executable.parent().unwrap_or_else(|| Path::new("."));
        let base = root.join("lib").join("ollama").join("ggml-base.dll");
        let cuda12 = root
            .join("lib")
            .join("ollama")
            .join("cuda_v12")
            .join("ggml-cuda.dll");
        let cuda13 = root
            .join("lib")
            .join("ollama")
            .join("cuda_v13")
            .join("ggml-cuda.dll");
        if !base.is_file() {
            return Err(ProviderError::Unavailable {
                details: format!("runtime Ollama incompleto: {} ausente", base.display()),
            });
        }
        if !cuda12.is_file() && !cuda13.is_file() {
            return Err(ProviderError::Unavailable {
                details: "runtime Ollama nao contem backend NVIDIA CUDA".to_string(),
            });
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, ProviderError> {
    let mut file = File::open(path).map_err(|source| ProviderError::Io {
        context: format!("abrindo {} para integridade", path.display()),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| ProviderError::Io {
            context: format!("lendo {} para integridade", path.display()),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_zip(archive_path: &Path, destination: &Path) -> Result<(), ProviderError> {
    fs::create_dir_all(destination).map_err(|source| ProviderError::Io {
        context: format!("criando {}", destination.display()),
        source,
    })?;
    let file = File::open(archive_path).map_err(|source| ProviderError::Io {
        context: format!("abrindo {}", archive_path.display()),
        source,
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| ProviderError::Unavailable {
        details: format!("arquivo do runtime Ollama invalido: {error}"),
    })?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| ProviderError::Unavailable {
                details: format!("falha lendo entrada {index} do runtime Ollama: {error}"),
            })?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(ProviderError::Unavailable {
                details: "pacote Ollama contem caminho inseguro".to_string(),
            });
        };
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|source| ProviderError::Io {
                context: format!("criando {}", output.display()),
                source,
            })?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|source| ProviderError::Io {
                context: format!("criando {}", parent.display()),
                source,
            })?;
        }
        let mut file = File::create(&output).map_err(|source| ProviderError::Io {
            context: format!("criando {}", output.display()),
            source,
        })?;
        std::io::copy(&mut entry, &mut file).map_err(|source| ProviderError::Io {
            context: format!("extraindo {}", output.display()),
            source,
        })?;
    }
    Ok(())
}

fn find_file_recursive(root: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case(name))
                .unwrap_or(false)
        {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), ProviderError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ProviderError::Io {
            context: format!("criando {}", parent.display()),
            source,
        })?;
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| ProviderError::Unavailable {
        details: format!("serializando estado do runtime Ollama: {error}"),
    })?;
    let mut file = File::create(&temporary).map_err(|source| ProviderError::Io {
        context: format!("criando {}", temporary.display()),
        source,
    })?;
    file.write_all(&bytes).map_err(|source| ProviderError::Io {
        context: format!("escrevendo {}", temporary.display()),
        source,
    })?;
    file.sync_all().map_err(|source| ProviderError::Io {
        context: format!("sincronizando {}", temporary.display()),
        source,
    })?;
    replace_file(&temporary, path)
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), ProviderError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };

        let source_wide = source
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination_wide = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let ok = unsafe {
            MoveFileExW(
                source_wide.as_ptr(),
                destination_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok != 0 {
            return Ok(());
        }
        Err(ProviderError::Io {
            context: format!("ativando {}", destination.display()),
            source: std::io::Error::last_os_error(),
        })
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination).map_err(|source| ProviderError::Io {
            context: format!("ativando {}", destination.display()),
            source,
        })
    }
}

#[derive(Debug, Default)]
struct ExternalOllama {
    detected: bool,
    version: Option<String>,
    pid: Option<u32>,
}

async fn detect_external_ollama(
    client: &reqwest::Client,
    managed_base_url: &str,
) -> ExternalOllama {
    let external_url = "http://127.0.0.1:11434";
    if managed_base_url.trim_end_matches('/') == external_url {
        return ExternalOllama::default();
    }
    let Ok(version) = api_version(client, external_url).await else {
        return ExternalOllama::default();
    };
    let pid = listening_pid(11434).await;
    info!(
        external_base_url = external_url,
        external_version = %version,
        external_pid = pid.unwrap_or_default(),
        "external Ollama server detected; it will not be terminated"
    );
    ExternalOllama {
        detected: true,
        version: Some(version),
        pid,
    }
}

async fn listening_pid(port: u16) -> Option<u32> {
    #[cfg(windows)]
    {
        let script = format!(
            "(Get-NetTCPConnection -State Listen -LocalPort {} -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty OwningProcess)",
            port
        );
        let mut command = Command::new("powershell.exe");
        silence_console(&mut command);
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            })
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        None
    }
}

async fn discover_nvidia_gpu() -> GpuStatus {
    let candidates = if cfg!(windows) {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        vec![
            PathBuf::from(system_root)
                .join("System32")
                .join("nvidia-smi.exe"),
            PathBuf::from("C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvidia-smi.exe"),
        ]
    } else {
        vec![PathBuf::from("nvidia-smi")]
    };
    for executable in candidates {
        if executable.is_absolute() && !executable.exists() {
            continue;
        }
        let mut command = Command::new(&executable);
        silence_console(&mut command);
        let output = command
            .args([
                "--query-gpu=name,memory.total,driver_version",
                "--format=csv,noheader,nounits",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await;
        let Ok(output) = output else { continue };
        if !output.status.success() {
            continue;
        }
        let line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        let parts = line.split(',').map(str::trim).collect::<Vec<_>>();
        if parts.len() < 3 || parts[0].is_empty() {
            continue;
        }
        return GpuStatus {
            expected: true,
            detected: false,
            name: Some(parts[0].to_string()),
            vram_bytes: parts[1].parse::<u64>().ok().map(|mib| mib * 1024 * 1024),
            backend: None,
            driver_version: Some(parts[2].to_string()),
        };
    }
    GpuStatus::default()
}

async fn wait_for_compute_backend(
    log_path: &Path,
    timeout: Duration,
) -> Option<(String, Option<String>, Option<u64>)> {
    let started = Instant::now();
    loop {
        if let Ok(content) = fs::read_to_string(log_path) {
            if let Some(result) = parse_compute_backend(&content) {
                return Some(result);
            }
        }
        if started.elapsed() >= timeout {
            return None;
        }
        sleep(Duration::from_millis(250)).await;
    }
}

fn parse_compute_backend(content: &str) -> Option<(String, Option<String>, Option<u64>)> {
    for line in content.lines().rev() {
        if !line.contains("inference compute") {
            continue;
        }
        let backend = extract_log_value(line, "library")
            .or_else(|| extract_log_value(line, "backend"))
            .unwrap_or_else(|| "unknown".to_string());
        let name = extract_log_value(line, "description");
        let vram = extract_log_value(line, "total")
            .or_else(|| extract_log_value(line, "total_vram"))
            .and_then(|value| parse_human_bytes(&value));
        return Some((backend, name, vram));
    }
    None
}

fn is_accelerated_backend(backend: &str) -> bool {
    matches!(
        backend.trim().to_ascii_lowercase().as_str(),
        "cuda" | "rocm" | "metal" | "vulkan"
    )
}

fn extract_log_value(line: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    if let Some(rest) = rest.strip_prefix('"') {
        return Some(rest.split('"').next()?.to_string());
    }
    Some(
        rest.split_whitespace()
            .next()?
            .trim_matches(',')
            .to_string(),
    )
}

fn parse_human_bytes(value: &str) -> Option<u64> {
    if let Some(bytes) = value.trim().strip_suffix(" B") {
        return bytes.trim().parse::<f64>().ok().map(|amount| amount as u64);
    }
    let normalized = value.trim().replace("GiB", "").replace("MiB", "");
    let amount = normalized.trim().parse::<f64>().ok()?;
    if value.contains("GiB") {
        Some((amount * 1024.0 * 1024.0 * 1024.0) as u64)
    } else if value.contains("MiB") {
        Some((amount * 1024.0 * 1024.0) as u64)
    } else {
        None
    }
}

async fn api_version(client: &reqwest::Client, base_url: &str) -> Result<String, ProviderError> {
    let response = client
        .get(format!("{}/api/version", base_url.trim_end_matches('/')))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(network_error)?;
    if response.status() != StatusCode::OK {
        return Err(ProviderError::Unavailable {
            details: format!("API Ollama respondeu HTTP {}", response.status()),
        });
    }
    let payload: serde_json::Value = response.json().await.map_err(network_error)?;
    payload
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| ProviderError::Unavailable {
            details: "API Ollama nao informou a versao".to_string(),
        })
}

fn network_error(error: reqwest::Error) -> ProviderError {
    ProviderError::Unavailable {
        details: format!("falha comunicando com a API Ollama: {error}"),
    }
}

fn host_without_scheme(base_url: &str) -> String {
    base_url
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string()
}

fn gpu_failure_diagnostic(log_path: &Path, executable: &Path) -> String {
    let tail = fs::read_to_string(log_path)
        .ok()
        .map(|content| {
            content
                .lines()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .unwrap_or_default();
    format!(
        "GPU NVIDIA detectada pelo sistema, mas o Ollama iniciou somente em CPU apos uma tentativa de recuperacao. Executavel: {}. Log: {}. Evidencia: {}",
        executable.display(),
        log_path.display(),
        tail
    )
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

#[cfg(windows)]
fn attach_kill_on_close_job(child: &Child) -> Result<WindowsJob, ProviderError> {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if handle.is_null() {
        return Err(ProviderError::Io {
            context: "criando Windows Job Object para Ollama".to_string(),
            source: std::io::Error::last_os_error(),
        });
    }
    let job = WindowsJob(handle as usize);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        return Err(ProviderError::Io {
            context: "configurando Windows Job Object para Ollama".to_string(),
            source: std::io::Error::last_os_error(),
        });
    }
    let process_handle = child
        .raw_handle()
        .ok_or_else(|| ProviderError::Unavailable {
            details: "processo Ollama nao expos handle no Windows".to_string(),
        })?;
    let assigned = unsafe {
        AssignProcessToJobObject(
            handle,
            process_handle as windows_sys::Win32::Foundation::HANDLE,
        )
    };
    if assigned == 0 {
        return Err(ProviderError::Io {
            context: "associando Ollama ao Windows Job Object".to_string(),
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(job)
}

async fn process_executable_matches(pid: u32, expected: &Path) -> bool {
    #[cfg(windows)]
    {
        let script = format!(
            "(Get-CimInstance Win32_Process -Filter \\\"ProcessId={}\\\").ExecutablePath",
            pid
        );
        let mut command = Command::new("powershell.exe");
        silence_console(&mut command);
        let output = command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await;
        output
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                !path.is_empty() && paths_equal(Path::new(&path), expected)
            })
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, expected);
        false
    }
}

async fn terminate_pid(pid: u32) -> Result<(), ProviderError> {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill.exe");
        silence_console(&mut command);
        let output = command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|source| ProviderError::Io {
                context: format!("encerrando processo Ollama obsoleto PID {pid}"),
                source,
            })?;
        if output.status.success() {
            return Ok(());
        }
        Err(ProviderError::CommandFailed {
            command: format!("taskkill /PID {pid} /T /F"),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Err(ProviderError::Unavailable {
            details: "recuperacao de processo obsoleto ainda nao suportada nesta plataforma"
                .to_string(),
        })
    }
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex as StdMutex, OnceLock};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    #[test]
    fn compatible_version_range_is_pinned() {
        assert!(version_is_compatible("0.30.0"));
        assert!(version_is_compatible("0.30.7"));
        assert!(!version_is_compatible("0.21.0"));
        assert!(!version_is_compatible("0.31.0"));
    }

    #[test]
    fn parses_gpu_and_cpu_discovery_lines() {
        let gpu = parse_compute_backend(
            r#"time=... level=INFO msg="inference compute" id=GPU-1 library=CUDA compute=12.0 description="NVIDIA GeForce RTX 5070" total="11.9 GiB""#,
        )
        .unwrap();
        assert_eq!(gpu.0, "CUDA");
        assert_eq!(gpu.1.as_deref(), Some("NVIDIA GeForce RTX 5070"));
        assert!(gpu.2.unwrap() > 11_000_000_000);

        let cpu = parse_compute_backend(
            r#"time=... level=INFO msg="inference compute" id=cpu library=cpu total="0 B""#,
        )
        .unwrap();
        assert_eq!(cpu.0, "cpu");
        assert_eq!(cpu.2, Some(0));
        assert!(is_accelerated_backend(&gpu.0));
        assert!(!is_accelerated_backend(&cpu.0));
        assert!(!is_accelerated_backend("unknown"));
    }

    #[test]
    fn rejects_zip_path_traversal() {
        let root = std::env::temp_dir().join(format!("mlx-ollama-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("bad.zip");
        {
            let file = File::create(&archive).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("../escape.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"bad").unwrap();
            writer.finish().unwrap();
        }
        assert!(extract_zip(&archive, &root.join("out")).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn atomic_state_replace_overwrites_previous_manifest() {
        let root = std::env::temp_dir().join(format!("mlx-ollama-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("runtime-state.json");
        write_json_atomic(&path, &serde_json::json!({"phase": "ready"})).unwrap();
        write_json_atomic(&path, &serde_json::json!({"phase": "failed"})).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["phase"], "failed");
        assert!(fs::read_dir(&root)
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp-")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancellation_does_not_remove_previous_active_runtime() {
        let root = std::env::temp_dir().join(format!("mlx-ollama-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let active = root.join("active.json");
        fs::write(&active, b"previous-valid-runtime").unwrap();
        let runtime = OllamaRuntime::new(OllamaRuntimeConfig {
            root: root.clone(),
            base_url: DEFAULT_MANAGED_URL.to_string(),
            models_dir: root.join("models"),
            startup_timeout: Duration::from_secs(1),
            auto_install: true,
            auto_start: true,
        });
        runtime.cancel();
        assert!(runtime.ensure_not_cancelled().is_err());
        assert_eq!(fs::read(&active).unwrap(), b"previous-valid-runtime");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupted_download_cleans_staging_and_preserves_active_manifest() {
        static ENV_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK.get_or_init(|| StdMutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!("mlx-ollama-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let active = root.join("active.json");
        fs::write(&active, b"previous-valid-runtime").unwrap();
        drop(_guard);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10485760\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let chunk = vec![7_u8; 64 * 1024];
            for _ in 0..160 {
                if stream.write_all(&chunk).await.is_err() {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        });

        std::env::set_var(
            "APP_OLLAMA_RUNTIME_URL",
            format!("http://{address}/ollama.zip"),
        );
        let runtime = OllamaRuntime::new(OllamaRuntimeConfig {
            root: root.clone(),
            base_url: DEFAULT_MANAGED_URL.to_string(),
            models_dir: root.join("models"),
            startup_timeout: Duration::from_secs(1),
            auto_install: true,
            auto_start: true,
        });
        let operation = {
            let runtime = runtime.clone();
            tokio::spawn(async move { runtime.install_or_update().await })
        };
        sleep(Duration::from_millis(35)).await;
        runtime.cancel();
        let result = operation.await.unwrap();
        std::env::remove_var("APP_OLLAMA_RUNTIME_URL");
        let _ = server.await;

        assert!(result.is_err());
        assert_eq!(fs::read(&active).unwrap(), b"previous-valid-runtime");
        let staging = root.join("staging");
        assert!(!staging.exists() || fs::read_dir(staging).unwrap().flatten().next().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_job_object_terminates_owned_child_on_close() {
        let mut command = Command::new("cmd.exe");
        silence_console(&mut command);
        command
            .args(["/C", "ping 127.0.0.1 -n 30 >NUL"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().unwrap();
        let job = attach_kill_on_close_job(&child).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        drop(job);
        let _status = tokio::time::timeout(Duration::from_secs(3), child.wait())
            .await
            .expect("child must terminate when the job handle closes")
            .unwrap();
    }
}
