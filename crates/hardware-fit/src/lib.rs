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

    let cpu_name = get_cpu_name();
    let cpu_cores = get_cpu_count();
    let ram_gb = get_ram_gb();
    let available_ram_gb = get_available_ram_gb();
    let mut gpus: Vec<GpuInfo> = Vec::new();

    // Try platform-specific GPU detection
    if platform == "macos" {
        match detect_apple_silicon().await {
            Ok(mut mac_gpus) => {
                gpus.append(&mut mac_gpus);
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

async fn detect_apple_silicon() -> Result<Vec<GpuInfo>, String> {
    if std::env::consts::OS != "macos" {
        return Err("Not macOS".to_string());
    }

    // Check if Apple Silicon via sysctl
    let model = run_cmd("sysctl", &["-n", "hw.model"])
        .await
        .unwrap_or_default();
    let is_apple_silicon = model.contains("Mac") && !model.contains("Intel");

    if !is_apple_silicon {
        return Err("Not Apple Silicon".to_string());
    }

    // Get unified memory
    let mem_bytes = run_cmd("sysctl", &["-n", "hw.memsize"])
        .await
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let ram_gb = mem_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

    // Get GPU core count for name
    let gpu_cores = run_cmd("sysctl", &["-n", "hw.perflevel1.gpu_count"])
        .await
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());

    let gpu_name = match gpu_cores {
        Some(cores) => format!("Apple Silicon GPU ({} cores)", cores),
        None => "Apple Silicon GPU".to_string(),
    };

    Ok(vec![GpuInfo {
        name: gpu_name,
        vram_gb: ram_gb * 0.75, // Unified memory: ~75% available for GPU
        index: 0,
        backend: "metal".to_string(),
        compute_capability: None,
        bandwidth_gb_s: Some(estimate_apple_bandwidth(&model)),
    }])
}

fn estimate_apple_bandwidth(model: &str) -> f64 {
    // Rough estimates based on known models
    if model.contains("M5") {
        600.0
    } else if model.contains("M4") {
        if model.contains("Max") {
            546.0
        } else if model.contains("Pro") {
            273.0
        } else {
            120.0
        }
    } else if model.contains("M3") {
        if model.contains("Max") {
            400.0
        } else if model.contains("Pro") {
            150.0
        } else {
            100.0
        }
    } else if model.contains("M2") {
        if model.contains("Ultra") {
            800.0
        } else if model.contains("Max") {
            400.0
        } else if model.contains("Pro") {
            200.0
        } else {
            100.0
        }
    } else if model.contains("M1") {
        if model.contains("Ultra") {
            800.0
        } else if model.contains("Max") {
            400.0
        } else if model.contains("Pro") {
            200.0
        } else {
            68.0
        }
    } else {
        150.0 // conservative fallback
    }
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
    manual_gpu_count: Option<usize>,
    manual_vram_gb: Option<f64>,
    manual_ram_gb: Option<f64>,
    manual_backend: Option<String>,
    ignore_detected_gpu: bool,
    ignore_detected_ram: bool,
) -> HardwareProfile {
    let mut profile = base.clone();

    if ignore_detected_gpu {
        profile.gpus.clear();
        profile.gpu_count = 0;
        profile.total_vram_gb = 0.0;
    }

    if ignore_detected_ram {
        profile.ram_gb = 0.0;
        profile.available_ram_gb = 0.0;
    }

    if let Some(ram) = manual_ram_gb {
        profile.ram_gb = ram;
        profile.available_ram_gb = ram * 0.9;
    }

    if let Some(backend) = manual_backend {
        profile.primary_backend = backend.clone();
        if let Some(count) = manual_gpu_count {
            if let Some(vram) = manual_vram_gb {
                profile.gpus.clear();
                for i in 0..count {
                    profile.gpus.push(GpuInfo {
                        name: format!("Manual GPU {} ({})", i + 1, backend),
                        vram_gb: vram,
                        index: i as u32,
                        backend: backend.clone(),
                        compute_capability: None,
                        bandwidth_gb_s: Some(match backend.as_str() {
                            "cuda" => 400.0,
                            "rocm" => 300.0,
                            "metal" => 200.0,
                            _ => 100.0,
                        }),
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
            Some(1),
            Some(24.0),
            Some(64.0),
            Some("cuda".into()),
            false,
            false,
        );

        assert_eq!(simulated.gpu_count, 1);
        assert_eq!(simulated.total_vram_gb, 24.0);
        assert_eq!(simulated.ram_gb, 64.0);
        assert_eq!(simulated.primary_backend, "cuda");
        assert!(!simulated.is_cpu_only);
    }
}
