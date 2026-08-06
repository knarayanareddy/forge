//! Offline degradation probes (Phase 12 slice 12.9 / OFFLINE-01).

use std::time::{Duration, Instant};

/// Network-dependent paths that must degrade clearly when offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPath {
    OllamaHealth,
    OllamaChat,
    OllamaEmbed,
}

impl NetworkPath {
    pub fn all() -> &'static [Self] {
        &[
            Self::OllamaHealth,
            Self::OllamaChat,
            Self::OllamaEmbed,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OllamaHealth => "ollama_health",
            Self::OllamaChat => "ollama_chat",
            Self::OllamaEmbed => "ollama_embed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStatus {
    Degraded { message: String },
    Available,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineMatrix {
    pub paths: Vec<(String, PathStatus)>,
}

impl OfflineMatrix {
    pub fn degraded_count(&self) -> usize {
        self.paths
            .iter()
            .filter(|(_, status)| matches!(status, PathStatus::Degraded { .. }))
            .count()
    }

    pub fn all_degraded_with_messages(&self) -> bool {
        self.paths.iter().all(|(_, status)| {
            matches!(status, PathStatus::Degraded { message } if !message.is_empty())
        })
    }
}

fn offline_client(per_path_timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(per_path_timeout)
        .connect_timeout(per_path_timeout)
        .build()
        .expect("offline probe client")
}

/// Probe Ollama health/chat/embed paths against an unreachable endpoint.
pub async fn probe_offline_degradation(
    dead_endpoint: &str,
    per_path_timeout: Duration,
) -> OfflineMatrix {
    let client = offline_client(per_path_timeout);
    let base = dead_endpoint.trim_end_matches('/');
    let mut paths = Vec::new();
    for path in NetworkPath::all() {
        let status = probe_one(*path, &client, base, per_path_timeout).await;
        paths.push((path.label().to_string(), status));
    }
    OfflineMatrix { paths }
}

async fn probe_one(
    path: NetworkPath,
    client: &reqwest::Client,
    base: &str,
    timeout: Duration,
) -> PathStatus {
    let label = path.label();
    let start = Instant::now();
    let request = match path {
        NetworkPath::OllamaHealth => client.get(format!("{base}/api/tags")),
        NetworkPath::OllamaChat => client.post(format!("{base}/api/chat")).json(&serde_json::json!({
            "model": "qwen2.5:3b",
            "messages": [{"role": "user", "content": "offline probe"}],
            "stream": false
        })),
        NetworkPath::OllamaEmbed => client.post(format!("{base}/api/embeddings")).json(&serde_json::json!({
            "model": "all-minilm",
            "prompt": "offline probe"
        })),
    };

    let outcome = tokio::time::timeout(timeout, request.send()).await;
    let elapsed = start.elapsed();

    if elapsed >= timeout {
        return PathStatus::Degraded {
            message: format!("{label}: timed out after {:?}", timeout),
        };
    }

    match outcome {
        Ok(Ok(resp)) if resp.status().is_success() => PathStatus::Available,
        Ok(Ok(resp)) => PathStatus::Degraded {
            message: format!("{label}: HTTP {}", resp.status()),
        },
        Ok(Err(e)) => PathStatus::Degraded {
            message: format!("{label}: {e}"),
        },
        Err(_) => PathStatus::Degraded {
            message: format!("{label}: request timed out"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dead_endpoint_degrades_all_paths() {
        let matrix =
            probe_offline_degradation("http://127.0.0.1:1", Duration::from_millis(800)).await;
        assert_eq!(matrix.degraded_count(), NetworkPath::all().len());
        assert!(matrix.all_degraded_with_messages());
    }
}
