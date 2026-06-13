//! Cloud model catalog and unified model listing.
//!
//! Provides a curated catalog of cloud models per provider, dynamic discovery
//! (optional), and the unified `/models/all` endpoint that groups models by
//! provider — Local (Ollama/MLX/llama.cpp) and Cloud (DeepSeek, OpenAI, etc.).

use crate::config::AgentUiConfig;
use crate::secrets_vault::SecretsVault;
use http_llm_provider::HttpApiKind;
use mlx_ollama_core::ModelDescriptor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, warn};

// ── Cloud model catalog ────────────────────────────────────────────────────

/// A curated entry for a cloud model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudModelEntry {
    pub id: String,         // e.g. "deepseek-v4-pro"
    pub label: String,      // e.g. "DeepSeek V4 Pro"
    pub family: String,     // e.g. "deepseek"
    pub context: usize,     // max context window
    pub flags: Vec<String>, // e.g. ["reasoning", "tool_use"]
}

/// A provider group in the unified model list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelGroup {
    pub provider: String, // e.g. "local", "deepseek", "openai"
    pub kind: String,     // "local" | "cloud"
    pub label: String,    // e.g. "Local", "DeepSeek"
    pub requires_api_key: bool,
    pub configured: bool, // has API key in vault
    pub status: String,   // "active" | "inactive" | "degraded"
    pub models: Vec<ModelGroupEntry>,
}

/// A single model in a group (simplified for the UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelGroupEntry {
    pub id: String,    // qualified id: "deepseek:deepseek-v4-pro" or "ollama::qwen3.5:9b"
    pub label: String, // display name
    pub provider: String, // group provider key
    pub badge: String, // "local" | "cloud"
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
                context: 1_000_000,
                flags: vec!["reasoning".to_string(), "tool_use".to_string()],
            },
            CloudModelEntry {
                id: "deepseek-v4-flash".to_string(),
                label: "DeepSeek V4 Flash".to_string(),
                family: "deepseek".to_string(),
                context: 1_000_000,
                flags: vec!["fast".to_string(), "tool_use".to_string()],
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
    pub vault_key: String, // e.g. "deepseek.api_key"
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
    agent_config: &AgentUiConfig,
    environment_values: &BTreeMap<String, String>,
    airgap: bool,
) -> Vec<ModelGroup> {
    let mut groups = Vec::new();
    let catalog = curated_cloud_models();

    // ── Local group ───────────────────────────────────────────────────
    let local_entries: Vec<ModelGroupEntry> = local_models
        .into_iter()
        .filter(|m| m.is_available)
        .map(|m| {
            let id = if m.id.contains("::") {
                m.id
            } else if m.provider == "ollama" {
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
        let Some(api_key) = resolve_api_key(vault, config, agent_config, environment_values) else {
            continue; // Don't show providers without keys.
        };

        let (models, status) = if let Some(curated) = catalog.get(&config.provider_key) {
            // Try dynamic discovery first, fall back to curated.
            match try_dynamic_discovery(config, &api_key).await {
                Ok(discovered) if !discovered.is_empty() => {
                    let discovered = discovered
                        .into_iter()
                        .filter(|model| {
                            config.provider_key != "deepseek"
                                || curated.iter().any(|item| item.id == model.id)
                        })
                        .collect::<Vec<_>>();
                    if discovered.is_empty() {
                        let entries = curated
                            .iter()
                            .map(|model| ModelGroupEntry {
                                id: format!("{}:{}", config.provider_key, model.id),
                                label: model.label.clone(),
                                provider: config.provider_key.clone(),
                                badge: "cloud".to_string(),
                                context: model.context,
                                flags: model.flags.clone(),
                            })
                            .collect();
                        (entries, "degraded".to_string())
                    } else {
                        let entries = discovered
                            .into_iter()
                            .map(|mut model| {
                                if let Some(metadata) =
                                    curated.iter().find(|item| item.id == model.id)
                                {
                                    model.label = metadata.label.clone();
                                    model.context = metadata.context;
                                    model.flags = metadata.flags.clone();
                                }
                                ModelGroupEntry {
                                    id: format!("{}:{}", config.provider_key, model.id),
                                    label: model.label,
                                    provider: config.provider_key.clone(),
                                    badge: "cloud".to_string(),
                                    context: model.context,
                                    flags: model.flags,
                                }
                            })
                            .collect();
                        (entries, "active".to_string())
                    }
                }
                Err(error) => {
                    warn!(
                        provider = %config.provider_key,
                        error = %error,
                        "cloud model discovery failed; using curated catalog"
                    );
                    // Use curated catalog.
                    let entries = curated
                        .iter()
                        .map(|m| ModelGroupEntry {
                            id: format!("{}:{}", config.provider_key, m.id),
                            label: m.label.clone(),
                            provider: config.provider_key.clone(),
                            badge: "cloud".to_string(),
                            context: m.context,
                            flags: m.flags.clone(),
                        })
                        .collect();
                    (entries, "degraded".to_string())
                }
                Ok(_) => {
                    let entries = curated
                        .iter()
                        .map(|m| ModelGroupEntry {
                            id: format!("{}:{}", config.provider_key, m.id),
                            label: m.label.clone(),
                            provider: config.provider_key.clone(),
                            badge: "cloud".to_string(),
                            context: m.context,
                            flags: m.flags.clone(),
                        })
                        .collect();
                    (entries, "degraded".to_string())
                }
            }
        } else {
            (Vec::new(), "degraded".to_string())
        };

        groups.push(ModelGroup {
            provider: config.provider_key.clone(),
            kind: "cloud".to_string(),
            label: config.label.clone(),
            requires_api_key: true,
            configured: true,
            status,
            models,
        });
    }

    groups
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

fn non_empty(value: impl Into<String>) -> Option<String> {
    let trimmed = value.into().trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn resolve_reference(
    reference: &str,
    vault: Option<&SecretsVault>,
    environment_values: &BTreeMap<String, String>,
) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    if let Some(key) = reference.strip_prefix("vault://") {
        return vault
            .and_then(|store| store.get_secret(key).ok().flatten())
            .and_then(non_empty);
    }
    environment_values
        .get(reference)
        .cloned()
        .and_then(non_empty)
        .or_else(|| std::env::var(reference).ok().and_then(non_empty))
}

/// Resolve a provider key from provider-specific vault entries, profiles,
/// the active Agent configuration, the managed environment file, or process env.
pub fn resolve_api_key(
    vault: Option<&SecretsVault>,
    config: &CloudProviderConfig,
    agent_config: &AgentUiConfig,
    environment_values: &BTreeMap<String, String>,
) -> Option<String> {
    if let Some(key) = get_api_key(vault, config) {
        return Some(key);
    }

    for profile in agent_config
        .provider_profiles
        .iter()
        .filter(|profile| profile.provider.eq_ignore_ascii_case(&config.provider_key))
    {
        if let Some(reference) = profile.api_key_ref.as_deref() {
            if let Some(key) = resolve_reference(reference, vault, environment_values) {
                return Some(key);
            }
        }
    }

    if let Some(key) = config
        .env_key
        .as_ref()
        .and_then(|key| environment_values.get(key))
        .cloned()
        .and_then(non_empty)
    {
        return Some(key);
    }

    if agent_config
        .provider
        .eq_ignore_ascii_case(&config.provider_key)
    {
        if let Some(key) = non_empty(agent_config.api_key.clone()) {
            return Some(key);
        }
        if let Some(reference) = agent_config.api_key_ref.as_deref() {
            if let Some(key) = resolve_reference(reference, vault, environment_values) {
                return Some(key);
            }
        }
        if let Some(key) = vault
            .and_then(|store| store.get_secret("agent.api_key").ok().flatten())
            .and_then(non_empty)
        {
            return Some(key);
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
    api_key: &str,
) -> Result<Vec<CloudModelEntry>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let url = if config.api_kind == HttpApiKind::Anthropic {
        format!(
            "{}/v1/models",
            config.default_base_url.trim_end_matches('/')
        )
    } else {
        format!("{}/models", config.default_base_url.trim_end_matches('/'))
    };

    let request = client.get(&url);
    let request = if config.api_kind == HttpApiKind::Anthropic {
        request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
    } else {
        request.header("Authorization", format!("Bearer {api_key}"))
    };
    let response = request.send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentProviderProfileConfig;
    use tempfile::tempdir;

    fn deepseek_config() -> CloudProviderConfig {
        find_cloud_config("deepseek").expect("deepseek config")
    }

    #[test]
    fn deepseek_catalog_contains_only_current_v4_models() {
        let catalog = curated_cloud_models();
        let ids = catalog
            .get("deepseek")
            .expect("deepseek models")
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["deepseek-v4-pro", "deepseek-v4-flash"]);
    }

    #[test]
    fn resolves_provider_specific_vault_key() {
        let dir = tempdir().expect("tempdir");
        let vault = SecretsVault::open(dir.path()).expect("vault");
        vault
            .set_secret("deepseek.api_key", "provider-secret")
            .expect("set secret");

        let key = resolve_api_key(
            Some(&vault),
            &deepseek_config(),
            &AgentUiConfig::default(),
            &BTreeMap::new(),
        );

        assert_eq!(key.as_deref(), Some("provider-secret"));
    }

    #[test]
    fn resolves_matching_profile_from_managed_environment() {
        let mut config = deepseek_config();
        config.env_key = None;
        let mut agent = AgentUiConfig::default();
        agent.provider = "ollama".to_string();
        agent.provider_profiles.push(AgentProviderProfileConfig {
            id: "deepseek-cloud".to_string(),
            provider: "deepseek".to_string(),
            model_id: "deepseek-v4-flash".to_string(),
            api_key_ref: Some("DEEPSEEK_API_KEY".to_string()),
            ..AgentProviderProfileConfig::default()
        });
        let environment =
            BTreeMap::from([("DEEPSEEK_API_KEY".to_string(), "env-secret".to_string())]);

        let key = resolve_api_key(None, &config, &agent, &environment);

        assert_eq!(key.as_deref(), Some("env-secret"));
    }

    #[test]
    fn resolves_legacy_agent_secret_for_active_provider() {
        let mut config = deepseek_config();
        config.env_key = None;
        let dir = tempdir().expect("tempdir");
        let vault = SecretsVault::open(dir.path()).expect("vault");
        vault
            .set_secret("agent.api_key", "legacy-secret")
            .expect("set secret");
        let mut agent = AgentUiConfig::default();
        agent.provider = "deepseek".to_string();

        let key = resolve_api_key(Some(&vault), &config, &agent, &BTreeMap::new());

        assert_eq!(key.as_deref(), Some("legacy-secret"));
    }
}
