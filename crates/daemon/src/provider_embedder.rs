//! Embedder implementation that uses the local LLM provider's embeddings endpoint.
//!
//! Currently supports Ollama's `/api/embeddings` endpoint. Falls back gracefully
//! to NullEmbedder behavior when the provider is unavailable.

use mlx_agent_core::embeddings::Embedder;
use mlx_ollama_core::{ModelDescriptor, ModelProvider};
use ollama_provider::OllamaProvider;
use reqwest::Client;
use serde::Deserialize;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// An embedder that calls the Ollama `/api/embeddings` endpoint.
pub struct ProviderEmbedder {
    client: Client,
    base_url: String,
    dim: usize,
    model: String,
    ready: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    embedding: Vec<f32>,
}

impl ProviderEmbedder {
    /// Create a new ProviderEmbedder that uses the given Ollama provider.
    /// Probes the embeddings endpoint to detect the model and dimension.
    pub async fn new(ollama_base_url: &str, ollama_provider: &Option<Arc<OllamaProvider>>) -> Self {
        let base_url = ollama_base_url.trim_end_matches('/').to_string();
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Failed to build HTTP client");

        let mut embedder = Self {
            client,
            base_url,
            dim: 0,
            model: String::new(),
            ready: false,
        };

        // Auto-detect an embedding-capable model.
        if let Some(provider) = ollama_provider {
            match provider.list_models().await {
                Ok(models) => {
                    let models: &Vec<ModelDescriptor> = &models;
                    // Pick the first available model that supports embeddings.
                    let embedding_candidates = [
                        "nomic-embed-text",
                        "mxbai-embed-large",
                        "bge-m3",
                        "all-minilm",
                        "snowflake-arctic-embed",
                    ];

                    let mut chosen: Option<String> = None;
                    for candidate in &embedding_candidates {
                        if models
                            .iter()
                            .any(|m| m.id.contains(candidate) || m.path.contains(candidate))
                        {
                            chosen = models
                                .iter()
                                .find(|m| m.id.contains(candidate) || m.path.contains(candidate))
                                .map(|m| m.id.clone());
                            break;
                        }
                    }

                    // If no embedding-specific model, try the first available model
                    if chosen.is_none() {
                        chosen = models.first().map(|m| m.id.clone());
                    }

                    if let Some(model_id) = chosen {
                        embedder.model = model_id;
                        // Probe the model to get embedding dimension.
                        match embedder.probe_dimension().await {
                            Ok(dim) => {
                                embedder.dim = dim;
                                embedder.ready = true;
                                debug!(
                                    "ProviderEmbedder ready: model={}, dim={dim}",
                                    embedder.model
                                );
                            }
                            Err(e) => {
                                warn!("ProviderEmbedder probe failed: {e}");
                            }
                        }
                    } else {
                        debug!("ProviderEmbedder: no models available for embeddings");
                    }
                }
                Err(e) => {
                    warn!("ProviderEmbedder: failed to list models: {e}");
                }
            }
        }

        embedder
    }

    /// Probe the embeddings endpoint to determine the embedding dimension.
    async fn probe_dimension(&self) -> io::Result<usize> {
        let response = self
            .client
            .post(format!("{}/api/embeddings", self.base_url))
            .json(&serde_json::json!({
                "model": self.model,
                "prompt": "test"
            }))
            .send()
            .await
            .map_err(|e| io::Error::other(format!("Ollama embed request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(io::Error::other(format!(
                "Ollama embed returned HTTP {status}: {body}"
            )));
        }

        let result: OllamaEmbedResponse = response
            .json()
            .await
            .map_err(|e| io::Error::other(format!("Ollama embed parse error: {e}")))?;

        Ok(result.embedding.len())
    }
}

#[async_trait::async_trait]
impl Embedder for ProviderEmbedder {
    async fn embed(&self, texts: &[String]) -> io::Result<Vec<Vec<f32>>> {
        if !self.ready {
            return Err(io::Error::other(
                "ProviderEmbedder not ready: no embedding-capable model found",
            ));
        }

        let mut results = Vec::with_capacity(texts.len());

        for text in texts {
            let response = self
                .client
                .post(format!("{}/api/embeddings", self.base_url))
                .json(&serde_json::json!({
                    "model": self.model,
                    "prompt": text
                }))
                .send()
                .await
                .map_err(|e| io::Error::other(format!("Ollama embed request failed: {e}")))?;

            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(io::Error::other(format!(
                    "Ollama embed returned HTTP {status}: {body}"
                )));
            }

            let result: OllamaEmbedResponse = response
                .json()
                .await
                .map_err(|e| io::Error::other(format!("Ollama embed parse error: {e}")))?;

            results.push(result.embedding);
        }

        Ok(results)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn name(&self) -> &str {
        "ollama"
    }
}
