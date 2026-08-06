//! Failure forensics (Phase 12 / FORENSIC-01).
use crate::session_log::{SessionLogPayload, SessionLogRecord};
use aether_core::ToolInvocation;
use serde::{Deserialize, Serialize};

pub const FORENSIC01_MIN_ACCURACY: f64 = 0.80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    MissingGrant,
    ContextExhaustion,
    SchemaFailure,
    WrongTool,
    BadToolOutput,
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForensicTrajectory {
    pub id: String,
    #[serde(rename = "human_label")]
    pub human_label: FailureClass,
    #[serde(default)]
    pub goal: Option<String>,
    pub records: Vec<SessionLogRecord>,
    #[serde(default)]
    pub regression_plan: Option<Vec<ToolInvocation>>,
}

#[derive(Debug, Clone)]
pub struct RegressionCase {
    pub trajectory_id: String,
    pub failure_class: FailureClass,
    pub goal: String,
    pub plan: Vec<ToolInvocation>,
}

pub fn classify_failure_trajectory(records: &[SessionLogRecord]) -> FailureClass {
    for record in records {
        match &record.payload {
            SessionLogPayload::Error { message } => {
                let message = message.to_ascii_lowercase();
                if message.contains("permission")
                    || message.contains("grant")
                    || message.contains("ungranted")
                {
                    return FailureClass::MissingGrant;
                }
                if message.contains("token budget") || message.contains("budget exceeded") {
                    return FailureClass::ContextExhaustion;
                }
                if message.contains("invalid plan")
                    || message.contains("schema")
                    || message.contains("json")
                {
                    return FailureClass::SchemaFailure;
                }
                if message.contains("unexpected tool") {
                    return FailureClass::WrongTool;
                }
            }
            SessionLogPayload::Budget {
                tokens_used,
                max_tokens,
                ..
            } if *tokens_used >= *max_tokens => return FailureClass::ContextExhaustion,
            SessionLogPayload::Tool { tool, output, .. } => {
                let output = output.to_ascii_lowercase();
                if output.contains("permission") || output.contains("grant") {
                    return FailureClass::MissingGrant;
                }
                if tool == "wrong_tool" || output.contains("unexpected invocation") {
                    return FailureClass::WrongTool;
                }
            }
            SessionLogPayload::Verify { passed, detail, .. } if !*passed => {
                let detail = detail.to_ascii_lowercase();
                if detail.contains("schema") {
                    return FailureClass::SchemaFailure;
                }
                return FailureClass::BadToolOutput;
            }
            _ => {}
        }
    }
    FailureClass::Unknown
}

pub fn classification_accuracy(trajectories: &[ForensicTrajectory]) -> f64 {
    if trajectories.is_empty() {
        return 0.0;
    }
    trajectories
        .iter()
        .filter(|t| classify_failure_trajectory(&t.records) == t.human_label)
        .count() as f64
        / trajectories.len() as f64
}

pub fn export_regression_case(trajectory: &ForensicTrajectory) -> RegressionCase {
    RegressionCase {
        trajectory_id: trajectory.id.clone(),
        failure_class: classify_failure_trajectory(&trajectory.records),
        goal: trajectory
            .goal
            .clone()
            .unwrap_or_else(|| format!("replay {}", trajectory.id)),
        plan: trajectory
            .regression_plan
            .clone()
            .unwrap_or_else(|| vec![ToolInvocation::Done]),
    }
}

pub fn load_forensic_trajectories(raw: &str) -> Result<Vec<ForensicTrajectory>, String> {
    #[derive(Deserialize)]
    struct Fixture {
        schema_version: u32,
        cases: Vec<ForensicTrajectory>,
    }
    let fixture: Fixture = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if fixture.schema_version != 1 {
        return Err(format!("bad schema {}", fixture.schema_version));
    }
    if fixture.cases.len() < 12 {
        return Err(format!("need >=12, got {}", fixture.cases.len()));
    }
    Ok(fixture.cases)
}
