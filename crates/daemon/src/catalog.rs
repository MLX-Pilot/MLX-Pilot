use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

const HF_SOURCE_ID: &str = "huggingface";

#[derive(Debug, Clone)]
pub struct CatalogConfig {
    pub hf_api_base: String,
    pub hf_token: Option<String>,
    pub downloads_root: PathBuf,
    pub search_limit_default: usize,
    pub download_timeout: Duration,
}

impl CatalogConfig {
    pub fn source_download_dir(&self, source: CatalogSource) -> PathBuf {
        match source {
            CatalogSource::HuggingFace => self.downloads_root.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSource {
    HuggingFace,
}

impl CatalogSource {
    pub fn parse(value: Option<&str>) -> Result<Self, CatalogError> {
        match value
            .unwrap_or(HF_SOURCE_ID)
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            HF_SOURCE_ID => Ok(Self::HuggingFace),
            other => Err(CatalogError::BadRequest(format!(
                "source '{other}' nao suportada. Use: {HF_SOURCE_ID}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CatalogSource::HuggingFace => HF_SOURCE_ID,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogService {
    http: reqwest::Client,
    cfg: CatalogConfig,
    jobs: Arc<RwLock<HashMap<String, DownloadJob>>>,
    cancel_flags: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>,
    sequence: Arc<AtomicU64>,
}

impl CatalogService {
    pub fn new(cfg: CatalogConfig) -> Result<Self, CatalogError> {
        let http = reqwest::Client::builder()
            .user_agent("mlx-pilot/0.1")
            .timeout(Duration::from_secs(40))
            .build()
            .map_err(|error| CatalogError::Unavailable(format!("http client: {error}")))?;

        Ok(Self {
            http,
            cfg,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            cancel_flags: Arc::new(RwLock::new(HashMap::new())),
            sequence: Arc::new(AtomicU64::new(1)),
        })
    }

    pub fn list_sources(&self) -> Vec<CatalogSourceDescriptor> {
        vec![CatalogSourceDescriptor {
            id: HF_SOURCE_ID.to_string(),
            name: "Hugging Face".to_string(),
            kind: "model-hub".to_string(),
            supports_download: true,
            description: "Catalogo publico de modelos com download direto para a pasta local."
                .to_string(),
        }]
    }

    pub async fn search_models(
        &self,
        query: CatalogSearchQuery,
    ) -> Result<Vec<RemoteModelCard>, CatalogError> {
        let source = CatalogSource::parse(query.source.as_deref())?;
        let limit = query
            .limit
            .unwrap_or(self.cfg.search_limit_default)
            .clamp(1, 40);

        match source {
            CatalogSource::HuggingFace => {
                self.search_huggingface(query.query.as_deref(), limit).await
            }
        }
    }

    async fn search_huggingface(
        &self,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RemoteModelCard>, CatalogError> {
        let base = self.cfg.hf_api_base.trim_end_matches('/');
        let mut url = Url::parse(&format!("{base}/api/models"))
            .map_err(|error| CatalogError::Unavailable(format!("HF api url invalida: {error}")))?;

        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("limit", &limit.to_string());
            pairs.append_pair("sort", "downloads");
            pairs.append_pair("direction", "-1");
            pairs.append_pair("full", "true");
            if let Some(value) = query.map(str::trim).filter(|v| !v.is_empty()) {
                pairs.append_pair("search", value);
            }
        }

        let models = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|error| CatalogError::Network(format!("falha na busca HF: {error}")))?
            .error_for_status()
            .map_err(|error| CatalogError::Network(format!("resposta invalida HF: {error}")))?
            .json::<Vec<HfModelSearchItem>>()
            .await
            .map_err(|error| CatalogError::Network(format!("json invalido HF: {error}")))?;

        let mut cards = Vec::with_capacity(models.len());

        for model in models {
            let size_bytes = self.fetch_hf_model_size_bytes(&model.id).await.ok();
            cards.push(to_remote_card(model, size_bytes));
        }

        Ok(cards)
    }

    async fn fetch_hf_model_size_bytes(&self, model_id: &str) -> Result<u64, CatalogError> {
        let detail = self.fetch_hf_model_detail(model_id).await?;
        let size = detail
            .siblings
            .iter()
            .filter_map(|entry| entry.size)
            .sum::<u64>();

        if size == 0 {
            return Err(CatalogError::Unavailable(
                "nao foi possivel estimar tamanho do repositorio".to_string(),
            ));
        }

        Ok(size)
    }

    async fn fetch_hf_model_detail(&self, model_id: &str) -> Result<HfModelDetail, CatalogError> {
        let encoded_id = encode_repo_id(model_id);
        let base = self.cfg.hf_api_base.trim_end_matches('/');
        let url = format!("{base}/api/models/{encoded_id}?blobs=true");

        let mut request = self.http.get(url);
        if let Some(token) = self
            .cfg
            .hf_token
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            request = request.bearer_auth(token);
        }

        request
            .send()
            .await
            .map_err(|error| CatalogError::Network(format!("falha ao detalhar modelo: {error}")))?
            .error_for_status()
            .map_err(|error| CatalogError::Network(format!("erro em detalhe de modelo: {error}")))?
            .json::<HfModelDetail>()
            .await
            .map_err(|error| CatalogError::Network(format!("json invalido em detalhe: {error}")))
    }

    pub async fn create_download(
        &self,
        request: CreateDownloadRequest,
    ) -> Result<DownloadJob, CatalogError> {
        let source = CatalogSource::parse(Some(&request.source))?;

        let model_id = request.model_id.trim();
        if model_id.is_empty() {
            return Err(CatalogError::BadRequest(
                "model_id nao pode ser vazio".to_string(),
            ));
        }

        let destination = self.cfg.source_download_dir(source).join(format!(
            "{}--{}",
            source.as_str(),
            sanitize_model_id(model_id)
        ));

        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|source| CatalogError::Io {
                    context: format!("criando pasta {}", parent.display()),
                    source,
                })?;
        }

        let job_id = self.generate_job_id();
        let created_at = now_epoch_ms();

        let job = DownloadJob {
            id: job_id.clone(),
            source: source.as_str().to_string(),
            model_id: model_id.to_string(),
            destination: destination.display().to_string(),
            status: DownloadStatus::Queued,
            created_at,
            started_at: None,
            finished_at: None,
            output: None,
            error: None,
            allow_patterns: request.allow_patterns,
            progress_percent: 0.0,
            bytes_downloaded: 0,
            bytes_total: 0,
            total_files: 0,
            completed_files: 0,
            current_file: None,
            can_cancel: true,
        };

        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(job_id.clone(), job.clone());
        }

        let cancel_flag = Arc::new(AtomicBool::new(false));
        {
            let mut flags = self.cancel_flags.write().await;
            flags.insert(job_id.clone(), cancel_flag.clone());
        }

        let service = self.clone();
        tokio::spawn(async move {
            service.execute_download(job_id, cancel_flag).await;
        });

        Ok(job)
    }

    pub async fn cancel_download(&self, job_id: &str) -> Result<DownloadJob, CatalogError> {
        let job = self
            .get_download(job_id)
            .await
            .ok_or_else(|| CatalogError::NotFound(format!("download '{job_id}' nao encontrado")))?;

        if !job.can_cancel {
            return Err(CatalogError::BadRequest(
                "download nao pode mais ser cancelado".to_string(),
            ));
        }

        let cancel_flag = {
            let flags = self.cancel_flags.read().await;
            flags.get(job_id).cloned()
        }
        .ok_or_else(|| CatalogError::BadRequest("download nao esta ativo".to_string()))?;

        cancel_flag.store(true, Ordering::Relaxed);

        self.patch_job(job_id, |entry| {
            entry.status = DownloadStatus::Cancelling;
            entry.output = Some("cancelamento solicitado".to_string());
        })
        .await;

        self.get_download(job_id)
            .await
            .ok_or_else(|| CatalogError::NotFound(format!("download '{job_id}' nao encontrado")))
    }

    pub async fn list_downloads(&self) -> Vec<DownloadJob> {
        let jobs = self.jobs.read().await;
        let mut ordered = jobs.values().cloned().collect::<Vec<_>>();
        ordered.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        ordered
    }

    pub async fn get_download(&self, job_id: &str) -> Option<DownloadJob> {
        let jobs = self.jobs.read().await;
        jobs.get(job_id).cloned()
    }

    async fn execute_download(&self, job_id: String, cancel_flag: Arc<AtomicBool>) {
        self.patch_job(&job_id, |job| {
            job.status = DownloadStatus::Running;
            job.started_at = Some(now_epoch_ms());
            job.error = None;
            job.output = Some("download em andamento".to_string());
        })
        .await;

        let snapshot = match self.get_download(&job_id).await {
            Some(value) => value,
            None => return,
        };

        let source = match CatalogSource::parse(Some(&snapshot.source)) {
            Ok(value) => value,
            Err(error) => {
                self.fail_job(&job_id, error.to_string()).await;
                return;
            }
        };

        let destination = PathBuf::from(&snapshot.destination);
        let started = Instant::now();

        let result = match source {
            CatalogSource::HuggingFace => {
                self.run_hf_download(
                    &job_id,
                    &snapshot.model_id,
                    &destination,
                    &snapshot.allow_patterns,
                    cancel_flag.clone(),
                )
                .await
            }
        };

        match result {
            Ok(output) => {
                self.patch_job(&job_id, |job| {
                    job.status = DownloadStatus::Completed;
                    job.finished_at = Some(now_epoch_ms());
                    job.output = Some(format!("{output} ({}s)", started.elapsed().as_secs()));
                    job.error = None;
                    job.progress_percent = 100.0;
                    job.can_cancel = false;
                    job.current_file = None;
                })
                .await;
            }
            Err(CatalogError::Cancelled { details }) => {
                let _ = tokio::fs::remove_dir_all(&destination).await;
                self.patch_job(&job_id, |job| {
                    job.status = DownloadStatus::Cancelled;
                    job.finished_at = Some(now_epoch_ms());
                    job.output = Some("download cancelado".to_string());
                    job.error = Some(details);
                    job.can_cancel = false;
                    job.current_file = None;
                })
                .await;
            }
            Err(error) => {
                self.fail_job(&job_id, error.to_string()).await;
            }
        }

        let mut flags = self.cancel_flags.write().await;
        flags.remove(&job_id);
    }

    async fn run_hf_download(
        &self,
        job_id: &str,
        model_id: &str,
        destination: &Path,
        allow_patterns: &[String],
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<String, CatalogError> {
        tokio::fs::create_dir_all(destination)
            .await
            .map_err(|source| CatalogError::Io {
                context: format!("criando destino {}", destination.display()),
                source,
            })?;

        let detail = self.fetch_hf_model_detail(model_id).await?;

        let mut files = detail
            .siblings
            .into_iter()
            .filter(|entry| should_include_file(&entry.rfilename, allow_patterns))
            .collect::<Vec<_>>();

        files.retain(|entry| !entry.rfilename.trim().is_empty());

        if files.is_empty() {
            return Err(CatalogError::BadRequest(
                "nenhum arquivo para baixar com os filtros informados".to_string(),
            ));
        }

        let total_files = files.len();
        let bytes_total = files.iter().filter_map(|entry| entry.size).sum::<u64>();

        self.patch_job(job_id, |job| {
            job.total_files = total_files;
            job.bytes_total = bytes_total;
            job.progress_percent = 0.0;
            job.completed_files = 0;
            job.bytes_downloaded = 0;
        })
        .await;

        let base = self.cfg.hf_api_base.trim_end_matches('/');

        let mut downloaded_bytes = 0_u64;
        let mut completed_files = 0_usize;

        for file in files {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(CatalogError::Cancelled {
                    details: "cancelado pelo usuario".to_string(),
                });
            }

            self.patch_job(job_id, |job| {
                job.current_file = Some(file.rfilename.clone());
                job.status = DownloadStatus::Running;
            })
            .await;

            let file_relative = file.rfilename.replace('\\', "/");
            let target_file = destination.join(&file_relative);
            if let Some(parent) = target_file.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|source| CatalogError::Io {
                        context: format!("criando pasta {}", parent.display()),
                        source,
                    })?;
            }

            let mut request = self
                .http
                .get(format!(
                    "{base}/{}/resolve/main/{}?download=1",
                    encode_repo_id(model_id),
                    encode_file_path(&file_relative)
                ))
                .timeout(self.cfg.download_timeout);

            if let Some(token) = self
                .cfg
                .hf_token
                .as_ref()
                .filter(|value| !value.trim().is_empty())
            {
                request = request.bearer_auth(token);
            }

            let response = request
                .send()
                .await
                .map_err(|error| {
                    CatalogError::Network(format!("falha ao baixar arquivo: {error}"))
                })?
                .error_for_status()
                .map_err(|error| {
                    CatalogError::Network(format!("erro no download de arquivo: {error}"))
                })?;

            let mut stream = response.bytes_stream();
            let temp_file = target_file.with_extension("part");
            let mut writer = tokio::fs::File::create(&temp_file)
                .await
                .map_err(|source| CatalogError::Io {
                    context: format!("criando arquivo temporario {}", temp_file.display()),
                    source,
                })?;

            let mut last_update = Instant::now();

            while let Some(next) = stream.next().await {
                if cancel_flag.load(Ordering::Relaxed) {
                    let _ = tokio::fs::remove_file(&temp_file).await;
                    return Err(CatalogError::Cancelled {
                        details: "cancelado pelo usuario".to_string(),
                    });
                }

                let chunk = next.map_err(|error| {
                    CatalogError::Network(format!("falha no stream do arquivo: {error}"))
                })?;

                writer
                    .write_all(&chunk)
                    .await
                    .map_err(|source| CatalogError::Io {
                        context: format!("escrevendo arquivo {}", temp_file.display()),
                        source,
                    })?;

                downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);

                if last_update.elapsed() >= Duration::from_millis(140) {
                    let bytes_total_copy = bytes_total;
                    let current_file_copy = file_relative.clone();
                    self.patch_job(job_id, |job| {
                        job.bytes_downloaded = downloaded_bytes;
                        job.progress_percent = calc_progress_percent(
                            downloaded_bytes,
                            bytes_total_copy,
                            completed_files,
                            total_files,
                        );
                        job.current_file = Some(current_file_copy);
                    })
                    .await;
                    last_update = Instant::now();
                }
            }

            writer.flush().await.map_err(|source| CatalogError::Io {
                context: format!("flush arquivo {}", temp_file.display()),
                source,
            })?;

            tokio::fs::rename(&temp_file, &target_file)
                .await
                .map_err(|source| CatalogError::Io {
                    context: format!(
                        "movendo arquivo temporario {} para {}",
                        temp_file.display(),
                        target_file.display()
                    ),
                    source,
                })?;

            completed_files = completed_files.saturating_add(1);
            let bytes_total_copy = bytes_total;
            self.patch_job(job_id, |job| {
                job.completed_files = completed_files;
                job.bytes_downloaded = downloaded_bytes;
                job.progress_percent = calc_progress_percent(
                    downloaded_bytes,
                    bytes_total_copy,
                    completed_files,
                    total_files,
                );
            })
            .await;
        }

        Ok(format!("download concluido em {}", destination.display()))
    }

    async fn fail_job(&self, job_id: &str, error: String) {
        self.patch_job(job_id, |job| {
            job.status = DownloadStatus::Failed;
            job.finished_at = Some(now_epoch_ms());
            job.error = Some(error);
            job.can_cancel = false;
            job.current_file = None;
        })
        .await;
    }

    async fn patch_job<F>(&self, job_id: &str, patch: F)
    where
        F: FnOnce(&mut DownloadJob),
    {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            patch(job);
        }
    }

    fn generate_job_id(&self) -> String {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        format!("dl-{}-{sequence}", now_epoch_ms())
    }
}

#[derive(Debug, Serialize)]
pub struct CatalogSourceDescriptor {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub supports_download: bool,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct CatalogSearchQuery {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteModelCard {
    pub source: String,
    pub model_id: String,
    pub name: String,
    pub author: String,
    pub task: Option<String>,
    pub downloads: u64,
    pub likes: u64,
    pub size_bytes: Option<u64>,
    pub last_modified: Option<String>,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub model_url: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateDownloadRequest {
    pub source: String,
    pub model_id: String,
    #[serde(default)]
    pub allow_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadJob {
    pub id: String,
    pub source: String,
    pub model_id: String,
    pub destination: String,
    pub status: DownloadStatus,
    pub created_at: u128,
    pub started_at: Option<u128>,
    pub finished_at: Option<u128>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub allow_patterns: Vec<String>,
    pub progress_percent: f32,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub total_files: usize,
    pub completed_files: usize,
    pub current_file: Option<String>,
    pub can_cancel: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Running,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug)]
pub enum CatalogError {
    BadRequest(String),
    NotFound(String),
    Network(String),
    Cancelled {
        details: String,
    },
    Io {
        context: String,
        source: std::io::Error,
    },
    Unavailable(String),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::BadRequest(message) => write!(formatter, "{message}"),
            CatalogError::NotFound(message) => write!(formatter, "{message}"),
            CatalogError::Network(message) => write!(formatter, "{message}"),
            CatalogError::Cancelled { details } => write!(formatter, "{details}"),
            CatalogError::Io { context, source } => write!(formatter, "{context}: {source}"),
            CatalogError::Unavailable(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for CatalogError {}

#[derive(Debug, Deserialize)]
struct HfModelSearchItem {
    id: String,
    #[serde(default)]
    pipeline_tag: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    likes: Option<u64>,
    #[serde(default, rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default, rename = "cardData")]
    card_data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct HfModelDetail {
    #[serde(default)]
    siblings: Vec<HfSibling>,
}

#[derive(Debug, Deserialize)]
struct HfSibling {
    #[serde(default)]
    rfilename: String,
    #[serde(default)]
    size: Option<u64>,
}

fn to_remote_card(model: HfModelSearchItem, size_bytes: Option<u64>) -> RemoteModelCard {
    let author = model
        .id
        .split('/')
        .next()
        .map(ToString::to_string)
        .unwrap_or_else(|| "unknown".to_string());

    let name = model
        .id
        .split('/')
        .nth(1)
        .map(ToString::to_string)
        .unwrap_or_else(|| model.id.clone());

    let summary = model
        .card_data
        .as_ref()
        .and_then(|value| value.get("description"))
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string);

    RemoteModelCard {
        source: HF_SOURCE_ID.to_string(),
        model_id: model.id.clone(),
        name,
        author,
        task: model.pipeline_tag,
        downloads: model.downloads.unwrap_or(0),
        likes: model.likes.unwrap_or(0),
        size_bytes,
        last_modified: model.last_modified,
        tags: model.tags,
        summary,
        model_url: format!("https://huggingface.co/{}", model.id),
    }
}

fn sanitize_model_id(model_id: &str) -> String {
    let sanitized = model_id
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => character,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if sanitized.is_empty() {
        "model".to_string()
    } else {
        sanitized
    }
}

fn encode_repo_id(repo_id: &str) -> String {
    repo_id
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_file_path(path: &str) -> String {
    path.split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn now_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn calc_progress_percent(
    bytes_downloaded: u64,
    bytes_total: u64,
    completed_files: usize,
    total_files: usize,
) -> f32 {
    if bytes_total > 0 {
        ((bytes_downloaded as f64 / bytes_total as f64) * 100.0).clamp(0.0, 100.0) as f32
    } else if total_files > 0 {
        ((completed_files as f64 / total_files as f64) * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    }
}

fn should_include_file(file: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }

    patterns
        .iter()
        .any(|pattern| wildcard_match(pattern.trim(), file))
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    wildcard_match_impl(
        &pattern.chars().collect::<Vec<_>>(),
        0,
        &value.chars().collect::<Vec<_>>(),
        0,
    )
}

fn wildcard_match_impl(pattern: &[char], p_idx: usize, value: &[char], v_idx: usize) -> bool {
    if p_idx == pattern.len() {
        return v_idx == value.len();
    }

    match pattern[p_idx] {
        '*' => {
            if wildcard_match_impl(pattern, p_idx + 1, value, v_idx) {
                return true;
            }

            let mut index = v_idx;
            while index < value.len() {
                if wildcard_match_impl(pattern, p_idx + 1, value, index + 1) {
                    return true;
                }
                index += 1;
            }

            false
        }
        '?' => {
            if v_idx < value.len() {
                wildcard_match_impl(pattern, p_idx + 1, value, v_idx + 1)
            } else {
                false
            }
        }
        expected => {
            if v_idx < value.len() && expected == value[v_idx] {
                wildcard_match_impl(pattern, p_idx + 1, value, v_idx + 1)
            } else {
                false
            }
        }
    }
}
