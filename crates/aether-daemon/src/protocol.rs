use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RequestLine {
    pub method: String,
    #[serde(default)]
    pub params: RequestParams,
}

#[derive(Debug, Default, Deserialize)]
pub struct RequestParams {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub workspace_path: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<usize>,
    #[serde(default)]
    pub auth_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EventLine {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iterations: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

impl EventLine {
    fn base(event_type: &str) -> Self {
        Self {
            event_type: event_type.into(),
            text: None,
            ttft_ms: None,
            content: None,
            message: None,
            model: None,
            iteration: None,
            tool: None,
            output: None,
            passed: None,
            detail: None,
            iterations: None,
            summary: None,
            action: None,
        }
    }

    pub fn token(text: String) -> Self {
        let mut e = Self::base("token");
        e.text = Some(text);
        e
    }

    pub fn token_with_ttft(text: String, ttft_ms: u128) -> Self {
        let mut e = Self::base("token");
        e.text = Some(text);
        e.ttft_ms = Some(ttft_ms);
        e
    }

    pub fn done(content: String, ttft_ms: u128, model: String) -> Self {
        let mut e = Self::base("done");
        e.content = Some(content);
        e.ttft_ms = Some(ttft_ms);
        e.model = Some(model);
        e
    }

    pub fn error(message: String) -> Self {
        let mut e = Self::base("error");
        e.message = Some(message);
        e
    }

    pub fn plan(iteration: usize, action: &str) -> Self {
        let mut e = Self::base("plan");
        e.iteration = Some(iteration);
        e.action = Some(action.to_string());
        e
    }

    pub fn tool(iteration: usize, tool: &str, output: &str) -> Self {
        let mut e = Self::base("tool");
        e.iteration = Some(iteration);
        e.tool = Some(tool.to_string());
        e.output = Some(output.to_string());
        e
    }

    pub fn observe(iteration: usize, summary: &str) -> Self {
        let mut e = Self::base("observe");
        e.iteration = Some(iteration);
        e.summary = Some(summary.to_string());
        e
    }

    pub fn verify(iteration: usize, passed: bool, detail: &str) -> Self {
        let mut e = Self::base("verify");
        e.iteration = Some(iteration);
        e.passed = Some(passed);
        e.detail = Some(detail.to_string());
        e
    }

    pub fn pong() -> Self {
        Self::base("pong")
    }
}
