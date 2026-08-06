mod compaction;
mod cost;
mod graph_extract;
mod hf_hub;
mod model_registry;
mod prefix_cache;
mod hooks;
mod inject;
mod keychain;
mod loop_engine;
mod nl_planner;
mod orchestration_graph;
mod risk;
mod subagent;
mod tool_reliability;
mod verifier_node;

pub use cost::{audit_loop_token_usage, ollama_token_usage, openai_token_usage, ProviderTokenUsage,};

pub use graph_extract::{
    build_graph_extract_prompt, enforce_max_entities, graph_extract_schema_json,
    payload_to_graph_inserts, run_graph_extract, strip_json_fence, validate_graph_extract,
    ExtractEdge, ExtractNode, GraphExtractError, GraphExtractPayload, PreparedGraphEdge,
    PreparedGraphNode, Provenance, GRAPH_EXTRACT_SCHEMA_PATH,
};

pub use compaction::{
    compact_turns, mechanical_summarize, CompactRequest, CompactResult, CompactionError,
    ContextTurn,
};

pub use hooks::{
    post_tool_use_scrub_output, pre_tool_use_path_check, HookDecision, HookEngine, HookPhase,
    DEFAULT_DENY_PATH_PATTERNS, DEFAULT_DENY_PROMPT_PATTERNS, DEFAULT_REDACT_OUTPUT_PATTERNS,
};

pub use inject::{
    admit_plan_against_observations, tool_result_has_injection_phrase, wrap_untrusted_tool_output,
    AdmitDecision, CorrelationFinding, ToolDepEdge, ToolDependencyGraph,
    MIN_CORRELATION_SUBSTRING, TOOL_RESULT_INJECTION_PATTERNS,
};

pub use hf_hub::{download_file, sha256_hex, DownloadPlan, HfHubError};
pub use prefix_cache::{
    assemble_context_prompt, build_volatile_replan_tail, measure_prefix_reuse, prefix_fingerprint,
    sort_tool_results_deterministic, static_tools_prefix, CACHE01_MIN_REUSE_RATIO,
};

pub use model_registry::{
    discover_registry_path, load_discovered_registry, BackendKind,
    ModelProfile, ModelRegistry, RegistryError, ENV_MODEL_PROFILE, ENV_MODEL_PROFILE_COMPLEX,
    ENV_REGISTRY_PATH,
};

pub use risk::{evaluate_approval_gate, find_steps_requiring_approval, RiskyStep};

pub use subagent::{
    run_subagent_read_task, SubagentFileSummary, SubagentResult, MAX_DISTILLED_CHARS,
    MAX_SUBAGENT_FILES,
};

pub use tool_reliability::{
    evaluate_profile_reliability, rank_profiles_by_reliability, score_tool_response,
    FrozenToolResponse, ToolCallCase,
};

pub use keychain::{
    ensure_daemon_auth_token, load_byok_key, load_daemon_auth_token, load_daemon_auth_token_file,
    delete_named_secret, load_gateway_token, load_named_secret, require_byok_key_if_configured,
    store_byok_key, store_daemon_auth_token, store_gateway_token, store_named_secret,
    verify_daemon_auth_token,
    verify_daemon_auth_token_expected, KeychainError, BYOK_ACCOUNT, BYOK_SERVICE,
    DAEMON_AUTH_ACCOUNT,
};

pub use loop_engine::{
    GoalStopHook, LoopConfig, LoopRunResult, LoopStreamEvent, PythonLintVerifier, ReActLoopEngine,
    record_provider_token_usage, resolve_default_max_loop_tokens, StopHook, ToolInvocation, ToolObservation, ToolRegistry,
    Verifier, DEFAULT_MAX_LOOP_TOKENS,
};

pub use nl_planner::{
    build_nl_plan_prompt, build_nl_repair_prompt, build_nl_verify_repair_prompt, nl_plan_schema,
    normalize_nl_plan_json, plan_tool_name, run_nl_planner, run_nl_planner_repair, NlPlannerResult,
    validate_nl_plan, validate_nl_plan_gold_trajectory, validate_goal_coverage, NlPlanError,
    LOOP02_EVAL_PROMPT, LOOP02_GOLD_TOOL_ORDER, NL_PLAN_SCHEMA,
};

pub use orchestration_graph::OrchestrationGraph;
pub use verifier_node::{MakerCheckerGoal, VerifierNode};

use aether_permissions::{PermissionDecision, PermissionManager};
use aether_sandbox::ProductionSandbox;
use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
use std::pin::Pin;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelBackend {
    OllamaMlx { endpoint: String, model: String },
    LlamaCpp { model_path: String },
    MlxLocal { model_path: String },
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
    active_profile: String,
}

impl ModelRouter {
    pub fn new(primary: ModelBackend, fallback: Option<ModelBackend>) -> Self {
        Self { primary, fallback, active_profile: String::new() }
    }
    fn with_active_profile(mut self, label: String) -> Self { self.active_profile = label; self }

    pub fn from_env() -> Result<Self, KeychainError> {
        if let Some(api_key) = require_byok_key_if_configured()? {
            let provider = std::env::var("AETHER_BYOK_PROVIDER").unwrap_or_else(|_| "openai".into());
            let model = std::env::var("AETHER_BYOK_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
            return Ok(Self::new(ModelBackend::ByokCloud { provider, api_key, model }, None).with_active_profile("byok-env".into()));
        }
        if let Ok(registry) = load_discovered_registry() {
            if let Ok(r) = Self::from_registry(&registry) { return Ok(r); }
        }
        Self::from_env_ollama_only()
    }
    pub fn from_env_ollama_only() -> Result<Self, KeychainError> {
        let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT").unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());
        let complex_model = std::env::var("AETHER_CHAT_MODEL_COMPLEX").unwrap_or_else(|_| model.clone());
        Ok(Self::new(ModelBackend::OllamaMlx { endpoint: endpoint.clone(), model: model.clone() }, Some(ModelBackend::OllamaMlx { endpoint, model: complex_model })).with_active_profile("ollama-env".into()))
    }
    pub fn from_registry(registry: &ModelRegistry) -> Result<Self, RegistryError> {
        let primary_label = registry.primary_profile_id();
        let mut router = registry.build_router()?;
        router.active_profile = primary_label;
        Ok(router)
    }
    pub fn active_profile_label(&self) -> &str { if self.active_profile.is_empty() { "legacy" } else { &self.active_profile } }

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
        let mut token_usage = None;

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
                token_usage = chunk.token_usage;
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
            token_usage,
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
        OllamaProvider::complete_json_backend(self.primary(), prompt, num_predict, None).await
    }

    /// Schema-constrained JSON completion.
    ///
    /// Ollama compiles the schema to a decoding grammar; BYOK uses OpenAI structured outputs.
    pub async fn complete_json_schema(
        &self,
        prompt: &str,
        num_predict: u32,
        schema: &serde_json::Value,
    ) -> Result<CompletionResult, CompleteError> {
        OllamaProvider::complete_json_backend(self.primary(), prompt, num_predict, Some(schema))
            .await
    }
}

pub type TokenStream = Pin<Box<dyn Stream<Item = Result<TokenChunk, CompleteError>> + Send>>;

#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub content: String,
    pub ttft_ms: u128,
    pub model: String,
    pub token_usage: Option<ProviderTokenUsage>,
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
    pub token_usage: Option<ProviderTokenUsage>,
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

    /// Probe the embed model so Ollama loads it before MEM-01 / GRAPH-01 embedding work.
    pub async fn warm_embed_model(endpoint: &str, model: &str) -> Result<(), CompleteError> {
        fetch_ollama_embedding(endpoint, model, "forge embed warmup")
            .await
            .map(|_| ())
            .map_err(|e| CompleteError::Api(e.to_string()))
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
            ModelBackend::LlamaCpp { model_path } => Err(CompleteError::Api(format!("GGUF backend not wired for inference yet (registry path: {model_path})"))),
            ModelBackend::MlxLocal { model_path } => Err(CompleteError::Api(format!("MLX backend not wired for inference yet (registry path: {model_path})"))),
        }
    }

    pub async fn complete_json_backend(
        backend: &ModelBackend,
        prompt: &str,
        num_predict: u32,
        schema: Option<&serde_json::Value>,
    ) -> Result<CompletionResult, CompleteError> {
        match backend {
            ModelBackend::OllamaMlx { endpoint, model } => {
                Self::complete_json_ollama(endpoint, model, prompt, num_predict, schema).await
            }
            ModelBackend::ByokCloud {
                provider,
                api_key,
                model,
            } => Self::complete_json_byok(provider, api_key, model, prompt, num_predict, schema).await,
            ModelBackend::LlamaCpp { model_path } => Err(CompleteError::Api(format!("GGUF backend not wired for inference yet (registry path: {model_path})"))),
            ModelBackend::MlxLocal { model_path } => Err(CompleteError::Api(format!("MLX backend not wired for inference yet (registry path: {model_path})"))),
        }
    }

    async fn complete_json_ollama(
        endpoint: &str,
        model: &str,
        prompt: &str,
        num_predict: u32,
        schema: Option<&serde_json::Value>,
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
            format: Some(match schema {
                Some(schema) => schema.clone(),
                None => serde_json::Value::String("json".into()),
            }),
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
            token_usage: ollama_token_usage(data.prompt_eval_count, data.eval_count),
        })
    }

    async fn complete_json_byok(
        provider: &str,
        api_key: &str,
        model: &str,
        prompt: &str,
        num_predict: u32,
        schema: Option<&serde_json::Value>,
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
            response_format: Some(match schema {
                Some(schema) => OpenAiResponseFormat {
                    format_type: "json_schema".to_string(),
                    json_schema: Some(OpenAiJsonSchema {
                        name: "aether_plan".to_string(),
                        schema: schema.clone(),
                        strict: false,
                    }),
                },
                None => OpenAiResponseFormat {
                    format_type: "json_object".to_string(),
                    json_schema: None,
                },
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
            token_usage: data.usage.as_ref().and_then(|u| openai_token_usage(Some(u.prompt_tokens), Some(u.completion_tokens))),
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
                            token_usage: ollama_token_usage(data.prompt_eval_count, data.eval_count),
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
                            token_usage: None,
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
                                token_usage: None,
                            };
                        }
                        if choice.finish_reason.as_deref() == Some("stop") {
                            yield TokenChunk {
                                text: String::new(),
                                ttft_ms: None,
                                server_ttft_ms: None,
                                model: model_name.clone(),
                                done: true,
                                token_usage: data.usage.as_ref().and_then(|u| openai_token_usage(Some(u.prompt_tokens), Some(u.completion_tokens))),
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
            prompt_eval_count: None,
            eval_count: None,
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
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
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
    usage: Option<OpenAiUsage>,
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
    /// `"json"` for free-form JSON mode, or a JSON Schema object for constrained decoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<ChatOptions>,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: StreamMessage,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
}

#[derive(Serialize)]
struct OpenAiResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_schema: Option<OpenAiJsonSchema>,
}

#[derive(Serialize)]
struct OpenAiJsonSchema {
    name: String,
    schema: serde_json::Value,
    strict: bool,
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
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
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
    /// A `verify_contains`/`python_lint` step failed before `done`. Distinct from `Turn` so a
    /// caller with a natural-language goal on hand (Phase 9 slice 9.9-9.10 / LOOP-04) can attempt
    /// a bounded replan instead of aborting; every other caller treats it as a plain failure.
    #[error("Verify failed on {failed_tool}: {detail}")]
    VerifyFailed {
        failed_tool: String,
        detail: String,
        iterations_used: usize,
        observations: Vec<crate::loop_engine::ToolObservation>,
    },
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

        if !workspace.is_dir() {
            return Err(GitError::Command(format!(
                "Git workspace must already exist: {}",
                workspace.display()
            )));
        }

        run_git(&workspace, &["init"])?;

        let readme = workspace.join("README.md");
        ProductionSandbox::write_file(&workspace, &readme, b"# AetherForge GIT-01\n")
            .map_err(|e| GitError::Command(e.to_string()))?;

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
    let mut command = ProductionSandbox::command("git", args, cwd)
        .map_err(|e| GitError::Command(e.to_string()))?;
    let output = command
        .current_dir(cwd)
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
    let mut command = ProductionSandbox::command("git", args, cwd)
        .map_err(|e| GitError::Command(e.to_string()))?;
    let output = command
        .current_dir(cwd)
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
        let workspace = tempfile::tempdir()?;
        Self::check_syntax_in_workspace(source, workspace.path())
    }

    pub fn check_syntax_in_workspace(
        source: &str,
        workspace: &Path,
    ) -> Result<Vec<SyntaxIssue>, LintError> {
        let temp_dir = workspace.join(".aether-tmp");
        ProductionSandbox::create_dir_all(workspace, &temp_dir)
            .map_err(|e| LintError::Command(e.to_string()))?;
        let tmp = tempfile::Builder::new()
            .prefix("lint-")
            .suffix(".py")
            .tempfile_in(&temp_dir)?;
        ProductionSandbox::write_file(workspace, tmp.path(), source.as_bytes())
            .map_err(|e| LintError::Command(e.to_string()))?;

        let args = ["-m", "py_compile", tmp.path().to_str().unwrap_or("snippet.py")];
        let output = ProductionSandbox::command("python3", args, workspace)
            .map_err(|e| LintError::Command(e.to_string()))?
            .output()
            .map_err(|e| LintError::Command(e.to_string()))?;

        if output.status.success() {
            return Ok(Vec::new());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let issues = parse_python_syntax_errors(&stderr);
        if issues.is_empty() {
            return Err(LintError::Command(format!(
                "python lint infrastructure failed: {}",
                stderr.trim()
            )));
        }
        Ok(issues)
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
