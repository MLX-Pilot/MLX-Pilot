//! Cloud model catalog and unified model listing.
//!
//! Provides a curated catalog of cloud models per provider, dynamic discovery
//! (optional), and the unified `/models/all` endpoint that groups models by
//! provider — Local (Ollama/MLX/llama.cpp) and Cloud (DeepSeek, OpenAI, etc.).

use crate::secrets_vault::SecretsVault;
use http_llm_provider::HttpApiKind;
use mlx_ollama_core::ModelDescriptor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::debug;

// ── Cloud model catalog ────────────────────────────────────────────────────

/// A curated entry for a cloud model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudModelEntry {
    pub id: String,          // e.g. "deepseek-v4-pro"
    pub label: String,       // e.g. "DeepSeek V4 Pro"
    pub family: String,      // e.g. "deepseek"
    pub context: usize,      // max context window
    pub flags: Vec<String>,  // e.g. ["reasoning", "tool_use"]
}

/// A provider group in the unified model list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelGroup {
    pub provider: String,            // e.g. "local", "deepseek", "openai"
    pub kind: String,                // "local" | "cloud"
    pub label: String,               // e.g. "Local", "DeepSeek"
    pub requires_api_key: bool,
    pub configured: bool,            // has API key in vault
    pub status: String,              // "active" | "inactive" | "degraded"
    pub models: Vec<ModelGroupEntry>,
}

/// A single model in a group (simplified for the UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelGroupEntry {
    pub id: String,           // qualified id: "deepseek:deepseek-v4-pro" or "ollama::qwen3.5:9b"
    pub label: String,        // display name
    pub provider: String,     // group provider key
    pub badge: String,        // "local" | "cloud"
    pub context: usize,
    pub flags: Vec<String>,
}

/// Curated cloud model catalog. Add new providers and models here.
/// This is the fallback when dynamic discovery is unavailable.
fn curated_cloud_models() -> BTreeMap<String, Vec<CloudModelEntry>> {
    let mut catalog: BTreeMap<String, Vec<CloudModelEntry>> = BTreeMap::new();

    // ── DeepSeek ──────────────────────────────────────────────────────
    catalog.insert(
        "deepseek".to_string(),
        vec![
            CloudModelEntry {
                id: "deepseek-v4-pro".to_string(),
                label: "DeepSeek V4 Pro".to_string(),
                family: "deepseek".to_string(),
                context: 131_072,
                flags: vec!["reasoning".to_string(), "tool_use".to_string()],
            },
            CloudModelEntry {
                id: "deepseek-v4-flash".to_string(),
                label: "DeepSeek V4 Flash".to_string(),
                family: "deepseek".to_string(),
                context: 131_072,
                flags: vec!["fast".to_string()],
            },
            CloudModelEntry {
                id: "deepseek-chat".to_string(),
                label: "DeepSeek Chat (V3)".to_string(),
                family: "deepseek".to_string(),
                context: 65_536,
                flags: vec!["tool_use".to_string()],
            },
            CloudModelEntry {
                id: "deepseek-reasoner".to_string(),
                label: "DeepSeek Reasoner (R1)".to_string(),
                family: "deepseek".to_string(),
                context: 65_536,
                flags: vec!["reasoning".to_string()],
            },
        ],
    );

    // ── OpenAI ────────────────────────────────────────────────────────
    catalog.insert(
        "openai".to_string(),
        vec![
            CloudModelEntry {
                id: "gpt-5".to_string(),
                label: "GPT-5".to_string(),
                family: "openai".to_string(),
                context: 131_072,
                flags: vec!["tool_use".to_string(), "reasoning".to_string()],
            },
            CloudModelEntry {
                id: "gpt-5-mini".to_string(),
                label: "GPT-5 Mini".to_string(),
                family: "openai".to_string(),
                context: 131_072,
                flags: vec!["tool_use".to_string(), "fast".to_string()],
            },
            CloudModelEntry {
                id: "gpt-4.1".to_string(),
                label: "GPT-4.1".to_string(),
                family: "openai".to_string(),
                context: 131_072,
                flags: vec!["tool_use".to_string()],
            },
        ],
    );

    // ── Anthropic ─────────────────────────────────────────────────────
    catalog.insert(
        "anthropic".to_string(),
        vec![
            CloudModelEntry {
                id: "claude-opus-4-8".to_string(),
                label: "Claude Opus 4.8".to_string(),
                family: "anthropic".to_string(),
                context: 200_000,
                flags: vec!["tool_use".to_string(), "reasoning".to_string()],
            },
            CloudModelEntry {
                id: "claude-sonnet-4-6".to_string(),
                label: "Claude Sonnet 4.6".to_string(),
                family: "anthropic".to_string(),
                context: 200_000,
                flags: vec!["tool_use".to_string(), "fast".to_string()],
            },
            CloudModelEntry {
                id: "claude-haiku-4-5-20251001".to_string(),
                label: "Claude Haiku 4.5".to_string(),
                family: "anthropic".to_string(),
                context: 200_000,
                flags: vec!["fast".to_string()],
            },
        ],
    );

    // ── Groq ──────────────────────────────────────────────────────────
    catalog.insert(
        "groq".to_string(),
        vec![
            CloudModelEntry {
                id: "llama-4-maverick-17b".to_string(),
                label: "Llama 4 Maverick 17B".to_string(),
                family: "groq".to_string(),
                context: 131_072,
                flags: vec!["fast".to_string(), "tool_use".to_string()],
            },
            CloudModelEntry {
                id: "deepseek-r1-distill-qwen-32b".to_string(),
                label: "DeepSeek R1 Distill Qwen 32B".to_string(),
                family: "groq".to_string(),
                context: 131_072,
                flags: vec!["reasoning".to_string()],
            },
        ],
    );

    // ── OpenRouter ────────────────────────────────────────────────────
    catalog.insert(
        "openrouter".to_string(),
        vec![
            CloudModelEntry {
                id: "openai/gpt-5".to_string(),
                label: "GPT-5 (OpenRouter)".to_string(),
                family: "openrouter".to_string(),
                context: 131_072,
                flags: vec!["tool_use".to_string()],
            },
            CloudModelEntry {
                id: "anthropic/claude-opus-4-8".to_string(),
                label: "Claude Opus 4.8 (OpenRouter)".to_string(),
                family: "openrouter".to_string(),
                context: 200_000,
                flags: vec!["tool_use".to_string(), "reasoning".to_string()],
            },
        ],
    );

    catalog
}

/// Conventions for each cloud provider: vault key, base URL, HTTP API kind.
#[derive(Debug, Clone)]
pub struct CloudProviderConfig {
    pub provider_key: String,
    pub label: String,
    pub vault_key: String,       // e.g. "deepseek.api_key"
    pub default_base_url: String,
    pub api_kind: HttpApiKind,
    pub env_key: Option<String>, // fallback env var
}

/// Known cloud providers.
pub fn cloud_provider_configs() -> Vec<CloudProviderConfig> {
    vec![
        CloudProviderConfig {
            provider_key: "deepseek".to_string(),
            label: "DeepSeek".to_string(),
            vault_key: "deepseek.api_key".to_string(),
            default_base_url: "https://api.deepseek.com".to_string(),
            api_kind: HttpApiKind::OpenAiCompatible,
            env_key: Some("DEEPSEEK_API_KEY".to_string()),
        },
        CloudProviderConfig {
            provider_key: "openai".to_string(),
            label: "OpenAI".to_string(),
            vault_key: "openai.api_key".to_string(),
            default_base_url: "https://api.openai.com/v1".to_string(),
            api_kind: HttpApiKind::OpenAiCompatible,
            env_key: Some("OPENAI_API_KEY".to_string()),
        },
        CloudProviderConfig {
            provider_key: "anthropic".to_string(),
            label: "Anthropic".to_string(),
            vault_key: "anthropic.api_key".to_string(),
            default_base_url: "https://api.anthropic.com".to_string(),
            api_kind: HttpApiKind::Anthropic,
            env_key: Some("ANTHROPIC_API_KEY".to_string()),
        },
        CloudProviderConfig {
            provider_key: "groq".to_string(),
            label: "Groq".to_string(),
            vault_key: "groq.api_key".to_string(),
            default_base_url: "https://api.groq.com/openai/v1".to_string(),
            api_kind: HttpApiKind::OpenAiCompatible,
            env_key: Some("GROQ_API_KEY".to_string()),
        },
        CloudProviderConfig {
            provider_key: "openrouter".to_string(),
            label: "OpenRouter".to_string(),
            vault_key: "openrouter.api_key".to_string(),
            default_base_url: "https://openrouter.ai/api/v1".to_string(),
            api_kind: HttpApiKind::OpenAiCompatible,
            env_key: Some("OPENROUTER_API_KEY".to_string()),
        },
    ]
}

// ── Unified model listing ──────────────────────────────────────────────────

/// Build the unified model list: Local models + configured Cloud models.
pub async fn build_unified_models(
    local_models: Vec<ModelDescriptor>,
    vault: Option<&SecretsVault>,
    airgap: bool,
) -> Vec<ModelGroup> {
    let mut groups = Vec::new();
    let catalog = curated_cloud_models();

    // ── Local group ───────────────────────────────────────────────────
    let local_entries: Vec<ModelGroupEntry> = local_models
        .into_iter()
        .map(|m| {
            let id = if m.provider == "ollama" {
                format!("ollama::{}", m.id)
            } else if m.provider == "llamacpp" {
                format!("llama::{}", m.id)
            } else {
                format!("mlx::{}", m.id)
            };
            ModelGroupEntry {
                id,
                label: m.name,
                provider: "local".to_string(),
                badge: "local".to_string(),
                context: 0,
                flags: if m.agent_recommended {
                    vec!["recommended".to_string()]
                } else {
                    vec![]
                },
            }
        })
        .collect();

    groups.push(ModelGroup {
        provider: "local".to_string(),
        kind: "local".to_string(),
        label: "Local".to_string(),
        requires_api_key: false,
        configured: true,
        status: "active".to_string(),
        models: local_entries,
    });

    // ── Cloud groups ──────────────────────────────────────────────────
    if airgap {
        return groups; // No cloud models in airgap mode.
    }

    let configs = cloud_provider_configs();

    for config in &configs {
        let has_key = has_api_key(vault, config);
        if !has_key {
            continue; // Don't show providers without keys.
        }

        let models = if let Some(curated) = catalog.get(&config.provider_key) {
            // Try dynamic discovery first, fall back to curated.
            match try_dynamic_discovery(config).await {
                Ok(discovered) if !discovered.is_empty() => {
                    discovered
                        .into_iter()
                        .map(|m| ModelGroupEntry {
                            id: format!("{}:{}", config.provider_key, m.id),
                            label: m.label,
                            provider: config.provider_key.clone(),
                            badge: "cloud".to_string(),
                            context: m.context,
                            flags: m.flags,
                        })
                        .collect()
                }
                _ => {
                    // Use curated catalog.
                    curated
                        .iter()
                        .map(|m| ModelGroupEntry {
                            id: format!("{}:{}", config.provider_key, m.id),
                            label: m.label.clone(),
                            provider: config.provider_key.clone(),
                            badge: "cloud".to_string(),
                            context: m.context,
                            flags: m.flags.clone(),
                        })
                        .collect()
                }
            }
        } else {
            Vec::new()
        };

        groups.push(ModelGroup {
            provider: config.provider_key.clone(),
            kind: "cloud".to_string(),
            label: config.label.clone(),
            requires_api_key: true,
            configured: true,
            status: "active".to_string(),
            models,
        });
    }

    groups
}

/// Check if the vault has an API key for the given provider config.
fn has_api_key(vault: Option<&SecretsVault>, config: &CloudProviderConfig) -> bool {
    // Check vault first.
    if let Some(v) = vault {
        if let Ok(Some(key)) = v.get_secret(&config.vault_key) {
            if !key.trim().is_empty() {
                return true;
            }
        }
    }
    // Fallback: check environment variable.
    if let Some(env_key) = &config.env_key {
        if let Ok(value) = std::env::var(env_key) {
            if !value.trim().is_empty() {
                return true;
            }
        }
    }
    false
}

/// Get the API key for a provider (vault first, then env).
pub fn get_api_key(vault: Option<&SecretsVault>, config: &CloudProviderConfig) -> Option<String> {
    if let Some(v) = vault {
        if let Ok(Some(key)) = v.get_secret(&config.vault_key) {
            let trimmed = key.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    if let Some(env_key) = &config.env_key {
        if let Ok(value) = std::env::var(env_key) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

/// Find a cloud provider config by provider key.
pub fn find_cloud_config(provider_key: &str) -> Option<CloudProviderConfig> {
    cloud_provider_configs()
        .into_iter()
        .find(|c| c.provider_key == provider_key)
}

/// Try dynamic model discovery via `GET {base_url}/models`.
async fn try_dynamic_discovery(
    config: &CloudProviderConfig,
) -> Result<Vec<CloudModelEntry>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let api_key = get_api_key(None, config).ok_or("no API key")?;

    let url = if config.api_kind == HttpApiKind::Anthropic {
        format!("{}/v1/models", config.default_base_url.trim_end_matches('/'))
    } else {
        format!(
            "{}/models",
            config.default_base_url.trim_end_matches('/')
        )
    };

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| e.to_string())?;

    // OpenAI-compatible format: { "data": [{ "id": "model-name", ... }] }
    let models: Vec<CloudModelEntry> = json
        .get("data")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let id = entry.get("id")?.as_str()?.to_string();
                    Some(CloudModelEntry {
                        id: id.clone(),
                        label: id,
                        family: config.provider_key.clone(),
                        context: 0,
                        flags: vec![],
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    debug!(
        "dynamic discovery: {} models from {}",
        models.len(),
        config.provider_key
    );
    Ok(models)
}
