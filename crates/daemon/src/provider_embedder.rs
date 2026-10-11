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
    provider: Option<Arc<OllamaProvider>>,
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
            provider: ollama_provider.clone(),
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
        self.validate_server_version().await?;
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

    async fn validate_server_version(&self) -> io::Result<()> {
        let provider = self
            .provider
            .as_ref()
            .ok_or_else(|| io::Error::other("ProviderEmbedder has no Ollama provider"))?;
        provider
            .validate_server_version()
            .await
            .map_err(|e| io::Error::other(e.to_string()))
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
            self.validate_server_version().await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use ollama_provider::OllamaProviderConfig;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn ready_embedder_rejects_replacement_with_incompatible_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = [0u8; 4096];
            let count = stream.read(&mut bytes).await.unwrap();
            assert!(String::from_utf8_lossy(&bytes[..count]).starts_with("GET /api/version"));
            let body = r#"{"version":"0.16.2"}"#;
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let provider = Arc::new(OllamaProvider::new(OllamaProviderConfig {
            base_url: base_url.clone(),
            auto_install: false,
            auto_start: false,
            ..Default::default()
        }));
        let embedder = ProviderEmbedder {
            client: Client::new(),
            base_url,
            dim: 3,
            model: "nomic-embed-text".into(),
            ready: true,
            provider: Some(provider),
        };
        let error = embedder.embed(&["hello".into()]).await.unwrap_err();
        assert!(error.to_string().contains("0.16.2"));
        assert!(error.to_string().contains("0.30.7"));
        server.await.unwrap();
    }
}
