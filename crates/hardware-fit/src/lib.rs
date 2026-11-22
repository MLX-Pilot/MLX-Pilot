//! Hardware detection for model-fit recommendations.
//!
//! Detects CPU, RAM, and GPU (NVIDIA via nvidia-smi, AMD via rocm-smi,
//! Apple Silicon via sysctl/sysinfo) with graceful CPU-only fallback.
//! Results cached with 24h TTL.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Instant;
use tracing::debug;

// ── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vram_gb: f64,
    pub index: u32,
    pub backend: String,
    #[serde(default)]
    pub compute_capability: Option<String>,
    #[serde(default)]
    pub bandwidth_gb_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuGroup {
    pub name: String,
    pub count: usize,
    pub vram_gb_per_gpu: f64,
    pub total_vram_gb: f64,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
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
    pub detected_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct HardwareSimulationOptions {
    pub manual_gpu_count: Option<usize>,
    pub manual_vram_gb: Option<f64>,
    pub manual_ram_gb: Option<f64>,
    pub manual_backend: Option<String>,
    pub manual_gpu_name: Option<String>,
    /// Simulates an Apple Silicon Mac with this much unified memory.
    /// `manual_gpu_name` carries the chip (e.g. "Apple M4 Max"). When set,
    /// it replaces every other GPU/RAM override: on a Mac, CPU and GPU share
    /// one memory pool, so RAM and VRAM cannot be chosen independently.
    pub manual_unified_memory_gb: Option<f64>,
    pub ignore_detected_gpu: bool,
    pub ignore_detected_ram: bool,
}

/// Share of unified memory macOS lets the GPU use. Shared by detection and
/// simulation so a simulated Mac ranks models the same way a real one does.
pub const APPLE_GPU_MEMORY_FRACTION: f64 = 0.75;

/// Published figures for an Apple Silicon chip configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppleSiliconSpec {
    pub cpu_cores: usize,
    pub bandwidth_gb_s: f64,
}

// ── Cache ──────────────────────────────────────────────────────────────────

static CACHE: Mutex<Option<(HardwareProfile, Instant)>> = Mutex::new(None);
const CACHE_TTL_SECS: u64 = 86400; // 24 hours

fn get_cached() -> Option<HardwareProfile> {
    let cache = CACHE.lock().ok()?;
    if let Some((ref profile, timestamp)) = *cache {
        if timestamp.elapsed().as_secs() < CACHE_TTL_SECS {
            return Some(profile.clone());
        }
    }
    None
}

fn set_cached(profile: &HardwareProfile) {
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((profile.clone(), Instant::now()));
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Detect system hardware. Uses cache unless `fresh` is true.
pub async fn detect_system(fresh: bool) -> Result<HardwareProfile, String> {
    if !fresh {
        if let Some(cached) = get_cached() {
            debug!("Returning cached hardware profile");
            return Ok(cached);
        }
    }

    let platform = std::env::consts::OS.to_string();
    debug!("Detecting hardware on platform: {platform}");

    let mut cpu_name = get_cpu_name();
    let cpu_cores = get_cpu_count();
    let ram_gb = get_ram_gb();
    let available_ram_gb = get_available_ram_gb();
    let mut gpus: Vec<GpuInfo> = Vec::new();

    // Try platform-specific GPU detection
    if platform == "macos" {
        match detect_apple_silicon(&cpu_name, ram_gb).await {
            Ok((chip, gpu)) => {
                cpu_name = chip;
                gpus.push(gpu);
            }
            Err(e) => {
                debug!("Apple Silicon detection skipped: {e}");
            }
        }
    }

    // Try NVIDIA (works on Windows and Linux)
    if gpus.is_empty() {
        match detect_nvidia().await {
            Ok(mut nv_gpus) => {
                gpus.append(&mut nv_gpus);
            }
            Err(e) => {
                debug!("NVIDIA detection skipped: {e}");
            }
        }
    }

    // Try AMD (works on Linux, some Windows)
    if gpus.is_empty() {
        match detect_amd().await {
            Ok(mut amd_gpus) => {
                gpus.append(&mut amd_gpus);
            }
            Err(e) => {
                debug!("AMD detection skipped: {e}");
            }
        }
    }

    let gpu_count = gpus.len();
    let total_vram_gb: f64 = gpus.iter().map(|g| g.vram_gb).sum();
    let is_cpu_only = gpus.is_empty();

    // Determine primary backend
    let primary_backend = if gpus.iter().any(|g| g.backend == "cuda") {
        "cuda"
    } else if gpus.iter().any(|g| g.backend == "rocm") {
        "rocm"
    } else if gpus.iter().any(|g| g.backend == "metal") {
        "metal"
    } else if platform == "macos" && cfg!(target_arch = "aarch64") {
        // Apple Silicon without detected GPU — still use Metal
        "metal"
    } else {
        "cpu"
    };

    // Add bandwidth estimates
    for gpu in &mut gpus {
        if gpu.bandwidth_gb_s.is_none() {
            gpu.bandwidth_gb_s = Some(estimate_gpu_bandwidth(&gpu.name, &gpu.backend));
        }
    }

    let profile = HardwareProfile {
        platform,
        cpu_name,
        cpu_cores,
        ram_gb,
        available_ram_gb,
        gpus,
        gpu_count,
        total_vram_gb,
        primary_backend: primary_backend.to_string(),
        is_cpu_only,
        detected_at: Utc::now().to_rfc3339(),
    };

    set_cached(&profile);
    Ok(profile)
}

// ── NVIDIA detection ───────────────────────────────────────────────────────

async fn detect_nvidia() -> Result<Vec<GpuInfo>, String> {
    // Try nvidia-smi with common paths
    let binary = find_binary(
        &["nvidia-smi", "nvidia-smi.exe"],
        &[
            // Common Windows paths
            "C:\\Windows\\System32\\nvidia-smi.exe",
            "C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvidia-smi.exe",
        ],
    );

    let binary = match binary {
        Some(b) => b,
        None => return Err("nvidia-smi not found".to_string()),
    };

    let output = run_cmd(
        &binary,
        &[
            "--query-gpu=name,memory.total,compute_cap",
            "--format=csv,noheader,nounits",
        ],
    )
    .await?;

    let mut gpus = Vec::new();
    for (i, line) in output.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[0].to_string();
        let vram_mib: f64 = parts[1].parse().unwrap_or(0.0);
        let vram_gb = vram_mib / 1024.0;
        let compute_cap = if parts[2].is_empty() || parts[2] == "[Not Supported]" {
            None
        } else {
            Some(parts[2].to_string())
        };

        gpus.push(GpuInfo {
            name,
            vram_gb,
            index: i as u32,
            backend: "cuda".to_string(),
            compute_capability: compute_cap,
            bandwidth_gb_s: None, // filled later
        });
    }

    if gpus.is_empty() {
        Err("nvidia-smi returned no GPUs".to_string())
    } else {
        Ok(gpus)
    }
}

// ── AMD detection ──────────────────────────────────────────────────────────

async fn detect_amd() -> Result<Vec<GpuInfo>, String> {
    let binary = find_binary(
        &["rocm-smi", "rocm-smi.exe"],
        &[
            "C:\\Program Files\\AMD\\ROCm\\bin\\rocm-smi.exe",
            "/opt/rocm/bin/rocm-smi",
        ],
    );

    let binary = match binary {
        Some(b) => b,
        None => return Err("rocm-smi not found".to_string()),
    };

    let output = run_cmd(
        &binary,
        &["--showproductname", "--showmeminfo", "vram", "--csv"],
    )
    .await?;

    let mut gpus = Vec::new();
    for (i, line) in output.lines().skip(1).enumerate() {
        // Skip header line
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts
            .get(1)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("AMD GPU {}", i));
        let vram_mib_str = parts.get(2).map(|s| s.to_string()).unwrap_or_default();
        let vram_mib: f64 = vram_mib_str
            .trim_end_matches(" MiB")
            .trim_end_matches(" MB")
            .parse()
            .unwrap_or(0.0);
        let vram_gb = vram_mib / 1024.0;

        gpus.push(GpuInfo {
            name,
            vram_gb,
            index: i as u32,
            backend: "rocm".to_string(),
            compute_capability: None,
            bandwidth_gb_s: None,
        });
    }

    if gpus.is_empty() {
        // Try alternative: /sys/class/drm on Linux
        if std::env::consts::OS == "linux" {
            return detect_amd_sysfs().await;
        }
        Err("rocm-smi returned no GPUs".to_string())
    } else {
        Ok(gpus)
    }
}

async fn detect_amd_sysfs() -> Result<Vec<GpuInfo>, String> {
    // Scan /sys/class/drm/card*/device for AMD GPUs
    let mut gpus = Vec::new();
    for i in 0..8 {
        let vendor_path = format!("/sys/class/drm/card{}/device/vendor", i);
        if let Ok(vendor) = std::fs::read_to_string(&vendor_path) {
            if vendor.trim() == "0x1002" {
                // AMD vendor ID
                let name = std::fs::read_to_string(format!(
                    "/sys/class/drm/card{}/device/product_name",
                    i
                ))
                .unwrap_or_else(|_| format!("AMD GPU {}", i));
                // Try to read VRAM from mem_info_vram_total
                let vram_bytes = std::fs::read_to_string(format!(
                    "/sys/class/drm/card{}/device/mem_info_vram_total",
                    i
                ))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);

                gpus.push(GpuInfo {
                    name: name.trim().to_string(),
                    vram_gb: vram_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                    index: i,
                    backend: "rocm".to_string(),
                    compute_capability: None,
                    bandwidth_gb_s: None,
                });
            }
        }
    }

    if gpus.is_empty() {
        Err("No AMD GPUs found via sysfs".to_string())
    } else {
        Ok(gpus)
    }
}

// ── Apple Silicon detection ────────────────────────────────────────────────

async fn detect_apple_silicon(cpu_name: &str, ram_gb: f64) -> Result<(String, GpuInfo), String> {
    if std::env::consts::OS != "macos" {
        return Err("Not macOS".to_string());
    }

    detect_apple_silicon_with(
        cpu_name,
        ram_gb,
        cfg!(target_arch = "aarch64"),
        macos_sysctl,
    )
    .await
}

async fn macos_sysctl(key: &'static str) -> Result<String, String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        run_cmd("/usr/sbin/sysctl", &["-n", key]),
    )
    .await
    .map_err(|_| format!("sysctl {key} timed out"))?
}

// Inject only the OS queries: tests exercise the same resolution path on Linux.
async fn detect_apple_silicon_with<F, Fut>(
    cpu_name: &str,
    ram_gb: f64,
    native_arm64: bool,
    mut sysctl: F,
) -> Result<(String, GpuInfo), String>
where
    F: FnMut(&'static str) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    // hw.model identifies the computer (e.g. Mac16,10), not the SoC. XNU
    // exposes the product's SoC name through machdep.cpu.brand_string.
    let brand = sysctl("machdep.cpu.brand_string").await.unwrap_or_default();
    let chip = [&brand, cpu_name]
        .into_iter()
        .map(str::trim)
        .find(|name| is_apple_chip_name(name));

    // Neither an Intel model identifier nor the process architecture alone
    // identifies the host: an x86_64 binary may be running under Rosetta.
    let is_arm64 = if native_arm64 || chip.is_some() {
        true
    } else {
        sysctl("hw.optional.arm64")
            .await
            .is_ok_and(|s| s.trim() == "1")
            || sysctl("sysctl.proc_translated")
                .await
                .is_ok_and(|s| s.trim() == "1")
    };
    if !is_arm64 {
        return Err("Not Apple Silicon (no ARM host evidence)".to_string());
    }
    if !ram_gb.is_finite() || ram_gb <= 0.0 {
        return Err("Invalid unified memory size".to_string());
    }

    let chip = chip.unwrap_or("Apple Silicon");
    if apple_silicon_spec(chip, Some(ram_gb)).is_none() {
        debug!("Unknown Apple chip {chip}; using heuristic bandwidth fallback");
    }
    Ok((chip.to_string(), apple_gpu_info(chip, ram_gb)))
}

fn is_apple_chip_name(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("apple ") && parse_apple_chip(name).is_some()
}

fn estimate_apple_bandwidth(chip: &str, memory_gb: Option<f64>) -> f64 {
    apple_silicon_spec(chip, memory_gb)
        .map(|spec| spec.bandwidth_gb_s)
        .unwrap_or(APPLE_FALLBACK_BANDWIDTH_GB_S)
}

/// Shared by real detection and simulation, including memory-dependent bins.
fn apple_gpu_info(chip: &str, memory_gb: f64) -> GpuInfo {
    GpuInfo {
        name: format!("{chip} GPU"),
        vram_gb: memory_gb * APPLE_GPU_MEMORY_FRACTION,
        index: 0,
        backend: "metal".to_string(),
        compute_capability: None,
        bandwidth_gb_s: Some(estimate_apple_bandwidth(chip, Some(memory_gb))),
    }
}

// A compatibility estimate for unknown chips, not a measured bandwidth or
// a guaranteed lower bound. Keep the previous value without guessing a model.
const APPLE_FALLBACK_BANDWIDTH_GB_S: f64 = 150.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppleTier {
    Base,
    Pro,
    Max,
    Ultra,
}

/// Reads "Apple M4 Max" as (4, Max). The tier must follow the generation
/// token, so a device name like "MacBook Pro (M4)" reads as a base M4.
fn parse_apple_chip(name: &str) -> Option<(u8, AppleTier)> {
    let lower = name.to_ascii_lowercase();
    let mut tokens = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty());
    while let Some(token) = tokens.next() {
        let Some(digits) = token.strip_prefix('m') else {
            continue;
        };
        let Ok(generation) = digits.parse::<u8>() else {
            continue;
        };
        let tier = match tokens.next() {
            Some("pro") => AppleTier::Pro,
            Some("max") => AppleTier::Max,
            Some("ultra") => AppleTier::Ultra,
            _ => AppleTier::Base,
        };
        return Some((generation, tier));
    }
    None
}

/// Apple's published CPU cores and memory bandwidth for a chip. Where the
/// lower-binned variant is the only one sold with a given memory size, that
/// memory size selects it (e.g. M4 Max with 36 GB is the 410 GB/s part);
/// otherwise — or when `unified_memory_gb` is unknown — the top bin is used.
pub fn apple_silicon_spec(
    chip_name: &str,
    unified_memory_gb: Option<f64>,
) -> Option<AppleSiliconSpec> {
    let (generation, tier) = parse_apple_chip(chip_name)?;
    let memory_is = |gb: f64| unified_memory_gb.is_some_and(|m| (m - gb).abs() < 0.5);
    let spec = |cpu_cores: usize, bandwidth_gb_s: f64| AppleSiliconSpec {
        cpu_cores,
        bandwidth_gb_s,
    };

    use AppleTier::*;
    Some(match (generation, tier) {
        (1, Base) => spec(8, 68.0),
        (1, Pro) => spec(10, 200.0),
        (1, Max) => spec(10, 400.0),
        (1, Ultra) => spec(20, 800.0),
        (2, Base) => spec(8, 100.0),
        (2, Pro) => spec(12, 200.0),
        (2, Max) => spec(12, 400.0),
        (2, Ultra) => spec(24, 800.0),
        (3, Base) => spec(8, 100.0),
        (3, Pro) => spec(12, 150.0),
        (3, Max) if memory_is(36.0) || memory_is(96.0) => spec(14, 300.0),
        (3, Max) => spec(16, 400.0),
        (3, Ultra) => spec(32, 819.0),
        (4, Base) => spec(10, 120.0),
        (4, Pro) => spec(14, 273.0),
        (4, Max) if memory_is(36.0) => spec(14, 410.0),
        (4, Max) => spec(16, 546.0),
        (5, Base) => spec(10, 153.0),
        (5, Pro) => spec(18, 307.0),
        (5, Max) if memory_is(36.0) => spec(18, 460.0),
        (5, Max) => spec(18, 614.0),
        (5, Ultra) if memory_is(96.0) => spec(30, 1200.0),
        (5, Ultra) => spec(36, 1200.0),
        (6, Base) if memory_is(16.0) => spec(12, 153.0),
        (6, Base) => spec(12, 170.0),
        _ => return None,
    })
}

// ── sysinfo helpers ────────────────────────────────────────────────────────

fn get_cpu_name() -> String {
    use sysinfo::System;
    let sys = System::new_all();
    sys.cpus()
        .first()
        .map(|cpu| cpu.brand().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string())
}

fn get_cpu_count() -> usize {
    use sysinfo::System;
    let sys = System::new_all();
    sys.physical_core_count()
        .unwrap_or_else(|| sys.cpus().len())
}

fn get_ram_gb() -> f64 {
    use sysinfo::System;
    let sys = System::new_all();
    sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn get_available_ram_gb() -> f64 {
    use sysinfo::System;
    let sys = System::new_all();
    sys.available_memory() as f64 / (1024.0 * 1024.0 * 1024.0)
}

// ── GPU bandwidth estimation ───────────────────────────────────────────────

pub fn estimate_gpu_bandwidth(gpu_name: &str, backend: &str) -> f64 {
    let name_lower = gpu_name.to_lowercase();

    // NVIDIA RTX 50 series
    if name_lower.contains("rtx 5090") {
        return 1792.0;
    }
    if name_lower.contains("rtx 5080") {
        return 960.0;
    }
    if name_lower.contains("rtx 5070") {
        return 672.0;
    }
    if name_lower.contains("rtx 5060") {
        return 448.0;
    }

    // NVIDIA RTX 40 series
    if name_lower.contains("rtx 4090") {
        return 1008.0;
    }
    if name_lower.contains("rtx 4080") {
        return 716.0;
    }
    if name_lower.contains("rtx 4070") {
        return 504.0;
    }
    if name_lower.contains("rtx 4060") {
        return 272.0;
    }

    // NVIDIA RTX 30 series
    if name_lower.contains("rtx 3090") {
        return 936.0;
    }
    if name_lower.contains("rtx 3080") {
        return 760.0;
    }
    if name_lower.contains("rtx 3070") {
        return 448.0;
    }
    if name_lower.contains("rtx 3060") {
        return 360.0;
    }

    // NVIDIA RTX 20 series
    if name_lower.contains("rtx 2080") {
        return 448.0;
    }
    if name_lower.contains("rtx 2070") {
        return 448.0;
    }
    if name_lower.contains("rtx 2060") {
        return 336.0;
    }

    // NVIDIA GTX series
    if name_lower.contains("gtx 1080") {
        return 320.0;
    }
    if name_lower.contains("gtx 1070") {
        return 256.0;
    }
    if name_lower.contains("gtx 1060") {
        return 192.0;
    }
    if name_lower.contains("gtx 1660") {
        return 192.0;
    }
    if name_lower.contains("gtx 1650") {
        return 128.0;
    }

    // NVIDIA data center
    if name_lower.contains("h100") {
        return 3350.0;
    }
    if name_lower.contains("h200") {
        return 4800.0;
    }
    if name_lower.contains("a100") {
        return 2039.0;
    }
    if name_lower.contains("a6000") {
        return 768.0;
    }
    if name_lower.contains("v100") {
        return 900.0;
    }
    if name_lower.contains("t4") {
        return 320.0;
    }

    // AMD Radeon
    if name_lower.contains("7900 xtx") {
        return 960.0;
    }
    if name_lower.contains("7900 xt") {
        return 800.0;
    }
    if name_lower.contains("7800 xt") {
        return 624.0;
    }
    if name_lower.contains("7700 xt") {
        return 432.0;
    }
    if name_lower.contains("7600") {
        return 288.0;
    }
    if name_lower.contains("6950 xt") {
        return 576.0;
    }
    if name_lower.contains("6900 xt") {
        return 512.0;
    }
    if name_lower.contains("6800 xt") {
        return 512.0;
    }
    if name_lower.contains("6800") {
        return 512.0;
    }
    if name_lower.contains("6700 xt") {
        return 384.0;
    }
    if name_lower.contains("6600") {
        return 224.0;
    }
    if name_lower.contains("9070") {
        return 640.0;
    }
    if name_lower.contains("9060") {
        return 432.0;
    }

    // AMD Instinct
    if name_lower.contains("mi300x") {
        return 5300.0;
    }
    if name_lower.contains("mi250x") {
        return 1600.0;
    }
    if name_lower.contains("mi210") {
        return 1600.0;
    }
    if name_lower.contains("mi100") {
        return 1200.0;
    }

    // Apple Silicon (integrated GPU, unified memory)
    if backend == "metal" || name_lower.contains("apple") {
        if let Some(spec) = apple_silicon_spec(gpu_name, None) {
            return spec.bandwidth_gb_s;
        }
    }

    // Fallback by backend
    match backend {
        "cuda" => 220.0,
        "rocm" => 180.0,
        "metal" => 150.0,
        "cpu" => {
            if cfg!(target_arch = "aarch64") {
                90.0
            } else {
                70.0
            }
        }
        _ => 100.0,
    }
}

// ── GPU grouping ───────────────────────────────────────────────────────────

pub fn group_gpus(gpus: &[GpuInfo]) -> Vec<GpuGroup> {
    let mut groups: std::collections::BTreeMap<String, GpuGroup> =
        std::collections::BTreeMap::new();
    for gpu in gpus {
        let key = format!("{}|{}", gpu.name, gpu.backend);
        groups
            .entry(key)
            .and_modify(|g| {
                g.count += 1;
                g.total_vram_gb += gpu.vram_gb;
            })
            .or_insert_with(|| GpuGroup {
                name: gpu.name.clone(),
                count: 1,
                vram_gb_per_gpu: gpu.vram_gb,
                total_vram_gb: gpu.vram_gb,
                backend: gpu.backend.clone(),
            });
    }
    groups.into_values().collect()
}

// ── Manual hardware simulation ─────────────────────────────────────────────

pub fn simulate_hardware(
    base: &HardwareProfile,
    options: HardwareSimulationOptions,
) -> HardwareProfile {
    let mut profile = base.clone();

    if let Some(memory_gb) = options
        .manual_unified_memory_gb
        .filter(|gb| gb.is_finite() && *gb > 0.0)
    {
        let chip = options
            .manual_gpu_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("Apple Silicon");
        apply_simulated_mac(&mut profile, chip, memory_gb);
        return profile;
    }

    if options.ignore_detected_gpu {
        profile.gpus.clear();
        profile.gpu_count = 0;
        profile.total_vram_gb = 0.0;
    }

    if options.ignore_detected_ram {
        profile.ram_gb = 0.0;
        profile.available_ram_gb = 0.0;
    }

    if let Some(ram) = options.manual_ram_gb {
        profile.ram_gb = ram;
        profile.available_ram_gb = ram * 0.9;
    }

    if let Some(backend) = options.manual_backend {
        profile.primary_backend = backend.clone();
        if let Some(count) = options.manual_gpu_count {
            if count == 0 {
                profile.gpus.clear();
                profile.gpu_count = 0;
                profile.total_vram_gb = 0.0;
            } else if let Some(vram) = options.manual_vram_gb {
                profile.gpus.clear();
                for i in 0..count {
                    let gpu_name = options
                        .manual_gpu_name
                        .as_ref()
                        .filter(|name| !name.is_empty())
                        .cloned()
                        .unwrap_or_else(|| format!("Manual GPU {} ({})", i + 1, backend));
                    profile.gpus.push(GpuInfo {
                        name: gpu_name.clone(),
                        vram_gb: vram,
                        index: i as u32,
                        backend: backend.clone(),
                        compute_capability: None,
                        bandwidth_gb_s: Some(estimate_gpu_bandwidth(&gpu_name, &backend)),
                    });
                }
                profile.gpu_count = count;
                profile.total_vram_gb = vram * count as f64;
            }
        }
        profile.is_cpu_only = profile.gpus.is_empty();
    }

    if profile.primary_backend == "cpu" || profile.gpus.is_empty() {
        profile.is_cpu_only = true;
    }

    profile
}

/// Replaces the whole machine with an Apple Silicon Mac: the chip is the CPU
/// and the GPU, and both draw from the same unified memory.
fn apply_simulated_mac(profile: &mut HardwareProfile, chip: &str, memory_gb: f64) {
    let spec = apple_silicon_spec(chip, Some(memory_gb));
    let gpu = apple_gpu_info(chip, memory_gb);
    let vram_gb = gpu.vram_gb;

    profile.platform = "macos".to_string();
    profile.cpu_name = chip.to_string();
    if let Some(spec) = spec {
        profile.cpu_cores = spec.cpu_cores;
    }
    profile.ram_gb = memory_gb;
    profile.available_ram_gb = memory_gb * 0.9;
    profile.gpus = vec![gpu];
    profile.gpu_count = 1;
    profile.total_vram_gb = vram_gb;
    profile.primary_backend = "metal".to_string();
    profile.is_cpu_only = false;
}

// ── Internal helpers ───────────────────────────────────────────────────────

async fn run_cmd(cmd: &str, args: &[&str]) -> Result<String, String> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to run {cmd}: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{cmd} failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

fn find_binary(names: &[&str], extra_paths: &[&str]) -> Option<String> {
    // Try names directly first (will use PATH)
    for name in names {
        if let Ok(output) = std::process::Command::new("which").arg(name).output() {
            if output.status.success() {
                return Some(name.to_string());
            }
        }
        // Windows: try "where"
        if let Ok(output) = std::process::Command::new("where").arg(name).output() {
            if output.status.success() {
                return Some(name.to_string());
            }
        }
    }

    // Try extra paths
    for path in extra_paths {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    // On Windows, try the bare name — it might be on PATH
    if cfg!(windows) {
        for name in names {
            let output = std::process::Command::new("cmd")
                .args(["/c", "where", name])
                .output();
            if let Ok(out) = output {
                if out.status.success() {
                    return Some(name.to_string());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_grouping() {
        let gpus = vec![
            GpuInfo {
                name: "RTX 4090".into(),
                vram_gb: 24.0,
                index: 0,
                backend: "cuda".into(),
                compute_capability: None,
                bandwidth_gb_s: None,
            },
            GpuInfo {
                name: "RTX 4090".into(),
                vram_gb: 24.0,
                index: 1,
                backend: "cuda".into(),
                compute_capability: None,
                bandwidth_gb_s: None,
            },
        ];
        let groups = group_gpus(&gpus);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count, 2);
        assert_eq!(groups[0].total_vram_gb, 48.0);
    }

    #[test]
    fn test_bandwidth_estimation() {
        let bw = estimate_gpu_bandwidth("NVIDIA GeForce RTX 4090", "cuda");
        assert_eq!(bw, 1008.0);
        let bw = estimate_gpu_bandwidth("Unknown GPU", "cuda");
        assert_eq!(bw, 220.0);
    }

    #[test]
    fn test_simulate_hardware() {
        let base = HardwareProfile {
            platform: "linux".into(),
            cpu_name: "Test CPU".into(),
            cpu_cores: 8,
            ram_gb: 32.0,
            available_ram_gb: 28.0,
            gpus: vec![],
            gpu_count: 0,
            total_vram_gb: 0.0,
            primary_backend: "cpu".into(),
            is_cpu_only: true,
            detected_at: Utc::now().to_rfc3339(),
        };

        let simulated = simulate_hardware(
            &base,
            HardwareSimulationOptions {
                manual_gpu_count: Some(1),
                manual_vram_gb: Some(24.0),
                manual_ram_gb: Some(64.0),
                manual_backend: Some("cuda".into()),
                ..Default::default()
            },
        );

        assert_eq!(simulated.gpu_count, 1);
        assert_eq!(simulated.total_vram_gb, 24.0);
        assert_eq!(simulated.ram_gb, 64.0);
        assert_eq!(simulated.primary_backend, "cuda");
        assert!(!simulated.is_cpu_only);
    }

    #[test]
    fn test_simulate_hardware_named_gpu() {
        let base = HardwareProfile {
            platform: "linux".into(),
            cpu_name: "Test CPU".into(),
            cpu_cores: 8,
            ram_gb: 32.0,
            available_ram_gb: 28.0,
            gpus: vec![],
            gpu_count: 0,
            total_vram_gb: 0.0,
            primary_backend: "cpu".into(),
            is_cpu_only: true,
            detected_at: Utc::now().to_rfc3339(),
        };

        let simulated = simulate_hardware(
            &base,
            HardwareSimulationOptions {
                manual_gpu_count: Some(1),
                manual_vram_gb: Some(12.0),
                manual_backend: Some("cuda".into()),
                manual_gpu_name: Some("NVIDIA GeForce RTX 5070".into()),
                ignore_detected_gpu: true,
                ..Default::default()
            },
        );

        assert_eq!(simulated.gpu_count, 1);
        assert_eq!(simulated.total_vram_gb, 12.0);
        assert_eq!(simulated.gpus[0].name, "NVIDIA GeForce RTX 5070");
        assert_eq!(
            simulated.gpus[0].bandwidth_gb_s,
            Some(estimate_gpu_bandwidth("NVIDIA GeForce RTX 5070", "cuda"))
        );
        assert!(simulated.gpus[0].bandwidth_gb_s.unwrap() > estimate_gpu_bandwidth("CPU", "cpu"));
    }

    #[test]
    fn test_simulate_hardware_dual_gpu() {
        let base = HardwareProfile {
            platform: "linux".into(),
            cpu_name: "Test CPU".into(),
            cpu_cores: 8,
            ram_gb: 32.0,
            available_ram_gb: 28.0,
            gpus: vec![],
            gpu_count: 0,
            total_vram_gb: 0.0,
            primary_backend: "cpu".into(),
            is_cpu_only: true,
            detected_at: Utc::now().to_rfc3339(),
        };

        let simulated = simulate_hardware(
            &base,
            HardwareSimulationOptions {
                manual_gpu_count: Some(2),
                manual_vram_gb: Some(12.0),
                manual_backend: Some("cuda".into()),
                manual_gpu_name: Some("NVIDIA GeForce RTX 5070".into()),
                ignore_detected_gpu: true,
                ..Default::default()
            },
        );

        assert_eq!(simulated.gpu_count, 2);
        assert_eq!(simulated.total_vram_gb, 24.0);
    }

    #[test]
    fn test_simulate_hardware_ram_override() {
        let base = HardwareProfile {
            platform: "linux".into(),
            cpu_name: "Test CPU".into(),
            cpu_cores: 8,
            ram_gb: 32.0,
            available_ram_gb: 28.0,
            gpus: vec![],
            gpu_count: 0,
            total_vram_gb: 0.0,
            primary_backend: "cpu".into(),
            is_cpu_only: true,
            detected_at: Utc::now().to_rfc3339(),
        };

        let simulated = simulate_hardware(
            &base,
            HardwareSimulationOptions {
                manual_ram_gb: Some(64.0),
                ignore_detected_ram: true,
                ..Default::default()
            },
        );

        assert_eq!(simulated.ram_gb, 64.0);
        assert!((simulated.available_ram_gb - 57.6).abs() < 0.01);
    }

    #[test]
    fn test_simulate_hardware_cpu_only() {
        let base = HardwareProfile {
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
                bandwidth_gb_s: None,
            }],
            gpu_count: 1,
            total_vram_gb: 24.0,
            primary_backend: "cuda".into(),
            is_cpu_only: false,
            detected_at: Utc::now().to_rfc3339(),
        };

        let simulated = simulate_hardware(
            &base,
            HardwareSimulationOptions {
                manual_gpu_count: Some(0),
                manual_backend: Some("cpu".into()),
                ignore_detected_gpu: true,
                ..Default::default()
            },
        );

        assert!(simulated.gpus.is_empty());
        assert!(simulated.is_cpu_only);
        assert_eq!(simulated.primary_backend, "cpu");
    }

    fn windows_pc_with_rtx_4090() -> HardwareProfile {
        HardwareProfile {
            platform: "windows".into(),
            cpu_name: "Intel Core i7-12700H".into(),
            cpu_cores: 14,
            ram_gb: 32.0,
            available_ram_gb: 20.0,
            gpus: vec![GpuInfo {
                name: "NVIDIA GeForce RTX 4090".into(),
                vram_gb: 24.0,
                index: 0,
                backend: "cuda".into(),
                compute_capability: Some("8.9".into()),
                bandwidth_gb_s: Some(1008.0),
            }],
            gpu_count: 1,
            total_vram_gb: 24.0,
            primary_backend: "cuda".into(),
            is_cpu_only: false,
            detected_at: Utc::now().to_rfc3339(),
        }
    }

    fn simulate_mac(chip: &str, memory_gb: f64) -> HardwareProfile {
        simulate_hardware(
            &windows_pc_with_rtx_4090(),
            HardwareSimulationOptions {
                manual_gpu_name: Some(chip.into()),
                manual_unified_memory_gb: Some(memory_gb),
                ignore_detected_gpu: true,
                ignore_detected_ram: true,
                ..Default::default()
            },
        )
    }

    #[test]
    fn test_simulate_mac_replaces_the_whole_machine() {
        let mac = simulate_mac("Apple M4 Max", 128.0);

        assert_eq!(mac.platform, "macos");
        assert_eq!(mac.cpu_name, "Apple M4 Max");
        assert_eq!(mac.cpu_cores, 16);
        assert_eq!(mac.ram_gb, 128.0);
        assert!((mac.available_ram_gb - 115.2).abs() < 0.01);
        assert_eq!(mac.gpu_count, 1);
        assert_eq!(mac.gpus.len(), 1);
        assert_eq!(mac.gpus[0].name, "Apple M4 Max GPU");
        assert_eq!(mac.gpus[0].backend, "metal");
        assert_eq!(mac.gpus[0].vram_gb, 96.0);
        assert_eq!(mac.gpus[0].bandwidth_gb_s, Some(546.0));
        assert_eq!(mac.total_vram_gb, 96.0);
        assert_eq!(mac.primary_backend, "metal");
        assert!(!mac.is_cpu_only);
    }

    #[test]
    fn test_simulate_mac_ignores_discrete_gpu_overrides() {
        let mac = simulate_hardware(
            &windows_pc_with_rtx_4090(),
            HardwareSimulationOptions {
                manual_gpu_name: Some("Apple M3 Pro".into()),
                manual_unified_memory_gb: Some(36.0),
                manual_gpu_count: Some(4),
                manual_vram_gb: Some(80.0),
                manual_ram_gb: Some(512.0),
                manual_backend: Some("cuda".into()),
                ignore_detected_gpu: true,
                ignore_detected_ram: true,
            },
        );

        assert_eq!(mac.gpu_count, 1);
        assert_eq!(mac.total_vram_gb, 27.0);
        assert_eq!(mac.ram_gb, 36.0);
        assert_eq!(mac.primary_backend, "metal");
    }

    #[test]
    fn test_simulate_mac_bandwidth_follows_memory_bound_bins() {
        let bw = |chip: &str, gb: f64| simulate_mac(chip, gb).gpus[0].bandwidth_gb_s.unwrap();

        assert_eq!(bw("Apple M4 Max", 36.0), 410.0);
        assert_eq!(bw("Apple M4 Max", 48.0), 546.0);
        assert_eq!(bw("Apple M3 Max", 36.0), 300.0);
        assert_eq!(bw("Apple M3 Max", 96.0), 300.0);
        assert_eq!(bw("Apple M3 Max", 128.0), 400.0);
        assert_eq!(bw("Apple M5 Max", 36.0), 460.0);
        assert_eq!(bw("Apple M5 Max", 64.0), 614.0);
        assert_eq!(bw("Apple M6", 16.0), 153.0);
        assert_eq!(bw("Apple M6", 24.0), 170.0);
        assert_eq!(simulate_mac("Apple M5 Ultra", 96.0).cpu_cores, 30);
        assert_eq!(simulate_mac("Apple M5 Ultra", 512.0).cpu_cores, 36);
    }

    #[test]
    fn test_simulate_mac_unknown_chip_falls_back_to_metal_estimate() {
        let base = windows_pc_with_rtx_4090();
        let mac = simulate_mac("Apple M9 Hyper", 64.0);

        assert_eq!(mac.cpu_name, "Apple M9 Hyper");
        assert_eq!(mac.cpu_cores, base.cpu_cores);
        assert_eq!(
            mac.gpus[0].bandwidth_gb_s,
            Some(APPLE_FALLBACK_BANDWIDTH_GB_S)
        );
        assert_eq!(mac.total_vram_gb, 48.0);
    }

    #[test]
    fn test_simulate_mac_without_chip_name_uses_generic_label() {
        let mac = simulate_mac("   ", 16.0);
        assert_eq!(mac.cpu_name, "Apple Silicon");
        assert_eq!(mac.gpus[0].name, "Apple Silicon GPU");
    }

    #[test]
    fn test_simulate_mac_rejects_invalid_memory() {
        for memory_gb in [0.0, -8.0, f64::NAN, f64::INFINITY] {
            let simulated = simulate_hardware(
                &windows_pc_with_rtx_4090(),
                HardwareSimulationOptions {
                    manual_gpu_name: Some("Apple M4".into()),
                    manual_unified_memory_gb: Some(memory_gb),
                    ..Default::default()
                },
            );
            assert_eq!(simulated.platform, "windows", "memory {memory_gb}");
            assert_eq!(simulated.primary_backend, "cuda", "memory {memory_gb}");
        }
    }

    #[test]
    fn test_apple_chip_tier_must_follow_generation() {
        // Device names carry "Pro"/"Air" before the chip; only the word right
        // after the generation is the chip tier.
        assert_eq!(
            parse_apple_chip("MacBook Pro (M4)"),
            Some((4, AppleTier::Base))
        );
        assert_eq!(
            parse_apple_chip("Apple M2 Ultra"),
            Some((2, AppleTier::Ultra))
        );
        // sysctl hw.model identifiers must not be read as chips.
        assert_eq!(parse_apple_chip("MacBookPro18,3"), None);
        assert_eq!(parse_apple_chip("Mac16,1"), None);
        // Non-Apple names with an "M<number>" token are not Apple chips.
        assert_eq!(apple_silicon_spec("Tesla M60", None), None);
        assert_eq!(apple_silicon_spec("Quadro M4000", None), None);
    }

    #[test]
    fn test_bandwidth_estimation_knows_apple_chips() {
        assert_eq!(estimate_gpu_bandwidth("Apple M4 Pro GPU", "metal"), 273.0);
        assert_eq!(estimate_gpu_bandwidth("Apple M1", "metal"), 68.0);
        // Without a memory size the top bin is assumed.
        assert_eq!(estimate_gpu_bandwidth("Apple M4 Max", "metal"), 546.0);
        // Computer model identifiers must not be interpreted as chip names.
        assert_eq!(
            estimate_apple_bandwidth("Mac16,1", Some(24.0)),
            APPLE_FALLBACK_BANDWIDTH_GB_S
        );
    }

    async fn detected_mac(
        brand: Result<&str, &str>,
        sysinfo_brand: &str,
        memory_gb: f64,
        native_arm64: bool,
        arm64: Result<&str, &str>,
        translated: Result<&str, &str>,
    ) -> Result<HardwareProfile, String> {
        let (chip, gpu) =
            detect_apple_silicon_with(sysinfo_brand, memory_gb, native_arm64, |key| {
                let result = match key {
                    "machdep.cpu.brand_string" => brand,
                    "hw.optional.arm64" => arm64,
                    "sysctl.proc_translated" => translated,
                    _ => panic!("unexpected sysctl key: {key}"),
                };
                std::future::ready(result.map(str::to_string).map_err(str::to_string))
            })
            .await?;
        Ok(HardwareProfile {
            platform: "macos".into(),
            cpu_name: chip,
            cpu_cores: 10, // OS-reported cores, not inferred from the spec table.
            ram_gb: memory_gb,
            available_ram_gb: 12.0, // Real memory pressure can differ from simulation.
            total_vram_gb: gpu.vram_gb,
            gpus: vec![gpu],
            gpu_count: 1,
            primary_backend: "metal".into(),
            is_cpu_only: false,
            detected_at: "fixture".into(),
        })
    }

    #[tokio::test]
    async fn detected_m4_24gb_matches_simulated_mac() {
        // Synthetic sysctl output for the user's Mac mini M4 / 24 GiB.
        // hw.model (Mac16,10) is deliberately never queried or mapped.
        let detected = detected_mac(
            Ok("Apple M4\n"),
            "Unknown CPU",
            24.0,
            false,
            Err("unused"),
            Err("unused"),
        )
        .await
        .unwrap();
        let simulated = simulate_mac("Apple M4", 24.0);
        assert_eq!(detected.cpu_name, simulated.cpu_name);
        assert_eq!(detected.cpu_cores, simulated.cpu_cores);
        assert_eq!(detected.ram_gb, simulated.ram_gb);
        assert_eq!(detected.total_vram_gb, 18.0);
        assert_eq!(detected.total_vram_gb, simulated.total_vram_gb);
        assert_eq!(detected.primary_backend, simulated.primary_backend);
        assert_eq!(detected.is_cpu_only, simulated.is_cpu_only);
        assert_eq!(detected.gpu_count, simulated.gpu_count);
        assert_eq!(detected.gpus[0].bandwidth_gb_s, Some(120.0));
        assert_eq!(
            serde_json::to_value(&detected.gpus).unwrap(),
            serde_json::to_value(&simulated.gpus).unwrap()
        );
    }

    #[tokio::test]
    async fn detected_chips_use_memory_dependent_bandwidth_like_simulation() {
        for (chip, gb, bandwidth) in [
            ("Apple M1", 16.0, 68.0),
            ("Apple M2 Pro", 32.0, 200.0),
            ("Apple M3 Max", 36.0, 300.0),
            ("Apple M3 Max", 96.0, 300.0),
            ("Apple M3 Max", 128.0, 400.0),
            ("Apple M4 Pro", 24.0, 273.0),
            ("Apple M4 Max", 36.0, 410.0),
            ("Apple M4 Max", 48.0, 546.0),
        ] {
            let mac = detected_mac(Ok(chip), "", gb, false, Err("unused"), Err("unused"))
                .await
                .unwrap();
            assert_eq!(mac.gpus[0].bandwidth_gb_s, Some(bandwidth), "{chip} {gb}");
            assert_eq!(
                serde_json::to_value(&mac.gpus).unwrap(),
                serde_json::to_value(&simulate_mac(chip, gb).gpus).unwrap()
            );
        }
    }

    #[tokio::test]
    async fn detection_falls_back_to_sysinfo_when_brand_query_fails_or_is_empty() {
        for brand in [Err("sysctl failed"), Ok("  \n")] {
            let mac = detected_mac(brand, "Apple M4", 24.0, false, Err("unused"), Err("unused"))
                .await
                .unwrap();
            assert_eq!(mac.cpu_name, "Apple M4");
            assert_eq!(mac.gpus[0].bandwidth_gb_s, Some(120.0));
        }
    }

    #[tokio::test]
    async fn unknown_apple_chips_keep_fallback_without_guessing_a_generation() {
        let mac = detected_mac(
            Ok("Apple M99"),
            "",
            24.0,
            false,
            Err("unused"),
            Err("unused"),
        )
        .await
        .unwrap();
        assert_eq!(mac.cpu_name, "Apple M99");
        assert_eq!(
            mac.gpus[0].bandwidth_gb_s,
            Some(APPLE_FALLBACK_BANDWIDTH_GB_S)
        );
        assert_eq!(
            serde_json::to_value(&mac.gpus).unwrap(),
            serde_json::to_value(&simulate_mac("Apple M99", 24.0).gpus).unwrap()
        );
    }

    #[tokio::test]
    async fn unknown_brand_requires_arm_host_evidence_including_rosetta() {
        for (native, arm64, translated) in [
            (true, Err("unused"), Err("unused")),
            (false, Ok("1\n"), Err("unused")),
            (false, Ok("0"), Ok("1\n")),
            (false, Err("missing"), Ok("1")),
        ] {
            let mac = detected_mac(
                Err("missing"),
                "Unknown CPU",
                24.0,
                native,
                arm64,
                translated,
            )
            .await
            .unwrap();
            assert_eq!(mac.gpus[0].name, "Apple Silicon GPU");
            assert_eq!(
                mac.gpus[0].bandwidth_gb_s,
                Some(APPLE_FALLBACK_BANDWIDTH_GB_S)
            );
        }
        for brand in [
            "Intel(R) Core(TM) i7",
            "Mac16,10",
            "MacBookPro18,3",
            "",
            "Tesla M4",
        ] {
            assert!(
                detected_mac(Ok(brand), brand, 24.0, false, Ok("0"), Err("missing"))
                    .await
                    .is_err(),
                "{brand} is not ARM host evidence"
            );
        }
        assert!(detected_mac(
            Err("missing"),
            "",
            24.0,
            false,
            Err("missing"),
            Err("missing")
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn detection_rejects_invalid_unified_memory() {
        for gb in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(
                detected_mac(Ok("Apple M4"), "", gb, true, Err("unused"), Err("unused"))
                    .await
                    .is_err()
            );
        }
    }
}
