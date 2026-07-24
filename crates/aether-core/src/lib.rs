use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelBackend {
    OllamaMlx { endpoint: String, model: String },
    LlamaCpp { model_path: String },
    ByokCloud { provider: String, api_key: String, model: String },
}

pub struct ModelRouter {
    primary: ModelBackend,
    fallback: Option<ModelBackend>,
}

impl ModelRouter {
    pub fn new(primary: ModelBackend, fallback: Option<ModelBackend>) -> Self {
        Self { primary, fallback }
    }

    pub fn primary(&self) -> &ModelBackend {
        &self.primary
    }
}

#[derive(Error, Debug)]
pub enum EmbedderError {
    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Ollama API error: {0}")]
    Api(String),
    #[error("Dimension mismatch: expected 384, got {0}")]
    DimensionMismatch(usize),
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    prompt: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    embedding: Vec<f32>,
}

/// Fetches a real 384-dimensional embedding from Ollama /api/embeddings using all-MiniLM-L6-v2.
/// Fails explicitly with an error if Ollama is down or unreachable (no silent fallback).
pub async fn fetch_ollama_embedding(endpoint: &str, model: &str, text: &str) -> Result<Vec<f32>, EmbedderError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    
    let url = format!("{}/api/embeddings", endpoint.trim_end_matches('/'));
    
    let req = EmbeddingRequest {
        model: model.to_string(),
        prompt: text.to_string(),
    };

    let resp = client.post(&url)
        .json(&req)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(EmbedderError::Api(format!("HTTP status {}: {}", status, body)));
    }

    let data: EmbeddingResponse = resp.json().await?;
    if data.embedding.len() != 384 {
        return Err(EmbedderError::DimensionMismatch(data.embedding.len()));
    }

    Ok(data.embedding)
}
