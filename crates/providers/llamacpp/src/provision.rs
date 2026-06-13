//! Automatic acquisition of a `llama-server` binary that matches the host hardware.
//!
//! The desktop app ships a CPU baseline next to the executable so inference works
//! offline out of the box. When an accelerated backend is available (e.g. a GPU with
//! the Vulkan loader installed) the matching prebuilt is downloaded once into a local
//! cache and reused on every subsequent run.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mlx_ollama_core::ProviderError;
use tracing::{info, warn};

/// Pinned upstream release used when no override is provided.
const DEFAULT_RELEASE: &str = "b9601";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineVariant {
    Cpu,
    Vulkan,
    Cuda,
}

impl EngineVariant {
    pub fn slug(self) -> &'static str {
        match self {
            EngineVariant::Cpu => "cpu",
            EngineVariant::Vulkan => "vulkan",
            EngineVariant::Cuda => "cuda",
        }
    }
}

fn release_tag() -> String {
    std::env::var("APP_LLAMACPP_RELEASE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_RELEASE.to_string())
}

fn server_file_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// Pick the best engine variant for this machine.
///
/// `APP_LLAMACPP_VARIANT=cpu|vulkan|cuda|auto` forces a specific choice; otherwise we
/// auto-detect: a usable GPU (Vulkan loader present) selects the Vulkan build, and we
/// fall back to the universally-compatible CPU build.
pub fn detect_variant() -> EngineVariant {
    if let Ok(forced) = std::env::var("APP_LLAMACPP_VARIANT") {
        match forced.trim().to_ascii_lowercase().as_str() {
            "cpu" => return EngineVariant::Cpu,
            "vulkan" | "gpu" => return EngineVariant::Vulkan,
            "cuda" => return EngineVariant::Cuda,
            "" | "auto" => {}
            other => {
                warn!("APP_LLAMACPP_VARIANT desconhecido '{other}', usando deteccao automatica")
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if windows_has_vulkan_gpu() {
            return EngineVariant::Vulkan;
        }
        EngineVariant::Cpu
    }

    // MLX/Metal on Apple Silicon is handled by the dedicated provider and the daemon's
    // `auto` routing; for the cross-platform llama.cpp engine we default to CPU elsewhere.
    #[cfg(not(target_os = "windows"))]
    {
        EngineVariant::Cpu
    }
}

#[cfg(target_os = "windows")]
fn windows_has_vulkan_gpu() -> bool {
    // The Vulkan loader (`vulkan-1.dll`) is installed by GPU drivers, so its presence is a
    // reliable, dependency-free signal that an accelerated backend is usable on this host.
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    Path::new(&system_root)
        .join("System32")
        .join("vulkan-1.dll")
        .exists()
}

/// Per-user cache directory where provisioned engines are stored.
pub fn engine_cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            if !local.trim().is_empty() {
                return PathBuf::from(local).join("MLX Pilot").join("engine");
            }
        }
    }

    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("mlx-pilot")
                .join("engine");
        }
    }

    std::env::temp_dir().join("mlx-pilot").join("engine")
}

/// Returns the cached server binary for a variant if it was already provisioned.
pub fn cached_binary(variant: EngineVariant) -> Option<PathBuf> {
    let path = engine_cache_dir()
        .join(variant.slug())
        .join(server_file_name());
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn download_url(variant: EngineVariant) -> Result<String, ProviderError> {
    let tag = release_tag();
    let asset = if cfg!(target_os = "windows") {
        match variant {
            EngineVariant::Cpu => format!("llama-{tag}-bin-win-cpu-x64.zip"),
            EngineVariant::Vulkan => format!("llama-{tag}-bin-win-vulkan-x64.zip"),
            EngineVariant::Cuda => format!("llama-{tag}-bin-win-cuda-12.4-x64.zip"),
        }
    } else {
        return Err(ProviderError::Unavailable {
            details: "auto-provisionamento do llama.cpp esta disponivel apenas no Windows x64; \
                 configure APP_LLAMACPP_SERVER_BINARY com um binario existente"
                .to_string(),
        });
    };

    Ok(format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{tag}/{asset}"
    ))
}

/// Download and extract the requested engine variant, returning the server binary path.
/// No-op (returns the cached path) when the variant was already provisioned.
pub async fn provision_variant(variant: EngineVariant) -> Result<PathBuf, ProviderError> {
    if let Some(existing) = cached_binary(variant) {
        return Ok(existing);
    }

    let url = download_url(variant)?;
    let dest = engine_cache_dir().join(variant.slug());
    info!(
        "provisioning llama.cpp engine variant '{}' from {}",
        variant.slug(),
        url
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(900))
        .build()
        .map_err(|source| ProviderError::Unavailable {
            details: format!("falha criando cliente HTTP para download do engine: {source}"),
        })?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|source| ProviderError::Unavailable {
            details: format!("falha baixando engine de {url}: {source}"),
        })?;

    if !response.status().is_success() {
        return Err(ProviderError::Unavailable {
            details: format!(
                "download do engine retornou HTTP {} ({url})",
                response.status()
            ),
        });
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|source| ProviderError::Unavailable {
            details: format!("falha lendo corpo do download do engine: {source}"),
        })?;

    let dest_for_blocking = dest.clone();
    let payload = bytes.to_vec();
    tokio::task::spawn_blocking(move || extract_engine_zip(&payload, &dest_for_blocking))
        .await
        .map_err(|source| ProviderError::Unavailable {
            details: format!("falha aguardando extracao do engine: {source}"),
        })??;

    cached_binary(variant).ok_or_else(|| ProviderError::Unavailable {
        details: format!(
            "extracao do engine '{}' concluiu sem o binario {}",
            variant.slug(),
            server_file_name()
        ),
    })
}

/// Extract the server launcher plus every shared library from a llama.cpp release zip.
fn extract_engine_zip(bytes: &[u8], dest: &Path) -> Result<(), ProviderError> {
    std::fs::create_dir_all(dest).map_err(|source| ProviderError::Io {
        context: format!("criando diretorio do engine {}", dest.display()),
        source,
    })?;

    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|source| ProviderError::Unavailable {
            details: format!("arquivo zip do engine invalido: {source}"),
        })?;

    let server_name = server_file_name();
    let mut extracted = 0usize;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|source| ProviderError::Unavailable {
                details: format!("falha lendo entrada {index} do zip: {source}"),
            })?;

        if entry.is_dir() {
            continue;
        }

        let normalized = entry.name().replace('\\', "/");
        let file_name = normalized
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if file_name.is_empty() {
            continue;
        }

        // Keep every shared library plus the server launcher; the other CLI exes are unused.
        let lower = file_name.to_ascii_lowercase();
        let keep = lower.ends_with(".dll") || lower.ends_with(".so") || file_name == server_name;
        if !keep {
            continue;
        }

        let out_path = dest.join(&file_name);
        let mut out_file =
            std::fs::File::create(&out_path).map_err(|source| ProviderError::Io {
                context: format!("criando {}", out_path.display()),
                source,
            })?;
        std::io::copy(&mut entry, &mut out_file).map_err(|source| ProviderError::Io {
            context: format!("escrevendo {}", out_path.display()),
            source,
        })?;
        extracted += 1;
    }

    if extracted == 0 {
        return Err(ProviderError::Unavailable {
            details: "zip do engine nao continha binarios utilizaveis".to_string(),
        });
    }

    Ok(())
}
