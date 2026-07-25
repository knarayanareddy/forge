mod graph_extract;
mod keychain;
mod loop_engine;
mod nl_planner;
mod orchestration_graph;
mod verifier_node;

pub use graph_extract::{
    build_graph_extract_prompt, enforce_max_entities, graph_extract_schema_json,
    payload_to_graph_inserts, run_graph_extract, strip_json_fence, validate_graph_extract,
    ExtractEdge, ExtractNode, GraphExtractError, GraphExtractPayload, PreparedGraphEdge,
    PreparedGraphNode, Provenance, GRAPH_EXTRACT_SCHEMA_PATH,
};

pub use keychain::{
    ensure_daemon_auth_token, load_byok_key, load_daemon_auth_token, load_daemon_auth_token_file,
    load_gateway_token, require_byok_key_if_configured, store_byok_key, store_daemon_auth_token,
    store_gateway_token, verify_daemon_auth_token, verify_daemon_auth_token_expected,
    KeychainError, BYOK_ACCOUNT, BYOK_SERVICE, DAEMON_AUTH_ACCOUNT,
};

pub use loop_engine::{
    GoalStopHook, LoopConfig, LoopRunResult, LoopStreamEvent, PythonLintVerifier, ReActLoopEngine,
    StopHook, ToolInvocation, ToolObservation, ToolRegistry, Verifier, DEFAULT_MAX_LOOP_TOKENS,
};

pub use nl_planner::{
    build_nl_plan_prompt, run_nl_planner, validate_nl_plan, NlPlanError, LOOP02_EVAL_PROMPT,
    LOOP02_GOLD_TOOL_ORDER,
};

pub use orchestration_graph::OrchestrationGraph;
pub use verifier_node::{MakerCheckerGoal, VerifierNode};

use aether_permissions::{PermissionDecision, PermissionManager};
use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
use std::pin::Pin;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelBackend {
    OllamaMlx { endpoint: String, model: String },
    LlamaCpp { model_path: String },
    ByokCloud { provider: String, api_key: String, model: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptComplexity {
    Simple,
    Complex,
}

pub struct ModelRouter {
    primary: ModelBackend,
    fallback: Option<ModelBackend>,
}

impl ModelRouter {
    pub fn new(primary: ModelBackend, fallback: Option<ModelBackend>) -> Self {
        Self { primary, fallback }
    }

    /// Build router from env. When `AETHER_BYOK_PROVIDER` is set, loads API key from macOS Keychain.
    pub fn from_env() -> Result<Self, KeychainError> {
        if let Some(api_key) = require_byok_key_if_configured()? {
            let provider = std::env::var("AETHER_BYOK_PROVIDER").unwrap_or_else(|_| "openai".into());
            let model = std::env::var("AETHER_BYOK_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
            return Ok(Self::new(
                ModelBackend::ByokCloud {
                    provider,
                    api_key,
                    model,
                },
                None,
            ));
        }

        let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());
        let complex_model =
            std::env::var("AETHER_CHAT_MODEL_COMPLEX").unwrap_or_else(|_| model.clone());

        Ok(Self::new(
            ModelBackend::OllamaMlx {
                endpoint: endpoint.clone(),
                model: model.clone(),
            },
            Some(ModelBackend::OllamaMlx {
                endpoint,
                model: complex_model,
            }),
        ))
    }

    pub fn primary(&self) -> &ModelBackend {
        &self.primary
    }

    pub fn route_backend(&self, complexity: PromptComplexity) -> &ModelBackend {
        match complexity {
            PromptComplexity::Simple => &self.primary,
            PromptComplexity::Complex => self.fallback.as_ref().unwrap_or(&self.primary),
        }
    }

    pub async fn complete(
        &self,
        prompt: &str,
        complexity: PromptComplexity,
    ) -> Result<CompletionResult, CompleteError> {
        let backend = self.route_backend(complexity);
        let mut content = String::new();
        let mut ttft_ms = 0u128;
        let mut model = String::new();

        let mut stream = Box::pin(OllamaProvider::complete_stream_backend(backend, prompt).await?);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if ttft_ms == 0 {
                if let Some(ms) = chunk.ttft_ms {
                    ttft_ms = ms;
                }
            }
            if model.is_empty() {
                model = chunk.model.clone();
            }
            content.push_str(&chunk.text);
            if chunk.done {
                break;
            }
        }

        if content.trim().is_empty() {
            return Err(CompleteError::Api("Empty completion from streaming Ollama".into()));
        }

        Ok(CompletionResult {
            content,
            ttft_ms,
            model,
        })
    }

    pub async fn complete_stream(
        &self,
        prompt: &str,
        complexity: PromptComplexity,
    ) -> Result<TokenStream, CompleteError> {
        let backend = self.route_backend(complexity);
        OllamaProvider::complete_stream_backend(backend, prompt).await
    }

    /// Non-streaming JSON-mode completion for structured extraction (Slice 6.4 graph_extract).
    pub async fn complete_json(
        &self,
        prompt: &str,
        num_predict: u32,
    ) -> Result<CompletionResult, CompleteError> {
        OllamaProvider::complete_json_backend(self.primary(), prompt, num_predict).await
    }
}

pub type TokenStream = Pin<Box<dyn Stream<Item = Result<TokenChunk, CompleteError>> + Send>>;

#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub content: String,
    pub ttft_ms: u128,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct TokenChunk {
    pub text: String,
    /// Client-side time-to-first-token in milliseconds; set only on the first content chunk.
    pub ttft_ms: Option<u128>,
    /// Ollama server-side TTFT (`load_duration + prompt_eval_duration`) on the terminal chunk.
    pub server_ttft_ms: Option<u128>,
    pub model: String,
    pub done: bool,
}

use std::sync::OnceLock;

pub(crate) fn ollama_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .pool_max_idle_per_host(4)
            .build()
            .expect("ollama reqwest client")
    })
}

/// Prompt used for ROUT-01 TTFT warmup and measurement (must match harness).
pub const ROUT_TTFT_PROMPT: &str = "forge";

/// Max Ollama `load_duration` (ns) to treat a sample as warm (model already resident).
/// Apple Silicon often reports 100–250ms load overhead even when `/api/ps` shows the model loaded.
pub const ROUT_WARM_LOAD_MAX_NS: u64 = 300_000_000;

pub struct OllamaProvider;

impl OllamaProvider {
    pub async fn health_check(endpoint: &str) -> Result<(), CompleteError> {
        let client = ollama_client();
        let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
        let resp = client.get(&url).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(CompleteError::Api(format!(
                "Ollama health check failed: HTTP {}",
                resp.status()
            )))
        }
    }

    /// Drain `rounds` streaming chat completions to load the model and keep it warm (`keep_alive: 30m`).
    pub async fn warm_chat_model(endpoint: &str, model: &str, rounds: usize) -> Result<(), CompleteError> {
        Self::warm_chat_model_with_prompt(endpoint, model, ROUT_TTFT_PROMPT, rounds).await
    }

    /// Warm with an explicit prompt (ROUT-01 uses `ROUT_TTFT_PROMPT` for parity with TTFT measurement).
    pub async fn warm_chat_model_with_prompt(
        endpoint: &str,
        model: &str,
        prompt: &str,
        rounds: usize,
    ) -> Result<(), CompleteError> {
        for _ in 0..rounds {
            let mut stream = Box::pin(Self::complete_stream(endpoint, model, prompt).await?);
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                if chunk.done {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Load the chat model into Ollama memory (`keep_alive: 30m`) before TTFT-sensitive work.
    pub async fn preload_chat_model(endpoint: &str, model: &str) -> Result<(), CompleteError> {
        Self::warm_chat_model(endpoint, model, 5).await
    }

    /// Returns true when `model` is resident in Ollama memory (`/api/ps`).
    pub async fn is_model_loaded(endpoint: &str, model: &str) -> Result<bool, CompleteError> {
        let client = ollama_client();
        let url = format!("{}/api/ps", endpoint.trim_end_matches('/'));
        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(CompleteError::Api(format!(
                "Ollama ps check failed: HTTP {}",
                resp.status()
            )));
        }
        let data: PsResponse = resp.json().await?;
        Ok(data.models.iter().any(|m| model_name_matches(&m.name, model)))
    }

    pub async fn complete(
        endpoint: &str,
        model: &str,
        prompt: &str,
    ) -> Result<CompletionResult, CompleteError> {
        let backend = ModelBackend::OllamaMlx {
            endpoint: endpoint.to_string(),
            model: model.to_string(),
        };
        let router = ModelRouter::new(backend, None);
        router.complete(prompt, PromptComplexity::Simple).await
    }

    pub async fn complete_stream(
        endpoint: &str,
        model: &str,
        prompt: &str,
    ) -> Result<TokenStream, CompleteError> {
        let backend = ModelBackend::OllamaMlx {
            endpoint: endpoint.to_string(),
            model: model.to_string(),
        };
        Self::complete_stream_backend(&backend, prompt).await
    }

    pub async fn complete_stream_backend(
        backend: &ModelBackend,
        prompt: &str,
    ) -> Result<TokenStream, CompleteError> {
        match backend {
            ModelBackend::OllamaMlx { endpoint, model } => {
                Self::complete_stream_ollama(endpoint, model, prompt).await
            }
            ModelBackend::ByokCloud {
                provider,
                api_key,
                model,
            } => Self::complete_stream_byok(provider, api_key, model, prompt).await,
            ModelBackend::LlamaCpp { .. } => Err(CompleteError::Api(
                "LlamaCpp backend not implemented in MVP router".into(),
            )),
        }
    }

    pub async fn complete_json_backend(
        backend: &ModelBackend,
        prompt: &str,
        num_predict: u32,
    ) -> Result<CompletionResult, CompleteError> {
        match backend {
            ModelBackend::OllamaMlx { endpoint, model } => {
                Self::complete_json_ollama(endpoint, model, prompt, num_predict).await
            }
            ModelBackend::ByokCloud {
                provider,
                api_key,
                model,
            } => Self::complete_json_byok(provider, api_key, model, prompt, num_predict).await,
            ModelBackend::LlamaCpp { .. } => Err(CompleteError::Api(
                "LlamaCpp backend not implemented in MVP router".into(),
            )),
        }
    }

    async fn complete_json_ollama(
        endpoint: &str,
        model: &str,
        prompt: &str,
        num_predict: u32,
    ) -> Result<CompletionResult, CompleteError> {
        let client = ollama_client();
        let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));
        let req = ChatRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            stream: false,
            keep_alive: Some("30m".to_string()),
            format: Some("json".to_string()),
            options: Some(ChatOptions {
                num_predict,
                temperature: 0.0,
                num_ctx: None,
            }),
        };

        let start = Instant::now();
        let resp = client.post(&url).json(&req).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CompleteError::Api(format!("HTTP status {}: {}", status, body)));
        }

        let data: ChatResponse = resp.json().await?;
        if data.message.content.trim().is_empty() {
            return Err(CompleteError::Api("Empty JSON completion from Ollama".into()));
        }

        Ok(CompletionResult {
            content: data.message.content,
            ttft_ms: start.elapsed().as_millis(),
            model: model.to_string(),
        })
    }

    async fn complete_json_byok(
        provider: &str,
        api_key: &str,
        model: &str,
        prompt: &str,
        num_predict: u32,
    ) -> Result<CompletionResult, CompleteError> {
        let base = std::env::var("AETHER_BYOK_ENDPOINT").unwrap_or_else(|_| {
            match provider {
                "openai" => "https://api.openai.com/v1".into(),
                other => format!("https://api.{}.com/v1", other),
            }
        });
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        let client = ollama_client();
        let req = OpenAiChatRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            stream: false,
            max_tokens: Some(num_predict),
            response_format: Some(OpenAiResponseFormat {
                format_type: "json_object".to_string(),
            }),
        };

        let start = Instant::now();
        let resp = client
            .post(&url)
            .bearer_auth(api_key)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CompleteError::Api(format!(
                "BYOK {} HTTP {}: {}",
                provider, status, body
            )));
        }

        let data: OpenAiChatResponse = resp.json().await?;
        let content = data
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        if content.trim().is_empty() {
            return Err(CompleteError::Api("Empty JSON completion from BYOK".into()));
        }

        Ok(CompletionResult {
            content,
            ttft_ms: start.elapsed().as_millis(),
            model: model.to_string(),
        })
    }

    async fn complete_stream_ollama(
        endpoint: &str,
        model: &str,
        prompt: &str,
    ) -> Result<TokenStream, CompleteError> {
        let client = ollama_client();
        let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));
        let req = ChatRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            stream: true,
            keep_alive: Some("30m".to_string()),
            format: None,
            options: Some(ChatOptions {
                num_predict: 16,
                temperature: 0.0,
                num_ctx: Some(512),
            }),
        };

        let start = Instant::now();
        let resp = client.post(&url).json(&req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CompleteError::Api(format!("HTTP status {}: {}", status, body)));
        }

        let model_name = model.to_string();
        let byte_stream = resp.bytes_stream();

        let stream = try_stream! {
            let mut buffer = String::new();
            let mut ttft_recorded = false;
            futures_util::pin_mut!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = chunk_result.map_err(CompleteError::Http)?;
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let data: StreamChatResponse = parse_stream_line(&line)?;
                    if data.done {
                        let server_ttft_ms =
                            ollama_warm_server_ttft_ms(data.load_duration, data.prompt_eval_duration);
                        yield TokenChunk {
                            text: String::new(),
                            ttft_ms: None,
                            server_ttft_ms,
                            model: model_name.clone(),
                            done: true,
                        };
                        return;
                    }
                    if !data.message.content.is_empty() {
                        let ttft_ms = if ttft_recorded {
                            None
                        } else {
                            ttft_recorded = true;
                            Some(start.elapsed().as_millis())
                        };
                        yield TokenChunk {
                            text: data.message.content.clone(),
                            ttft_ms,
                            server_ttft_ms: None,
                            model: model_name.clone(),
                            done: false,
                        };
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn complete_stream_byok(
        provider: &str,
        api_key: &str,
        model: &str,
        prompt: &str,
    ) -> Result<TokenStream, CompleteError> {
        let base = std::env::var("AETHER_BYOK_ENDPOINT").unwrap_or_else(|_| {
            match provider {
                "openai" => "https://api.openai.com/v1".into(),
                other => format!("https://api.{}.com/v1", other),
            }
        });
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        let client = ollama_client();
        let req = OpenAiChatRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            stream: true,
            max_tokens: None,
            response_format: None,
        };

        let start = Instant::now();
        let resp = client
            .post(&url)
            .bearer_auth(api_key)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CompleteError::Api(format!(
                "BYOK {} HTTP {}: {}",
                provider, status, body
            )));
        }

        let model_name = model.to_string();
        let byte_stream = resp.bytes_stream();
        let stream = try_stream! {
            let mut buffer = String::new();
            let mut ttft_recorded = false;
            futures_util::pin_mut!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = chunk_result.map_err(CompleteError::Http)?;
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();
                    if line.is_empty() || line == "data: [DONE]" {
                        continue;
                    }
                    let json = line.strip_prefix("data: ").unwrap_or(&line);
                    let data: OpenAiStreamChunk = serde_json::from_str(json)
                        .map_err(|e| CompleteError::Api(format!("BYOK SSE parse: {}", e)))?;

                    if let Some(choice) = data.choices.first() {
                        if !choice.delta.content.is_empty() {
                            let ttft_ms = if ttft_recorded {
                                None
                            } else {
                                ttft_recorded = true;
                                Some(start.elapsed().as_millis())
                            };
                            yield TokenChunk {
                                text: choice.delta.content.clone(),
                                ttft_ms,
                                server_ttft_ms: None,
                                model: model_name.clone(),
                                done: false,
                            };
                        }
                        if choice.finish_reason.as_deref() == Some("stop") {
                            yield TokenChunk {
                                text: String::new(),
                                ttft_ms: None,
                                server_ttft_ms: None,
                                model: model_name.clone(),
                                done: true,
                            };
                            return;
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

fn parse_stream_line(line: &str) -> Result<StreamChatResponse, CompleteError> {
    let json = line.strip_prefix("data: ").unwrap_or(line).trim();
    if json == "[DONE]" {
        return Ok(StreamChatResponse {
            message: StreamMessage {
                content: String::new(),
            },
            done: true,
            load_duration: None,
            prompt_eval_duration: None,
        });
    }
    serde_json::from_str(json).map_err(|e| CompleteError::Api(format!("SSE parse error: {}", e)))
}

#[derive(Error, Debug)]
pub enum CompleteError {
    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Ollama API error: {0}")]
    Api(String),
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

#[derive(Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<OpenAiResponseFormat>,
}

#[derive(Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChatChoice>,
}

#[derive(Deserialize)]
struct OpenAiChatChoice {
    message: OpenAiChatMessage,
}

#[derive(Deserialize)]
struct OpenAiChatMessage {
    content: String,
}

#[derive(Deserialize)]
struct OpenAiStreamChunk {
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiStreamDelta {
    #[serde(default)]
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<ChatOptions>,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: StreamMessage,
}

#[derive(Serialize)]
struct OpenAiResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

#[derive(Serialize)]
struct ChatOptions {
    num_predict: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct StreamChatResponse {
    message: StreamMessage,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    load_duration: Option<u64>,
    #[serde(default)]
    prompt_eval_duration: Option<u64>,
}

#[derive(Deserialize)]
struct PsResponse {
    models: Vec<PsModel>,
}

#[derive(Deserialize)]
struct PsModel {
    name: String,
}

/// Server TTFT for warm samples — returns `None` when Ollama reports a cold model load.
pub fn ollama_warm_server_ttft_ms(
    load_duration: Option<u64>,
    prompt_eval_duration: Option<u64>,
) -> Option<u128> {
    match (load_duration, prompt_eval_duration) {
        (None, None) => None,
        (Some(load), _) if load > ROUT_WARM_LOAD_MAX_NS => None,
        (load, prompt) => {
            let load_ms = load.unwrap_or(0) as u128 / 1_000_000;
            let prompt_ms = prompt.unwrap_or(0) as u128 / 1_000_000;
            Some(load_ms + prompt_ms)
        }
    }
}

fn model_name_matches(resident: &str, requested: &str) -> bool {
    resident == requested
        || resident.strip_suffix(":latest") == Some(requested)
        || requested.strip_suffix(":latest") == Some(resident)
        || resident.starts_with(requested)
            && resident
                .chars()
                .nth(requested.len())
                .is_some_and(|c| c == ':')
}

#[derive(Deserialize)]
struct StreamMessage {
    #[serde(default)]
    content: String,
}

pub async fn fetch_ollama_embedding(endpoint: &str, model: &str, text: &str) -> Result<Vec<f32>, EmbedderError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let url = format!("{}/api/embeddings", endpoint.trim_end_matches('/'));

    let req = EmbeddingRequest {
        model: model.to_string(),
        prompt: text.to_string(),
    };

    let resp = client.post(&url).json(&req).send().await?;

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

#[derive(Error, Debug, PartialEq, Eq)]
pub enum LoopError {
    #[error("Max iterations ({0}) exceeded")]
    MaxIterations(usize),
    #[error("Token budget exceeded: {used} / {max}")]
    BudgetExceeded { used: usize, max: usize },
    #[error("Turn error: {0}")]
    Turn(String),
}

impl From<String> for LoopError {
    fn from(s: String) -> Self {
        LoopError::Turn(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnResult {
    pub iteration: usize,
    pub action: String,
    pub done: bool,
}

pub trait LoopEngine {
    fn run_turn_loop(&self, max_iterations: usize) -> Result<Vec<TurnResult>, LoopError>;
}

pub struct StubLoopEngine {
    actions: Vec<String>,
}

#[cfg(test)]
impl StubLoopEngine {
    pub fn new(actions: Vec<String>) -> Self {
        Self { actions }
    }
}

#[cfg(test)]
impl LoopEngine for StubLoopEngine {
    fn run_turn_loop(&self, max_iterations: usize) -> Result<Vec<TurnResult>, LoopError> {
        let mut results = Vec::new();
        for (i, action) in self.actions.iter().enumerate().take(max_iterations) {
            let done = action == "done" || i + 1 == max_iterations;
            results.push(TurnResult {
                iteration: i + 1,
                action: action.clone(),
                done,
            });
            if action == "done" {
                break;
            }
        }
        if results.is_empty() {
            return Err(LoopError::Turn("No actions to execute".into()));
        }
        Ok(results)
    }
}

#[derive(Error, Debug)]
pub enum GitError {
    #[error("Git command failed: {0}")]
    Command(String),
    #[error("Unexpected git output: {0}")]
    Output(String),
    #[error("Permission denied: {0}")]
    Permission(String),
    #[error("Database error: {0}")]
    Database(String),
}

pub struct GitOps;

impl GitOps {
    /// Initialize repo, commit, and create branch — grant-checked before any git subprocess.
    pub fn init_commit_and_branch(
        conn: &Connection,
        session_id: &str,
        repo_dir: &Path,
        branch_name: &str,
    ) -> Result<(), GitError> {
        let workspace = if repo_dir.exists() {
            repo_dir
                .canonicalize()
                .map_err(|e| GitError::Command(e.to_string()))?
        } else {
            repo_dir.to_path_buf()
        };
        let workspace_str = workspace.to_string_lossy().to_string();

        let decision = PermissionManager::check_file_access(conn, session_id, &workspace_str, "write")
            .map_err(|e| GitError::Database(e.to_string()))?;

        if decision != PermissionDecision::Approved {
            return Err(GitError::Permission(format!(
                "Write grant required for git operations on {}",
                workspace_str
            )));
        }

        std::fs::create_dir_all(&workspace).map_err(|e| GitError::Command(e.to_string()))?;

        run_git(&workspace, &["init"])?;

        let readme = workspace.join("README.md");
        std::fs::write(&readme, "# AetherForge GIT-01\n").map_err(|e| GitError::Command(e.to_string()))?;

        run_git(&workspace, &["add", "README.md"])?;
        run_git(&workspace, &["commit", "-m", "Initial commit"])?;
        run_git(&workspace, &["checkout", "-b", branch_name])?;

        let current = run_git_output(&workspace, &["branch", "--show-current"])?;
        if current.trim() != branch_name {
            return Err(GitError::Output(format!(
                "Expected branch {}, got {}",
                branch_name, current
            )));
        }

        let log = run_git_output(&workspace, &["log", "--oneline", "-1"])?;
        if log.trim().is_empty() {
            return Err(GitError::Output("No commits found after init".into()));
        }

        Ok(())
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<(), GitError> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|e| GitError::Command(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::Command(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr
        )));
    }
    Ok(())
}

fn run_git_output(cwd: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|e| GitError::Command(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitError::Command(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Error, Debug)]
pub enum LintError {
    #[error("Lint command failed: {0}")]
    Command(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct PythonLinter;

impl PythonLinter {
    pub fn check_syntax(source: &str) -> Result<Vec<SyntaxIssue>, LintError> {
        let tmp = tempfile::NamedTempFile::new()?;
        std::fs::write(tmp.path(), source)?;

        let output = Command::new("python3")
            .args(["-m", "py_compile", tmp.path().to_str().unwrap_or("snippet.py")])
            .output()
            .map_err(|e| LintError::Command(e.to_string()))?;

        if output.status.success() {
            return Ok(Vec::new());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(parse_python_syntax_errors(&stderr))
    }
}

fn parse_python_syntax_errors(stderr: &str) -> Vec<SyntaxIssue> {
    let mut issues = Vec::new();
    for line in stderr.lines() {
        if let Some(rest) = line.strip_prefix("  File \"") {
            if let Some(line_part) = rest.split(", line ").nth(1) {
                if let Ok(line_no) = line_part.split(',').next().unwrap_or("0").trim().parse::<usize>()
                {
                    issues.push(SyntaxIssue {
                        line: line_no,
                        message: "SyntaxError".into(),
                    });
                }
            }
        } else if line.starts_with("SyntaxError:") || line.starts_with("IndentationError:") {
            if let Some(last) = issues.last_mut() {
                last.message = line.to_string();
            }
        }
    }
    issues
}

/// Default TCP port for aether-daemon JSON-lines IPC (Phase 1).
pub fn default_daemon_port() -> u16 {
    std::env::var("AETHER_DAEMON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7433)
}

pub fn default_daemon_addr() -> String {
    std::env::var("AETHER_DAEMON_ADDR").unwrap_or_else(|_| {
        format!("127.0.0.1:{}", default_daemon_port())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_loop_engine() {
        let engine = StubLoopEngine::new(vec!["think".into(), "done".into()]);
        let turns = engine.run_turn_loop(5).unwrap();
        assert_eq!(turns.len(), 2);
        assert!(turns.last().unwrap().done);
    }

    #[test]
    fn test_python_linter_finds_syntax_error() {
        let source = "def broken(\n    pass\n";
        let issues = PythonLinter::check_syntax(source).unwrap();
        assert!(!issues.is_empty());
        assert!(issues[0].line >= 1);
    }
}
