//! Session-log failure forensics (Phase 12 / FORENSIC-01).
//!
//! Classifies failed trajectories from append-only JSONL transcripts and exports
//! frozen regression cases for replay without re-running inference.

use crate::session_log::{SessionLogPayload, SessionLogRecord};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const FORENSIC01_MIN_ACCURACY: f64 = 0.80;
pub const DEFAULT_FORENSIC_FIXTURE: &str =
    "tests/golden_harness/fixtures/forensic01_corpus.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    WrongTool,
    ContextExhaustion,
    SchemaFailure,
    MissingGrant,
    BadToolOutput,
    Unknown,
}

impl FailureClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WrongTool => "wrong_tool",
            Self::ContextExhaustion => "context_exhaustion",
            Self::SchemaFailure => "schema_failure",
            Self::MissingGrant => "missing_grant",
            Self::BadToolOutput => "bad_tool_output",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(label: &str) -> Result<Self, ForensicError> {
        match label {
            "wrong_tool" => Ok(Self::WrongTool),
            "context_exhaustion" => Ok(Self::ContextExhaustion),
            "schema_failure" => Ok(Self::SchemaFailure),
            "missing_grant" => Ok(Self::MissingGrant),
            "bad_tool_output" => Ok(Self::BadToolOutput),
            "unknown" => Ok(Self::Unknown),
            other => Err(ForensicError::Corpus(format!("unknown label {other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ForensicCase {
    pub id: String,
    pub human_label: String,
    pub records: Vec<SessionLogRecord>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ForensicCorpus {
    pub schema_version: u32,
    pub min_accuracy: f64,
    pub cases: Vec<ForensicCase>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionCase {
    pub case_id: String,
    pub human_label: String,
    pub predicted_class: String,
    pub records: Vec<SessionLogRecord>,
}

#[derive(Debug, Error)]
pub enum ForensicError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("corpus error: {0}")]
    Corpus(String),
}

pub fn discover_forensic_fixture() -> PathBuf {
    if let Ok(path) = std::env::var("AETHER_FORENSIC01_FIXTURE") {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(DEFAULT_FORENSIC_FIXTURE)
}

pub fn load_forensic_corpus(path: &Path) -> Result<ForensicCorpus, ForensicError> {
    let raw = std::fs::read_to_string(path)?;
    let corpus: ForensicCorpus = serde_json::from_str(&raw)?;
    if corpus.schema_version != 1 {
        return Err(ForensicError::Corpus(format!(
            "unsupported schema_version {}",
            corpus.schema_version
        )));
    }
    if corpus.cases.len() != 12 {
        return Err(ForensicError::Corpus(format!(
            "expected 12 cases, got {}",
            corpus.cases.len()
        )));
    }
    Ok(corpus)
}

fn message_blob(records: &[SessionLogRecord]) -> String {
    records
        .iter()
        .map(|r| match &r.payload {
            SessionLogPayload::Error { message } => message.clone(),
            SessionLogPayload::Verify { detail, passed, .. } => {
                format!("verify passed={passed} {detail}")
            }
            SessionLogPayload::Plan { action, .. } => action.clone(),
            SessionLogPayload::Tool { tool, output, .. } => format!("{tool} {output}"),
            SessionLogPayload::Budget {
                tokens_used,
                max_tokens,
                ..
            } => format!("budget {tokens_used}/{max_tokens}"),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase()
}

pub fn classify_failure_trajectory(records: &[SessionLogRecord]) -> FailureClass {
    let blob = message_blob(records);

    if blob.contains("grant")
        || blob.contains("permission denied")
        || blob.contains("write grant required")
        || blob.contains("capability")
    {
        return FailureClass::MissingGrant;
    }

    if blob.contains("token budget exceeded")
        || blob.contains("budget exceeded")
        || blob.contains("context length")
        || blob.contains("context exhausted")
        || blob.contains("max tokens")
    {
        return FailureClass::ContextExhaustion;
    }

    if blob.contains("schema")
        || blob.contains("json parse")
        || blob.contains("invalid json")
        || blob.contains("decode error")
        || blob.contains("malformed plan")
    {
        return FailureClass::SchemaFailure;
    }

    if blob.contains("verify failed")
        || blob.contains("verify_contains")
        || blob.contains("python_lint")
        || blob.contains("lint failed")
        || blob.contains("unexpected tool output")
    {
        return FailureClass::BadToolOutput;
    }

    if blob.contains("wrong tool")
        || blob.contains("unknown tool")
        || blob.contains("unexpected action")
        || blob.contains("forbidden tool")
    {
        return FailureClass::WrongTool;
    }

    if records.iter().any(|r| matches!(r.payload, SessionLogPayload::Error { .. })) {
        return FailureClass::Unknown;
    }

    FailureClass::Unknown
}

pub fn classification_accuracy(corpus: &ForensicCorpus) -> f64 {
    if corpus.cases.is_empty() {
        return 0.0;
    }
    let correct = corpus
        .cases
        .iter()
        .filter(|case| {
            FailureClass::parse(&case.human_label)
                .map(|expected| classify_failure_trajectory(&case.records) == expected)
                .unwrap_or(false)
        })
        .count();
    correct as f64 / corpus.cases.len() as f64
}

pub fn export_regression_case(case: &ForensicCase) -> RegressionCase {
    let predicted = classify_failure_trajectory(&case.records);
    RegressionCase {
        case_id: case.id.clone(),
        human_label: case.human_label.clone(),
        predicted_class: predicted.as_str().to_string(),
        records: case.records.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_log::SESSION_LOG_SCHEMA_VERSION;

    fn error_record(message: &str) -> SessionLogRecord {
        SessionLogRecord {
            schema_version: SESSION_LOG_SCHEMA_VERSION,
            session_id: "sess-test".into(),
            turn_index: 1,
            seq: 0,
            unix_ms: 1,
            payload: SessionLogPayload::Error {
                message: message.into(),
            },
        }
    }

    #[test]
    fn classifies_missing_grant() {
        let records = vec![error_record("Workspace write grant required for /tmp/ws")];
        assert_eq!(
            classify_failure_trajectory(&records),
            FailureClass::MissingGrant
        );
    }

    #[test]
    fn fixture_meets_accuracy_gate() {
        let corpus = load_forensic_corpus(&discover_forensic_fixture()).expect("fixture");
        assert!(classification_accuracy(&corpus) >= FORENSIC01_MIN_ACCURACY);
    }
}
