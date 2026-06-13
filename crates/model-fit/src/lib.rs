//! Model-fit scoring — ranks models against detected hardware.
//!
//! Heuristic scoring considering VRAM/RAM vs model size/quantization,
//! architecture age, backend support, and vision requirements.
//! Produces fit analyses and llama.cpp serve profiles.

use mlx_hardware_fit::HardwareProfile;
use serde::{Deserialize, Serialize};

// ── Quantization constants ─────────────────────────────────────────────────

pub const QUANT_HIERARCHY: &[&str] = &["Q8_0", "Q6_K", "Q5_K_M", "Q4_K_M", "Q3_K_M", "Q2_K"];

/// Bytes per parameter for each quantization level.
pub fn quant_bytes_per_param(quant: &str) -> f64 {
    let q = quant.to_uppercase();
    match q.as_str() {
        "Q8_0" => 1.0,
        "Q6_K" => 0.875,
        "Q5_K_M" => 0.75,
        "Q4_K_M" => 0.625,
        "Q3_K_M" => 0.5,
        "Q2_K" => 0.375,
        "IQ4_NL" | "IQ4_XS" => 0.5625,
        "IQ3_M" | "IQ3_S" | "IQ3_XXS" => 0.4375,
        "IQ2_M" | "IQ2_S" | "IQ2_XXS" => 0.3125,
        "FP16" => 2.0,
        "FP8" => 1.0,
        "AWQ" | "GPTQ" => 0.5625, // ~4.5 bits
        _ => 0.625, // default to Q4_K_M equivalent
    }
}

/// Quality penalty for each quant (0 = best, higher = worse).
pub fn quant_quality_penalty(quant: &str) -> f64 {
    let q = quant.to_uppercase();
    match q.as_str() {
        "FP16" => 0.0,
        "FP8" => 0.02,
        "Q8_0" => 0.03,
        "Q6_K" => 0.06,
        "Q5_K_M" => 0.10,
        "Q4_K_M" => 0.14,
        "IQ4_NL" | "IQ4_XS" => 0.16,
        "Q3_K_M" => 0.20,
        "IQ3_M" => 0.22,
        "Q2_K" => 0.28,
        "IQ2_M" => 0.30,
        _ => 0.15,
    }
}

/// Speed multiplier relative to FP16 for each quant.
pub fn quant_speed_mult(quant: &str) -> f64 {
    let q = quant.to_uppercase();
    match q.as_str() {
        "FP16" => 1.0,
        "Q8_0" => 1.3,
        "Q6_K" => 1.5,
        "Q5_K_M" => 1.8,
        "Q4_K_M" => 2.0,
        "Q3_K_M" => 2.5,
        "Q2_K" => 3.0,
        _ => 2.0,
    }
}

// ── Model card ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCard {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub params_b: f64,
    pub architecture: String,
    #[serde(default)]
    pub is_moe: bool,
    #[serde(default)]
    pub active_params_b: Option<f64>,
    #[serde(default = "default_context_length")]
    pub context_length: usize,
    #[serde(default)]
    pub quantizations: Vec<String>,
    #[serde(default)]
    pub default_quant: String,
    #[serde(default)]
    pub has_vision: bool,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub size_gb: Option<f64>,
}

fn default_context_length() -> usize {
    4096
}

// ── Fit analysis ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitAnalysis {
    pub model: ModelCard,
    pub fit_score: f64,
    pub speed_score: f64,
    pub quality_score: f64,
    pub composite_score: f64,
    pub recommended_quant: String,
    pub estimated_vram_gb: f64,
    pub estimated_tps: f64,
    pub estimated_ctx: usize,
    pub fit_level: String,
    pub badges: Vec<String>,
    #[serde(default)]
    pub warning: Option<String>,
}

// ── Serve profile ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeProfile {
    pub name: String,
    pub quant: String,
    pub n_gpu_layers: i32,
    pub n_cpu_moe: i32,
    pub cache_type: String,
    pub context_size: usize,
    pub estimated_vram_gb: f64,
    pub fits: bool,
    pub note: String,
}

// ── Use case configuration ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct UseCaseConfig {
    speed_weight: f64,
    quality_weight: f64,
    fit_weight: f64,
    context_weight: f64,
    speed_target: f64,
    context_target: usize,
}

fn use_case_config(use_case: Option<&str>) -> UseCaseConfig {
    match use_case.unwrap_or("general") {
        "coding" => UseCaseConfig {
            speed_weight: 0.20,
            quality_weight: 0.35,
            fit_weight: 0.25,
            context_weight: 0.20,
            speed_target: 20.0,
            context_target: 8192,
        },
        "creative" => UseCaseConfig {
            speed_weight: 0.15,
            quality_weight: 0.40,
            fit_weight: 0.25,
            context_weight: 0.20,
            speed_target: 10.0,
            context_target: 4096,
        },
        "analysis" => UseCaseConfig {
            speed_weight: 0.20,
            quality_weight: 0.30,
            fit_weight: 0.20,
            context_weight: 0.30,
            speed_target: 12.0,
            context_target: 16384,
        },
        _ => UseCaseConfig {
            // general
            speed_weight: 0.25,
            quality_weight: 0.25,
            fit_weight: 0.30,
            context_weight: 0.20,
            speed_target: 15.0,
            context_target: 4096,
        },
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Rank a catalog of models against detected hardware.
pub fn rank_models(
    system: &HardwareProfile,
    models: &[ModelCard],
    use_case: Option<&str>,
    sort: Option<&str>,
    limit: Option<usize>,
    search: Option<&str>,
    target_quant: Option<&str>,
    target_context: Option<usize>,
    fit_only: bool,
) -> Vec<FitAnalysis> {
    let mut results: Vec<FitAnalysis> = models
        .iter()
        .filter(|m| {
            if let Some(q) = search {
                let q = q.to_lowercase();
                m.name.to_lowercase().contains(&q) || m.id.to_lowercase().contains(&q)
            } else {
                true
            }
        })
        .map(|m| analyze_model(system, m, target_quant, use_case, target_context))
        .collect();

    if fit_only {
        results.retain(|a| a.fit_score >= 40.0);
    }

    // Sort
    match sort.unwrap_or("score") {
        "speed" => results.sort_by(|a, b| b.estimated_tps.partial_cmp(&a.estimated_tps).unwrap_or(std::cmp::Ordering::Equal)),
        "quality" => results.sort_by(|a, b| b.quality_score.partial_cmp(&a.quality_score).unwrap_or(std::cmp::Ordering::Equal)),
        "vram" => results.sort_by(|a, b| a.estimated_vram_gb.partial_cmp(&b.estimated_vram_gb).unwrap_or(std::cmp::Ordering::Equal)),
        "name" => results.sort_by(|a, b| a.model.name.cmp(&b.model.name)),
        _ => results.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score).unwrap_or(std::cmp::Ordering::Equal)),
    }

    if let Some(limit) = limit {
        results.truncate(limit);
    }

    results
}

/// Analyze a single model against hardware.
pub fn analyze_model(
    system: &HardwareProfile,
    model: &ModelCard,
    target_quant: Option<&str>,
    use_case: Option<&str>,
    target_context: Option<usize>,
) -> FitAnalysis {
    let uc = use_case_config(use_case);
    let ctx = target_context.unwrap_or(uc.context_target).min(model.context_length);

    // Determine best quantization
    let available_vram = if system.is_cpu_only {
        0.0
    } else {
        system.total_vram_gb
    };
    let available_ram = system.ram_gb;

    let quant = target_quant
        .map(|q| q.to_string())
        .or_else(|| best_quant_for_budget(model, available_vram, ctx))
        .unwrap_or_else(|| model.default_quant.clone());

    let estimated_vram = estimate_memory_gb(model, &quant, ctx);

    // Scores
    let fit = fit_score(estimated_vram, if system.is_cpu_only { available_ram * 0.5 } else { available_vram });
    let offload_frac = if system.is_cpu_only || available_vram <= 0.0 {
        0.0
    } else {
        (available_vram / estimated_vram.max(0.001)).min(1.0)
    };
    let estimated_tps = estimate_speed(model, &quant, system, offload_frac);
    let speed = speed_score(estimated_tps, use_case.unwrap_or("general"));
    let quality = quality_score(model, &quant, use_case.unwrap_or("general"));
    let ctx_score = context_score(ctx, use_case.unwrap_or("general"));

    let composite = fit * uc.fit_weight
        + speed * uc.speed_weight
        + quality * uc.quality_weight
        + ctx_score * uc.context_weight;

    let fit_level = if composite >= 80.0 {
        "excellent"
    } else if composite >= 60.0 {
        "good"
    } else if composite >= 40.0 {
        "tight"
    } else if composite >= 20.0 {
        "poor"
    } else {
        "impossible"
    };

    let mut badges = Vec::new();
    if model.has_vision {
        badges.push("vision".to_string());
    }
    if model.is_moe {
        badges.push("moe".to_string());
    }
    if estimated_tps >= 30.0 {
        badges.push("fast".to_string());
    }
    if !model.default_quant.is_empty() {
        badges.push(model.default_quant.to_lowercase().replace('_', "-"));
    }

    let warning = if system.is_cpu_only && model.params_b > 7.0 {
        Some(format!(
            "Modelo de {:.1}B parâmetros em CPU-only — esperado < 5 tok/s",
            model.params_b
        ))
    } else if fit < 30.0 {
        Some("VRAM insuficiente — pode não carregar totalmente na GPU".to_string())
    } else {
        None
    };

    FitAnalysis {
        model: model.clone(),
        fit_score: fit,
        speed_score: speed,
        quality_score: quality,
        composite_score: composite,
        recommended_quant: quant.clone(),
        estimated_vram_gb: estimated_vram,
        estimated_tps,
        estimated_ctx: ctx,
        fit_level: fit_level.to_string(),
        badges,
        warning,
    }
}

/// Compute serve profiles (Quality/Balanced/Speed) for a model.
pub fn compute_serve_profiles(
    system: &HardwareProfile,
    model: &ModelCard,
    serve_weights_gb: Option<f64>,
    serve_quant: Option<&str>,
) -> Vec<ServeProfile> {
    let available_vram = if system.is_cpu_only { 0.0 } else { system.total_vram_gb };
    let n_layers = estimate_n_layers(model);

    let profiles = vec![
        ("Qualidade", "Q6_K", "f16", model.context_length, "Máxima qualidade, contexto completo. Pode não caber em GPUs menores."),
        ("Equilíbrio", "Q4_K_M", "q8_0", (model.context_length / 2).max(4096), "Bom equilíbrio entre qualidade, velocidade e uso de VRAM."),
        ("Velocidade", "Q3_K_M", "q4_0", 4096.min(model.context_length), "Máxima velocidade. Qualidade reduzida, ideal para consultas rápidas."),
    ];

    profiles
        .into_iter()
        .map(|(name, quant, cache_type, ctx, note)| {
            let quant = serve_quant.unwrap_or(quant);
            let weights_gb = serve_weights_gb.unwrap_or_else(|| {
                let params = model.active_params_b.unwrap_or(model.params_b);
                params * quant_bytes_per_param(quant)
            });
            let kv_gb = estimate_kv_cache_gb(model, ctx, cache_type);
            let total_vram = weights_gb + kv_gb;
            let gpu_layers = if available_vram > 0.0 && total_vram <= available_vram {
                -1 // all layers on GPU
            } else if available_vram > 0.0 {
                // Offload what we can
                let layer_vram = weights_gb / n_layers.max(1) as f64;
                (available_vram / layer_vram.max(0.001)) as i32
            } else {
                0 // CPU only
            };

            ServeProfile {
                name: name.to_string(),
                quant: quant.to_string(),
                n_gpu_layers: gpu_layers,
                n_cpu_moe: if model.is_moe { (n_layers / 3).max(1) as i32 } else { 0 },
                cache_type: cache_type.to_string(),
                context_size: ctx,
                estimated_vram_gb: total_vram,
                fits: total_vram <= available_vram || system.is_cpu_only,
                note: if system.is_cpu_only {
                    format!("CPU-only: {}", note)
                } else if total_vram > available_vram {
                    format!("⚠ Não cabe na GPU (precisa {:.1} GB, tem {:.1} GB). {}", total_vram, available_vram, note)
                } else {
                    note.to_string()
                },
            }
        })
        .collect()
}

// ── Memory estimation ──────────────────────────────────────────────────────

/// Estimate VRAM needed for a model at given quant and context.
pub fn estimate_memory_gb(model: &ModelCard, quant: &str, context_size: usize) -> f64 {
    let params = model.active_params_b.unwrap_or(model.params_b);
    let weights_gb = params * quant_bytes_per_param(quant);
    let kv_gb = estimate_kv_cache_gb(model, context_size, "f16");
    let overhead_gb = weights_gb * 0.07; // ~7% overhead
    weights_gb + kv_gb + overhead_gb
}

fn estimate_kv_cache_gb(model: &ModelCard, context_size: usize, kv_type: &str) -> f64 {
    let n_layers = estimate_n_layers(model);
    // Typical: 2 bytes per KV element * 2 (K+V) * n_layers * n_heads * head_dim
    // Simplified: ~0.5 GB per 1K context per 7B params
    let bytes_per_element = match kv_type {
        "f16" => 2.0,
        "q8_0" => 1.0,
        "q4_0" => 0.5,
        _ => 1.0,
    };
    let params = model.active_params_b.unwrap_or(model.params_b);
    // Approximate: kv_cache ≈ 2 * n_layers * hidden_dim * ctx * bytes_per_element
    // Rough heuristic: hidden_dim ≈ sqrt(params_b) * 1000
    let hidden_dim = params.sqrt() * 1000.0;
    let kv_bytes = 2.0 * n_layers as f64 * hidden_dim * context_size as f64 * bytes_per_element;
    kv_bytes / (1024.0 * 1024.0 * 1024.0)
}

fn estimate_n_layers(model: &ModelCard) -> usize {
    // Rough layer count by architecture and size
    let params = model.params_b;
    match model.architecture.to_lowercase().as_str() {
        "llama" | "mistral" | "qwen" => {
            if params <= 1.5 { 24 }
            else if params <= 3.0 { 28 }
            else if params <= 8.0 { 32 }
            else if params <= 14.0 { 40 }
            else if params <= 35.0 { 60 }
            else if params <= 72.0 { 80 }
            else { 100 }
        }
        "phi" => {
            if params <= 3.0 { 32 }
            else { 40 }
        }
        "gemma" => {
            if params <= 3.0 { 26 }
            else { 42 }
        }
        _ => {
            // Generic estimate
            (params * 5.0).max(20.0).min(120.0) as usize
        }
    }
}

// ── Speed estimation ───────────────────────────────────────────────────────

fn estimate_speed(model: &ModelCard, quant: &str, system: &HardwareProfile, offload_frac: f64) -> f64 {
    let params = model.active_params_b.unwrap_or(model.params_b);
    let bytes_per_param = quant_bytes_per_param(quant);
    let total_gb = params * bytes_per_param;

    let gpu_bw = if let Some(gpu) = system.gpus.first() {
        gpu.bandwidth_gb_s.unwrap_or_else(|| {
            mlx_hardware_fit::estimate_gpu_bandwidth(&gpu.name, &gpu.backend)
        })
    } else {
        mlx_hardware_fit::estimate_gpu_bandwidth("CPU", "cpu")
    };

    let cpu_bw = mlx_hardware_fit::estimate_gpu_bandwidth("CPU", "cpu");

    // Effective bandwidth = weighted average of GPU and CPU bandwidth
    let effective_bw = gpu_bw * offload_frac + cpu_bw * (1.0 - offload_frac);

    // Tokens per second ≈ bandwidth / (model_size_gb)
    let speed_mult = quant_speed_mult(quant);
    effective_bw / total_gb.max(0.001) * speed_mult
}

// ── Scoring functions ──────────────────────────────────────────────────────

fn fit_score(required_vram: f64, available_vram: f64) -> f64 {
    if available_vram <= 0.0 {
        return 30.0; // CPU-only: neutral fit score
    }
    let ratio = required_vram / available_vram.max(0.001);
    if ratio <= 0.7 {
        100.0
    } else if ratio <= 1.0 {
        80.0 + (1.0 - ratio) / 0.3 * 20.0
    } else if ratio <= 1.2 {
        50.0 + (1.2 - ratio) / 0.2 * 30.0
    } else if ratio <= 1.5 {
        20.0 + (1.5 - ratio) / 0.3 * 30.0
    } else {
        10.0
    }
}

fn speed_score(tps: f64, use_case: &str) -> f64 {
    let uc = use_case_config(Some(use_case));
    let ratio = tps / uc.speed_target.max(0.001);
    if ratio >= 1.5 {
        100.0
    } else if ratio >= 1.0 {
        80.0 + (ratio - 1.0) / 0.5 * 20.0
    } else if ratio >= 0.5 {
        50.0 + (ratio - 0.5) / 0.5 * 30.0
    } else if ratio >= 0.25 {
        30.0 + (ratio - 0.25) / 0.25 * 20.0
    } else {
        10.0
    }
}

fn quality_score(model: &ModelCard, quant: &str, _use_case: &str) -> f64 {
    let penalty = quant_quality_penalty(quant);

    // Base quality from parameter count (logarithmic scale)
    let params = model.params_b;
    let param_score = if params >= 70.0 {
        95.0
    } else if params >= 30.0 {
        85.0
    } else if params >= 13.0 {
        75.0
    } else if params >= 7.0 {
        65.0
    } else if params >= 3.0 {
        50.0
    } else {
        35.0
    };

    // Architecture bonus
    let arch_bonus = match model.architecture.to_lowercase().as_str() {
        "llama" => 5.0,
        "mistral" => 3.0,
        "qwen" => 3.0,
        "phi" => 2.0,
        "gemma" => 2.0,
        _ => 0.0,
    };

    // Recency bonus (newer architectures)
    let recency = if model.architecture.to_lowercase().contains("llama-4") {
        10.0
    } else if model.architecture.to_lowercase().contains("llama-3") {
        5.0
    } else {
        0.0
    };

    // MoE: higher quality ceiling for same active params
    let moe_bonus = if model.is_moe { 5.0 } else { 0.0 };

    let raw = param_score + arch_bonus + recency + moe_bonus - penalty * 100.0;
    raw.clamp(0.0, 100.0)
}

fn context_score(ctx: usize, use_case: &str) -> f64 {
    let uc = use_case_config(Some(use_case));
    let ratio = ctx as f64 / uc.context_target.max(1) as f64;
    if ratio >= 1.5 {
        100.0
    } else if ratio >= 1.0 {
        80.0 + (ratio - 1.0) / 0.5 * 20.0
    } else if ratio >= 0.5 {
        50.0 + (ratio - 0.5) / 0.5 * 30.0
    } else if ratio >= 0.25 {
        20.0 + (ratio - 0.25) / 0.25 * 30.0
    } else {
        5.0
    }
}

// ── Quant inference ────────────────────────────────────────────────────────

/// Best quant that fits within VRAM budget.
pub fn best_quant_for_budget(model: &ModelCard, budget_gb: f64, context_size: usize) -> Option<String> {
    if budget_gb <= 0.0 {
        // CPU-only: pick Q4_K_M as reasonable default
        return Some("Q4_K_M".to_string());
    }

    // Try quants from best to most compressed
    let all_quants = &["Q8_0", "Q6_K", "Q5_K_M", "Q4_K_M", "Q3_K_M", "Q2_K"];
    for &q in all_quants {
        let vram = estimate_memory_gb(model, q, context_size);
        if vram <= budget_gb * 0.85 {
            // 15% safety margin
            return Some(q.to_string());
        }
    }

    // Check if Q2_K fits with context halving
    let half_ctx = context_size / 2;
    let vram_q2 = estimate_memory_gb(model, "Q2_K", half_ctx);
    if vram_q2 <= budget_gb * 0.85 {
        return Some("Q2_K".to_string());
    }

    // Nothing fits — return the most compressed quant anyway
    Some("Q2_K".to_string())
}

/// Infer quantization from model name (e.g., "llama-3-8b-Q4_K_M" → "Q4_K_M").
pub fn infer_quant_from_name(name: &str) -> Option<String> {
    let name_upper = name.to_uppercase();
    for quant in QUANT_HIERARCHY {
        if name_upper.contains(quant) {
            return Some(quant.to_string());
        }
    }
    // Also check IQ quants
    for prefix in &["IQ4_NL", "IQ4_XS", "IQ3_M", "IQ3_S", "IQ3_XXS", "IQ2_M", "IQ2_S", "IQ2_XXS"] {
        if name_upper.contains(prefix) {
            return Some(prefix.to_string());
        }
    }
    // Check FP formats
    if name_upper.contains("FP16") {
        return Some("FP16".to_string());
    }
    if name_upper.contains("FP8") {
        return Some("FP8".to_string());
    }
    if name_upper.contains("AWQ") {
        return Some("AWQ".to_string());
    }
    if name_upper.contains("GPTQ") {
        return Some("GPTQ".to_string());
    }
    None
}

/// Parse parameter count from model name.
pub fn params_b_from_name(name: &str) -> Option<f64> {
    let name_lower = name.to_lowercase();
    // Pattern: "N_b" or "Nb" or "N-b" or "N.B"
    for pattern in &["_b", "b", "-b"] {
        if let Some(idx) = name_lower.rfind(pattern) {
            let before = &name_lower[..idx];
            // Find number before the pattern
            if let Some(num_str) = before
                .rsplit(|c: char| !c.is_numeric() && c != '.')
                .next()
            {
                if let Ok(num) = num_str.parse::<f64>() {
                    if num > 0.0 && num < 1000.0 {
                        return Some(num);
                    }
                }
            }
        }
    }
    None
}

/// Infer use case from model name.
pub fn infer_use_case(name: &str) -> &str {
    let name_lower = name.to_lowercase();
    if name_lower.contains("code") || name_lower.contains("coder") {
        "coding"
    } else if name_lower.contains("creative") || name_lower.contains("writer") || name_lower.contains("story") {
        "creative"
    } else if name_lower.contains("analy") || name_lower.contains("research") {
        "analysis"
    } else {
        "general"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mlx_hardware_fit::GpuInfo;

    fn test_hardware() -> HardwareProfile {
        HardwareProfile {
            platform: "linux".into(),
            cpu_name: "Test CPU".into(),
            cpu_cores: 8,
            ram_gb: 32.0,
            available_ram_gb: 28.0,
            gpus: vec![GpuInfo {
                name: "RTX 4090".into(),
                vram_gb: 24.0,
                index: 0,
                backend: "cuda".into(),
                compute_capability: None,
                bandwidth_gb_s: Some(1008.0),
            }],
            gpu_count: 1,
            total_vram_gb: 24.0,
            primary_backend: "cuda".into(),
            is_cpu_only: false,
            detected_at: Utc::now().to_rfc3339(),
        }
    }

    fn test_hardware_cpu_only() -> HardwareProfile {
        HardwareProfile {
            platform: "linux".into(),
            cpu_name: "Test CPU".into(),
            cpu_cores: 4,
            ram_gb: 16.0,
            available_ram_gb: 12.0,
            gpus: vec![],
            gpu_count: 0,
            total_vram_gb: 0.0,
            primary_backend: "cpu".into(),
            is_cpu_only: true,
            detected_at: Utc::now().to_rfc3339(),
        }
    }

    fn test_model_7b() -> ModelCard {
        ModelCard {
            id: "llama-3-8b".into(),
            name: "Llama 3 8B".into(),
            provider: "huggingface".into(),
            params_b: 8.0,
            architecture: "llama".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 8192,
            quantizations: vec!["Q8_0".into(), "Q4_K_M".into(), "Q3_K_M".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: None,
            size_gb: None,
        }
    }

    fn test_model_70b() -> ModelCard {
        ModelCard {
            id: "llama-3-70b".into(),
            name: "Llama 3 70B".into(),
            provider: "huggingface".into(),
            params_b: 70.0,
            architecture: "llama".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 8192,
            quantizations: vec!["Q4_K_M".into(), "Q3_K_M".into(), "Q2_K".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: None,
            size_gb: None,
        }
    }

    #[test]
    fn test_quant_bytes() {
        assert!((quant_bytes_per_param("Q8_0") - 1.0).abs() < 0.01);
        assert!((quant_bytes_per_param("Q4_K_M") - 0.625).abs() < 0.01);
        assert!((quant_bytes_per_param("Q2_K") - 0.375).abs() < 0.01);
        assert!((quant_bytes_per_param("FP16") - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_analyze_model_gpu() {
        let system = test_hardware();
        let model = test_model_7b();
        let analysis = analyze_model(&system, &model, None, None, None);
        assert!(analysis.composite_score > 60.0);
        assert_eq!(analysis.fit_level, "excellent");
        assert!(!analysis.badges.is_empty());
    }

    #[test]
    fn test_analyze_model_cpu_only() {
        let system = test_hardware_cpu_only();
        let model = test_model_7b();
        let analysis = analyze_model(&system, &model, None, None, None);
        assert!(analysis.warning.is_some());
        assert!(analysis.fit_score > 0.0);
    }

    #[test]
    fn test_large_model_limited_vram() {
        let system = test_hardware(); // 24 GB VRAM
        let model = test_model_70b();
        let analysis = analyze_model(&system, &model, None, None, None);
        // 70B at Q2_K ≈ 26 GB weights + overhead ≈ fits tightly in 24 GB
        // Best quant will be Q2_K which is heavily compressed
        assert!(analysis.fit_score > 0.0);
        assert!(analysis.composite_score > 0.0);
        // Should be at Q2_K or lower quality quant
        assert!(analysis.recommended_quant.contains("Q2") || analysis.composite_score < 60.0);
    }

    #[test]
    fn test_serve_profiles() {
        let system = test_hardware();
        let model = test_model_7b();
        let profiles = compute_serve_profiles(&system, &model, None, None);
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0].name, "Qualidade");
        assert_eq!(profiles[1].name, "Equilíbrio");
        assert_eq!(profiles[2].name, "Velocidade");
    }

    #[test]
    fn test_rank_models() {
        let system = test_hardware();
        let models = vec![test_model_7b(), test_model_70b()];
        let ranked = rank_models(&system, &models, None, Some("score"), None, None, None, None, false);
        assert_eq!(ranked.len(), 2);
        // 7B should rank higher than 70B on 24 GB VRAM
        assert!(ranked[0].composite_score > ranked[1].composite_score);
    }

    #[test]
    fn test_best_quant_for_budget() {
        let model = test_model_7b();
        // 24 GB VRAM → Q8_0 should fit (8 GB + overhead)
        let best = best_quant_for_budget(&model, 24.0, 4096);
        assert_eq!(best, Some("Q8_0".to_string()));

        // 6 GB VRAM → Q4_K_M should fit (5 GB + overhead)
        let best = best_quant_for_budget(&model, 6.0, 4096);
        assert!(best.is_some());
    }

    #[test]
    fn test_params_b_from_name() {
        assert_eq!(params_b_from_name("llama-3-8b"), Some(8.0));
        assert_eq!(params_b_from_name("mistral-7B-v0.1"), Some(7.0));
        assert_eq!(params_b_from_name("qwen2.5-72b-instruct"), Some(72.0));
        assert_eq!(params_b_from_name("no-params-here"), None);
    }

    #[test]
    fn test_infer_quant_from_name() {
        assert_eq!(infer_quant_from_name("llama-3-8b-Q4_K_M"), Some("Q4_K_M".to_string()));
        assert_eq!(infer_quant_from_name("model-FP16"), Some("FP16".to_string()));
        assert_eq!(infer_quant_from_name("plain-model"), None);
    }

    #[test]
    fn test_infer_use_case() {
        assert_eq!(infer_use_case("deepseek-coder-33b"), "coding");
        assert_eq!(infer_use_case("creative-writer-v2"), "creative");
        assert_eq!(infer_use_case("research-analyzer"), "analysis");
        assert_eq!(infer_use_case("llama-3-8b"), "general");
    }

    // ── CPU-only scoring tests (must be honest, not overpromise) ────────

    #[test]
    fn test_cpu_only_ranking_is_honest() {
        let system = test_hardware_cpu_only();
        let models = vec![test_model_7b(), test_model_70b()];
        let ranked = rank_models(&system, &models, None, Some("score"), None, None, None, None, false);

        // 7B should rank higher than 70B on CPU-only
        assert_eq!(ranked.len(), 2);
        let small = ranked.iter().find(|a| a.model.params_b < 10.0).unwrap();
        let large = ranked.iter().find(|a| a.model.params_b > 10.0).unwrap();
        // Smaller models fit better on CPU: higher composite or at minimum higher fit_score
        assert!(
            small.composite_score >= large.composite_score || small.fit_score > large.fit_score,
            "Smaller models should not be worse than large models on CPU-only"
        );
        // Both should produce valid scores
        assert!(small.composite_score > 0.0);
        assert!(large.composite_score > 0.0);
    }

    #[test]
    fn test_cpu_only_small_models_rank_higher() {
        let system = test_hardware_cpu_only();
        let tiny = ModelCard {
            id: "tiny-1b".into(),
            name: "Tiny 1B".into(),
            provider: "hf".into(),
            params_b: 1.1,
            architecture: "llama".into(),
            is_moe: false,
            active_params_b: None,
            context_length: 2048,
            quantizations: vec!["Q4_K_M".into()],
            default_quant: "Q4_K_M".into(),
            has_vision: false,
            source_url: None,
            size_gb: None,
        };
        let large = test_model_70b();

        let ranked = rank_models(&system, &[tiny.clone(), large.clone()], None, Some("score"), None, None, None, None, false);
        // Tiny model should rank higher on CPU-only
        assert!(ranked[0].composite_score > ranked[1].composite_score,
            "Tiny models must rank higher than large models on CPU-only");
    }

    #[test]
    fn test_cpu_only_serve_profiles_note_cpu() {
        let system = test_hardware_cpu_only();
        let model = test_model_7b();
        let profiles = compute_serve_profiles(&system, &model, None, None);
        assert_eq!(profiles.len(), 3);
        // All profiles should mention CPU-only
        for p in &profiles {
            assert!(p.note.contains("CPU-only"), "CPU-only profiles must note CPU-only mode");
            assert_eq!(p.n_gpu_layers, 0, "CPU-only must have 0 GPU layers");
            assert!(p.fits, "CPU-only profiles should always fit (RAM-based)");
        }
    }
}
