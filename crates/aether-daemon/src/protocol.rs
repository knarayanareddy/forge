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
}

impl EventLine {
    pub fn token(text: String) -> Self {
        Self {
            event_type: "token".into(),
            text: Some(text),
            ttft_ms: None,
            content: None,
            message: None,
            model: None,
        }
    }

    pub fn token_with_ttft(text: String, ttft_ms: u128) -> Self {
        Self {
            event_type: "token".into(),
            text: Some(text),
            ttft_ms: Some(ttft_ms),
            content: None,
            message: None,
            model: None,
        }
    }

    pub fn done(content: String, ttft_ms: u128, model: String) -> Self {
        Self {
            event_type: "done".into(),
            text: None,
            ttft_ms: Some(ttft_ms),
            content: Some(content),
            message: None,
            model: Some(model),
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            event_type: "error".into(),
            text: None,
            ttft_ms: None,
            content: None,
            message: Some(message),
            model: None,
        }
    }
}
