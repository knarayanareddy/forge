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
        OllamaProvider::complete_backend(backend, prompt).await
    }
}

#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub content: String,
    pub ttft_ms: u128,
    pub model: String,
}

pub struct OllamaProvider;

impl OllamaProvider {
    pub async fn health_check(endpoint: &str) -> Result<(), CompleteError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;
        let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
        let resp = client.get(&url).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(CompleteError::Api(format!("Ollama health check failed: HTTP {}", resp.status())))
        }
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
        Self::complete_backend(&backend, prompt).await
    }

    pub async fn complete_backend(
        backend: &ModelBackend,
        prompt: &str,
    ) -> Result<CompletionResult, CompleteError> {
        let (endpoint, model) = match backend {
            ModelBackend::OllamaMlx { endpoint, model } => (endpoint.as_str(), model.as_str()),
            _ => {
                return Err(CompleteError::Api(
                    "Only OllamaMlx backend supported in MVP router".into(),
                ))
            }
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));
        let req = ChatRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            stream: false,
        };

        let start = Instant::now();
        let resp = client.post(&url).json(&req).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CompleteError::Api(format!("HTTP status {}: {}", status, body)));
        }

        let data: ChatResponse = resp.json().await?;
        let ttft_ms = start.elapsed().as_millis();

        if data.message.content.trim().is_empty() {
            return Err(CompleteError::Api("Empty completion from Ollama".into()));
        }

        Ok(CompletionResult {
            content: data.message.content,
            ttft_ms,
            model: model.to_string(),
        })
    }
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
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
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

#[derive(Error, Debug)]
pub enum LoopError {
    #[error("Max iterations ({0}) exceeded")]
    MaxIterations(usize),
    #[error("Turn error: {0}")]
    Turn(String),
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

impl StubLoopEngine {
    pub fn new(actions: Vec<String>) -> Self {
        Self { actions }
    }
}

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
}

pub struct GitOps;

impl GitOps {
    pub fn init_commit_and_branch(repo_dir: &Path, branch_name: &str) -> Result<(), GitError> {
        run_git(repo_dir, &["init"])?;

        let readme = repo_dir.join("README.md");
        std::fs::write(&readme, "# AetherForge GIT-01\n").map_err(|e| GitError::Command(e.to_string()))?;

        run_git(repo_dir, &["add", "README.md"])?;
        run_git(repo_dir, &["commit", "-m", "Initial commit"])?;
        run_git(repo_dir, &["checkout", "-b", branch_name])?;

        let current = run_git_output(repo_dir, &["branch", "--show-current"])?;
        if current.trim() != branch_name {
            return Err(GitError::Output(format!(
                "Expected branch {}, got {}",
                branch_name, current
            )));
        }

        let log = run_git_output(repo_dir, &["log", "--oneline", "-1"])?;
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
