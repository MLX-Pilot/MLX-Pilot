//! Hardware-Fit endpoints — detect hardware, rank models, serve profiles.
//!
//! Endpoints:
//! - `GET /api/hwfit/system` — detect system hardware
//! - `GET /api/hwfit/models` — rank catalog models against hardware
//! - `GET /api/hwfit/profiles` — serve profiles for a specific model
//! - `POST /api/hwfit/simulate` — simulate hardware manually

use axum::extract::Query;
use axum::http::StatusCode;
use axum::Json;
use mlx_hardware_fit::{self, GpuGroup, GpuInfo};
use mlx_model_fit::{self, FitAnalysis, ModelCard, ServeProfile};
use serde::{Deserialize, Serialize};
use tracing::error;

// ── Request types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SystemQuery {
    #[serde(default)]
    pub fresh: bool,
}

#[derive(Debug, Deserialize)]
pub struct ModelsQuery {
    pub use_case: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<usize>,
    pub search: Option<String>,
    pub quant: Option<String>,
    pub ctx: Option<usize>,
    pub fit_only: Option<bool>,
    // Manual hardware overrides
    pub manual_mode: Option<bool>,
    pub manual_gpu_name: Option<String>,
    pub manual_gpu_count: Option<usize>,
    pub manual_vram_gb: Option<f64>,
    pub manual_ram_gb: Option<f64>,
    pub manual_backend: Option<String>,
    pub ignore_detected_gpu: Option<bool>,
    pub ignore_detected_ram: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ProfilesQuery {
    pub model_id: String,
    pub params_b: Option<f64>,
    pub architecture: Option<String>,
    pub is_moe: Option<bool>,
    pub active_params_b: Option<f64>,
    pub context_length: Option<usize>,
    pub default_quant: Option<String>,
    pub serve_weights_gb: Option<f64>,
    pub serve_quant: Option<String>,
    // Manual hardware overrides (same contract as ModelsQuery)
    pub manual_mode: Option<bool>,
    pub manual_gpu_name: Option<String>,
    pub manual_gpu_count: Option<usize>,
    pub manual_vram_gb: Option<f64>,
    pub manual_ram_gb: Option<f64>,
    pub manual_backend: Option<String>,
    pub ignore_detected_gpu: Option<bool>,
    pub ignore_detected_ram: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SimulateRequest {
    pub manual_gpu_name: Option<String>,
    pub manual_gpu_count: Option<usize>,
    pub manual_vram_gb: Option<f64>,
    pub manual_ram_gb: Option<f64>,
    pub manual_backend: Option<String>,
    pub ignore_detected_gpu: Option<bool>,
    pub ignore_detected_ram: Option<bool>,
}

// ── Response types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SystemResponse {
    pub platform: String,
    pub cpu_name: String,
    pub cpu_cores: usize,
    pub ram_gb: f64,
    pub available_ram_gb: f64,
    pub gpus: Vec<GpuInfo>,
    pub gpu_count: usize,
    pub total_vram_gb: f64,
    pub primary_backend: String,
    pub is_cpu_only: bool,
    pub gpu_groups: Vec<GpuGroup>,
    pub detected_at: String,
}

#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub hardware: SystemResponse,
    pub models: Vec<FitAnalysis>,
    pub total: usize,
}

#[derive(Debug, Clone, Copy)]
struct HardwareOverrides<'a> {
    manual_mode: bool,
    manual_gpu_name: Option<&'a str>,
    manual_gpu_count: Option<usize>,
    manual_vram_gb: Option<f64>,
    manual_ram_gb: Option<f64>,
    manual_backend: Option<&'a str>,
    ignore_detected_gpu: bool,
    ignore_detected_ram: bool,
}

impl<'a> HardwareOverrides<'a> {
    fn from_models_query(query: &'a ModelsQuery) -> Self {
        Self {
            manual_mode: query.manual_mode.unwrap_or(false),
            manual_gpu_name: query.manual_gpu_name.as_deref(),
            manual_gpu_count: query.manual_gpu_count,
            manual_vram_gb: query.manual_vram_gb,
            manual_ram_gb: query.manual_ram_gb,
            manual_backend: query.manual_backend.as_deref(),
            ignore_detected_gpu: query.ignore_detected_gpu.unwrap_or(false),
            ignore_detected_ram: query.ignore_detected_ram.unwrap_or(false),
        }
    }

    fn from_profiles_query(query: &'a ProfilesQuery) -> Self {
        Self {
            manual_mode: query.manual_mode.unwrap_or(false),
            manual_gpu_name: query.manual_gpu_name.as_deref(),
            manual_gpu_count: query.manual_gpu_count,
            manual_vram_gb: query.manual_vram_gb,
            manual_ram_gb: query.manual_ram_gb,
            manual_backend: query.manual_backend.as_deref(),
            ignore_detected_gpu: query.ignore_detected_gpu.unwrap_or(false),
            ignore_detected_ram: query.ignore_detected_ram.unwrap_or(false),
        }
    }
}

async fn resolve_hardware_profile(
    overrides: HardwareOverrides<'_>,
) -> Result<mlx_hardware_fit::HardwareProfile, StatusCode> {
    let profile = mlx_hardware_fit::detect_system(false).await.map_err(|e| {
        error!("Hardware detection failed: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if overrides.manual_mode {
        Ok(mlx_hardware_fit::simulate_hardware(
            &profile,
            overrides.manual_gpu_count,
            overrides.manual_vram_gb,
            overrides.manual_ram_gb,
            overrides.manual_backend.map(str::to_string),
            overrides.manual_gpu_name.map(str::to_string),
            overrides.ignore_detected_gpu,
            overrides.ignore_detected_ram,
        ))
    } else {
        Ok(profile)
    }
}

fn profile_to_system_response(profile: &mlx_hardware_fit::HardwareProfile) -> SystemResponse {
    let gpu_groups = mlx_hardware_fit::group_gpus(&profile.gpus);
    SystemResponse {
        platform: profile.platform.clone(),
        cpu_name: profile.cpu_name.clone(),
        cpu_cores: profile.cpu_cores,
        ram_gb: profile.ram_gb,
        available_ram_gb: profile.available_ram_gb,
        gpus: profile.gpus.clone(),
        gpu_count: profile.gpu_count,
        total_vram_gb: profile.total_vram_gb,
        primary_backend: profile.primary_backend.clone(),
        is_cpu_only: profile.is_cpu_only,
        gpu_groups,
        detected_at: profile.detected_at.clone(),
    }
}

// ── GET /api/hwfit/system ──────────────────────────────────────────────────

pub async fn hwfit_system(
    Query(query): Query<SystemQuery>,
) -> Result<Json<SystemResponse>, StatusCode> {
    let profile = mlx_hardware_fit::detect_system(query.fresh)
        .await
        .map_err(|e| {
            error!("Hardware detection failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let gpu_groups = mlx_hardware_fit::group_gpus(&profile.gpus);

    Ok(Json(SystemResponse {
        platform: profile.platform,
        cpu_name: profile.cpu_name,
        cpu_cores: profile.cpu_cores,
        ram_gb: profile.ram_gb,
        available_ram_gb: profile.available_ram_gb,
        gpus: profile.gpus,
        gpu_count: profile.gpu_count,
        total_vram_gb: profile.total_vram_gb,
        primary_backend: profile.primary_backend,
        is_cpu_only: profile.is_cpu_only,
        gpu_groups,
        detected_at: profile.detected_at,
    }))
}

// ── GET /api/hwfit/models ──────────────────────────────────────────────────

pub async fn hwfit_models(
    Query(query): Query<ModelsQuery>,
) -> Result<Json<ModelsResponse>, StatusCode> {
    let overrides = HardwareOverrides::from_models_query(&query);
    let profile = resolve_hardware_profile(overrides).await?;
    let hardware = profile_to_system_response(&profile);

    // Build model catalog from common GGUF models
    let models = get_default_model_catalog();

    let ranked = mlx_model_fit::rank_models(
        &profile,
        &models,
        query.use_case.as_deref(),
        query.sort.as_deref(),
        query.limit,
        query.search.as_deref(),
        query.quant.as_deref(),
        query.ctx,
        query.fit_only.unwrap_or(false),
    );

    let total = ranked.len();
    Ok(Json(ModelsResponse {
        hardware,
        models: ranked,
        total,
    }))
}

// ── GET /api/hwfit/profiles ────────────────────────────────────────────────

pub async fn hwfit_profiles(
    Query(query): Query<ProfilesQuery>,
) -> Result<Json<Vec<ServeProfile>>, StatusCode> {
    let overrides = HardwareOverrides::from_profiles_query(&query);
    let profile = resolve_hardware_profile(overrides).await?;

    let model = ModelCard {
        id: query.model_id.clone(),
        name: query.model_id.clone(),
        provider: "huggingface".to_string(),
        params_b: query
            .params_b
            .unwrap_or_else(|| mlx_model_fit::params_b_from_name(&query.model_id).unwrap_or(7.0)),
        architecture: query.architecture.unwrap_or_else(|| "llama".to_string()),
        is_moe: query.is_moe.unwrap_or(false),
        active_params_b: query.active_params_b,
        context_length: query.context_length.unwrap_or(8192),
        quantizations: vec![],
        default_quant: query.default_quant.unwrap_or_else(|| "Q4_K_M".to_string()),
        has_vision: false,
        source_url: None,
        size_gb: None,
    };

    let profiles = mlx_model_fit::compute_serve_profiles(
        &profile,
        &model,
        query.serve_weights_gb,
        query.serve_quant.as_deref(),
    );

    Ok(Json(profiles))
}

// ── POST /api/hwfit/simulate ───────────────────────────────────────────────

pub async fn hwfit_simulate(
    Json(req): Json<SimulateRequest>,
) -> Result<Json<SystemResponse>, StatusCode> {
    let overrides = HardwareOverrides {
        manual_mode: true,
        manual_gpu_name: req.manual_gpu_name.as_deref(),
        manual_gpu_count: req.manual_gpu_count,
        manual_vram_gb: req.manual_vram_gb,
        manual_ram_gb: req.manual_ram_gb,
        manual_backend: req.manual_backend.as_deref(),
        ignore_detected_gpu: req.ignore_detected_gpu.unwrap_or(false),
        ignore_detected_ram: req.ignore_detected_ram.unwrap_or(false),
    };
    let simulated = resolve_hardware_profile(overrides).await?;

    Ok(Json(profile_to_system_response(&simulated)))
}

// ── Default model catalog ──────────────────────────────────────────────────

fn get_default_model_catalog() -> Vec<ModelCard> {
    vec![
        // Llama 3 family
        ModelCard {
            id: "meta-llama/Meta-Llama-3-8B-Instruct".into(),
            name: "Llama 3 8B Instruct".into(),
            provider: "huggingface".into(),
            params_b: 8.0,
            architecture: "llama".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 8192,
            quantizations: vec![
                "Q8_0".into(),
                "Q6_K".into(),
                "Q5_K_M".into(),
                "Q4_K_M".into(),
                "Q3_K_M".into(),
            ],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/meta-llama/Meta-Llama-3-8B-Instruct".into()),
            size_gb: Some(4.5),
        },
        ModelCard {
            id: "meta-llama/Meta-Llama-3-70B-Instruct".into(),
            name: "Llama 3 70B Instruct".into(),
            provider: "huggingface".into(),
            params_b: 70.0,
            architecture: "llama".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 8192,
            quantizations: vec!["Q4_K_M".into(), "Q3_K_M".into(), "Q2_K".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/meta-llama/Meta-Llama-3-70B-Instruct".into()),
            size_gb: Some(40.0),
        },
        // Mistral family
        ModelCard {
            id: "mistralai/Mistral-7B-Instruct-v0.3".into(),
            name: "Mistral 7B Instruct v0.3".into(),
            provider: "huggingface".into(),
            params_b: 7.3,
            architecture: "mistral".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 32768,
            quantizations: vec![
                "Q8_0".into(),
                "Q6_K".into(),
                "Q5_K_M".into(),
                "Q4_K_M".into(),
                "Q3_K_M".into(),
            ],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/mistralai/Mistral-7B-Instruct-v0.3".into()),
            size_gb: Some(4.1),
        },
        ModelCard {
            id: "mistralai/Mixtral-8x7B-Instruct-v0.1".into(),
            name: "Mixtral 8x7B Instruct".into(),
            provider: "huggingface".into(),
            params_b: 46.7,
            architecture: "mistral".into(),
            is_moe: true,
            active_params_b: Some(12.9),
            context_length: 32768,
            quantizations: vec!["Q4_K_M".into(), "Q3_K_M".into(), "Q2_K".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/mistralai/Mixtral-8x7B-Instruct-v0.1".into()),
            size_gb: Some(26.0),
        },
        // Qwen 2.5 family
        ModelCard {
            id: "Qwen/Qwen2.5-7B-Instruct".into(),
            name: "Qwen 2.5 7B Instruct".into(),
            provider: "huggingface".into(),
            params_b: 7.6,
            architecture: "qwen".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 32768,
            quantizations: vec![
                "Q8_0".into(),
                "Q6_K".into(),
                "Q5_K_M".into(),
                "Q4_K_M".into(),
                "Q3_K_M".into(),
            ],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/Qwen/Qwen2.5-7B-Instruct".into()),
            size_gb: Some(4.3),
        },
        ModelCard {
            id: "Qwen/Qwen2.5-32B-Instruct".into(),
            name: "Qwen 2.5 32B Instruct".into(),
            provider: "huggingface".into(),
            params_b: 32.5,
            architecture: "qwen".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 32768,
            quantizations: vec!["Q4_K_M".into(), "Q3_K_M".into(), "Q2_K".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/Qwen/Qwen2.5-32B-Instruct".into()),
            size_gb: Some(18.3),
        },
        ModelCard {
            id: "Qwen/Qwen2.5-72B-Instruct".into(),
            name: "Qwen 2.5 72B Instruct".into(),
            provider: "huggingface".into(),
            params_b: 72.7,
            architecture: "qwen".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 32768,
            quantizations: vec!["Q4_K_M".into(), "Q3_K_M".into(), "Q2_K".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/Qwen/Qwen2.5-72B-Instruct".into()),
            size_gb: Some(41.0),
        },
        // Phi family
        ModelCard {
            id: "microsoft/Phi-3-mini-4k-instruct".into(),
            name: "Phi-3 Mini 4K Instruct".into(),
            provider: "huggingface".into(),
            params_b: 3.8,
            architecture: "phi".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 4096,
            quantizations: vec![
                "Q8_0".into(),
                "Q6_K".into(),
                "Q5_K_M".into(),
                "Q4_K_M".into(),
            ],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/microsoft/Phi-3-mini-4k-instruct".into()),
            size_gb: Some(2.1),
        },
        ModelCard {
            id: "microsoft/Phi-3-medium-128k-instruct".into(),
            name: "Phi-3 Medium 128K Instruct".into(),
            provider: "huggingface".into(),
            params_b: 14.0,
            architecture: "phi".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 131072,
            quantizations: vec!["Q4_K_M".into(), "Q3_K_M".into(), "Q2_K".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/microsoft/Phi-3-medium-128k-instruct".into()),
            size_gb: Some(7.9),
        },
        // Gemma family
        ModelCard {
            id: "google/gemma-2-9b-it".into(),
            name: "Gemma 2 9B IT".into(),
            provider: "huggingface".into(),
            params_b: 9.2,
            architecture: "gemma".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 8192,
            quantizations: vec![
                "Q8_0".into(),
                "Q6_K".into(),
                "Q5_K_M".into(),
                "Q4_K_M".into(),
                "Q3_K_M".into(),
            ],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/google/gemma-2-9b-it".into()),
            size_gb: Some(5.2),
        },
        ModelCard {
            id: "google/gemma-2-27b-it".into(),
            name: "Gemma 2 27B IT".into(),
            provider: "huggingface".into(),
            params_b: 27.0,
            architecture: "gemma".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 8192,
            quantizations: vec!["Q4_K_M".into(), "Q3_K_M".into(), "Q2_K".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/google/gemma-2-27b-it".into()),
            size_gb: Some(15.2),
        },
        // DeepSeek family
        ModelCard {
            id: "deepseek-ai/DeepSeek-R1-Distill-Qwen-7B".into(),
            name: "DeepSeek R1 Distill Qwen 7B".into(),
            provider: "huggingface".into(),
            params_b: 7.6,
            architecture: "qwen".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 131072,
            quantizations: vec![
                "Q8_0".into(),
                "Q6_K".into(),
                "Q5_K_M".into(),
                "Q4_K_M".into(),
                "Q3_K_M".into(),
            ],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some(
                "https://huggingface.co/deepseek-ai/DeepSeek-R1-Distill-Qwen-7B".into(),
            ),
            size_gb: Some(4.3),
        },
        ModelCard {
            id: "deepseek-ai/DeepSeek-R1-Distill-Llama-8B".into(),
            name: "DeepSeek R1 Distill Llama 8B".into(),
            provider: "huggingface".into(),
            params_b: 8.0,
            architecture: "llama".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 131072,
            quantizations: vec![
                "Q8_0".into(),
                "Q6_K".into(),
                "Q5_K_M".into(),
                "Q4_K_M".into(),
                "Q3_K_M".into(),
            ],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some(
                "https://huggingface.co/deepseek-ai/DeepSeek-R1-Distill-Llama-8B".into(),
            ),
            size_gb: Some(4.5),
        },
        // Small models for CPU-only
        ModelCard {
            id: "TinyLlama/TinyLlama-1.1B-Chat-v1.0".into(),
            name: "TinyLlama 1.1B Chat".into(),
            provider: "huggingface".into(),
            params_b: 1.1,
            architecture: "llama".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 2048,
            quantizations: vec![
                "Q8_0".into(),
                "Q6_K".into(),
                "Q5_K_M".into(),
                "Q4_K_M".into(),
                "Q3_K_M".into(),
            ],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: Some("https://huggingface.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0".into()),
            size_gb: Some(0.6),
        },
        // Vision models
        ModelCard {
            id: "llava-hf/llava-1.5-7b-hf".into(),
            name: "LLaVA 1.5 7B".into(),
            provider: "huggingface".into(),
            params_b: 7.0,
            architecture: "llama".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 4096,
            quantizations: vec!["Q4_K_M".into(), "Q3_K_M".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: true,
            source_url: Some("https://huggingface.co/llava-hf/llava-1.5-7b-hf".into()),
            size_gb: Some(4.0),
        },
    ]
}
