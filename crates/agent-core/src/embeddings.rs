//! Embedding trait and implementations for semantic memory.
//!
//! Provides:
//! - `Embedder` trait — generate vector representations of text
//! - `NullEmbedder` — no-op fallback (always available)
//! - `OnnxEmbedder` — behind `semantic-embeddings` feature flag
//! - Cosine similarity utility

use std::io;

/// Trait for generating embeddings from text.
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    /// Generate embeddings for a batch of texts.
    /// Returns a vector of embedding vectors (one per input text).
    /// Each embedding is a Vec<f32> of `dim` dimensions.
    async fn embed(&self, texts: &[String]) -> io::Result<Vec<Vec<f32>>>;

    /// Dimension of the embeddings produced by this embedder.
    fn dim(&self) -> usize;

    /// Whether this embedder is operational (e.g. model loaded, provider available).
    fn is_ready(&self) -> bool;

    /// Human-readable name of the embedder.
    fn name(&self) -> &str;
}

// ── NullEmbedder (no-op fallback) ───────────────────────────────────────────

/// An embedder that always returns an error. Used when no real embedder is
/// available, ensuring the system degrades gracefully to FTS-only search.
pub struct NullEmbedder;

#[async_trait::async_trait]
impl Embedder for NullEmbedder {
    async fn embed(&self, _texts: &[String]) -> io::Result<Vec<Vec<f32>>> {
        Err(io::Error::other("no embedder configured"))
    }

    fn dim(&self) -> usize {
        0
    }

    fn is_ready(&self) -> bool {
        false
    }

    fn name(&self) -> &str {
        "null"
    }
}

// ── Cosine similarity ──────────────────────────────────────────────────────

/// Compute cosine similarity between two slices of f32.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;

    for i in 0..a.len() {
        let va = a[i] as f64;
        let vb = b[i] as f64;
        dot += va * vb;
        norm_a += va * va;
        norm_b += vb * vb;
    }

    let denom = (norm_a * norm_b).sqrt();
    if denom < 1e-12 {
        return 0.0;
    }

    (dot / denom) as f32
}

// ── Serialization helpers ───────────────────────────────────────────────────

/// Serialize a Vec<f32> to a BLOB (little-endian f32 bytes).
pub fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for value in embedding {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Deserialize a BLOB (little-endian f32 bytes) back to Vec<f32>.
pub fn deserialize_embedding(bytes: &[u8]) -> Vec<f32> {
    let len = bytes.len() / 4;
    let mut embedding = Vec::with_capacity(len);
    for chunk in bytes.chunks_exact(4) {
        let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        embedding.push(value);
    }
    embedding
}

// ── OnnxEmbedder (feature-gated) ────────────────────────────────────────────

/// ONNX-based embedder using a lightweight model (bge-small-en or
/// multilingual-e5-small). Only compiled when the `semantic-embeddings`
/// feature is enabled.
#[cfg(feature = "semantic-embeddings")]
pub struct OnnxEmbedder {
    model_path: std::path::PathBuf,
    dim: usize,
    ready: bool,
}

#[cfg(feature = "semantic-embeddings")]
impl OnnxEmbedder {
    /// Create a new ONNX embedder from a model file path.
    /// The model is loaded lazily on first use.
    pub fn new(model_path: std::path::PathBuf, dim: usize) -> Self {
        let ready = model_path.exists();
        Self {
            model_path,
            dim,
            ready,
        }
    }
}

#[cfg(feature = "semantic-embeddings")]
#[async_trait::async_trait]
impl Embedder for OnnxEmbedder {
    async fn embed(&self, _texts: &[String]) -> io::Result<Vec<Vec<f32>>> {
        if !self.ready {
            return Err(io::Error::other(format!(
                "ONNX model not found at {}",
                self.model_path.display()
            )));
        }

        // The ONNX Runtime embedder would tokenize and run inference here.
        // For now, return a stub that the user can fill in after installing
        // the ONNX Runtime and model.
        //
        // In a real implementation, you would:
        // 1. Load the model via `ort::Session::new`
        // 2. Tokenize text using the model's tokenizer (e.g. tokenizers crate)
        // 3. Run inference and extract the pooled embedding
        //
        // Example stub:
        Err(io::Error::other(
            "ONNX embedder requires ort runtime + tokenizer setup. \
             Use ProviderEmbedder (Ollama) for embeddings instead.",
        ))
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn is_ready(&self) -> bool {
        self.ready
    }

    fn name(&self) -> &str {
        "onnx"
    }
}
