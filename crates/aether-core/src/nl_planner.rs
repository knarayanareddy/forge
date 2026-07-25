//! NL → structured tool plan for LOOP-02 (Slice 6.6).
//!
//! Plans are validated and executed by the same `ReActLoopEngine` verify shell as LOOP-01.
//! Production validation checks schema, allowed tools, step cap, and forbidden patterns —
//! **not** harness gold tool order (see `validate_nl_plan_gold_trajectory` for eval-only asserts).

use crate::graph_extract::strip_json_fence;
use crate::{ModelRouter, ToolInvocation};
use serde_json::Value;
use thiserror::Error;

/// Frozen LOOP-02 eval goal — natural language, not JSON inject.
pub const LOOP02_EVAL_PROMPT: &str = "Write a file named loop02_marker.txt containing exactly LOOP-02-verified. \
Verify loop02_marker.txt contains LOOP-02-verified. Lint this Python source: def ok():\n    return 1\n. \
Then finish the plan.";

/// Gold tool trajectory for LOOP-02 harness asserts only — not enforced in production validation.
pub const LOOP02_GOLD_TOOL_ORDER: &[&str] =
    &["fs_write", "verify_contains", "python_lint", "done"];

const NL_PLAN_NUM_PREDICT: u32 = 1024;

const ALLOWED_NL_TOOLS: &[&str] = &[
    "fs_write",
    "fs_read",
    "verify_contains",
    "python_lint",
    "git_init",
    "mcp_call",
    "skill_execute",
    "done",
];

#[derive(Error, Debug, PartialEq)]
pub enum NlPlanError {
    #[error("Malformed JSON: {0}")]
    Json(String),
    #[error("Missing `loop` array in planner output")]
    MissingLoopArray,
    #[error("Empty planner plan")]
    EmptyPlan,
    #[error("Invalid tool step at index {index}: {detail}")]
    InvalidStep { index: usize, detail: String },
    #[error("Disallowed tool `{tool}` at step {index}")]
    DisallowedTool { index: usize, tool: String },
    #[error("Forbidden plan pattern at step {index}: {detail}")]
    ForbiddenPattern { index: usize, detail: String },
    #[error("Trajectory mismatch at step {index}: expected {expected}, got {actual}")]
    TrajectoryMismatch {
        index: usize,
        expected: String,
        actual: String,
    },
    #[error("Redundant tool `{tool}` at step {index}")]
    RedundantTool { index: usize, tool: String },
    #[error("Plan exceeds max_iterations: {steps} steps (max {max})")]
    ExceedsMaxIterations { steps: usize, max: usize },
    #[error("Ollama NL planner failed: {0}")]
    Ollama(String),
}

/// Build the bounded Ollama prompt for NL plan generation.
pub fn build_nl_plan_prompt(nl_goal: &str) -> String {
    format!(
        r#"Convert the user goal below into a JSON tool plan for AetherForge ReAct loop execution.
Return ONLY valid JSON (no markdown fences) with this shape:
{{"loop":[{{"action":"fs_write","path":"...","content":"..."}},{{"action":"verify_contains","path":"...","text":"..."}},{{"action":"python_lint","source":"..."}},{{"action":"done"}}]}}

Allowed actions only: fs_write, fs_read, verify_contains, python_lint, git_init, mcp_call, skill_execute, done.
Rules:
- Emit exactly one verify_contains after each fs_write target.
- python_lint must run on the provided Python source verbatim.
- End with {{"action":"done"}}.
- Do not repeat the same tool on the same target twice in a row.
- Keep paths relative to the workspace.

User goal:
{nl_goal}"#
    )
}

/// Parse and validate planner JSON into tool invocations (production path).
pub fn validate_nl_plan(json: &str, max_iterations: usize) -> Result<Vec<ToolInvocation>, NlPlanError> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| NlPlanError::Json(e.to_string()))?;

    let steps = value
        .get("loop")
        .and_then(|v| v.as_array())
        .ok_or(NlPlanError::MissingLoopArray)?;

    if steps.is_empty() {
        return Err(NlPlanError::EmptyPlan);
    }

    if steps.len() > max_iterations {
        return Err(NlPlanError::ExceedsMaxIterations {
            steps: steps.len(),
            max: max_iterations,
        });
    }

    let mut plan = Vec::with_capacity(steps.len());
    let mut seen_targets: Vec<(String, String)> = Vec::new();

    for (index, step) in steps.iter().enumerate() {
        let invocation: ToolInvocation = serde_json::from_value(step.clone()).map_err(|e| {
            NlPlanError::InvalidStep {
                index,
                detail: e.to_string(),
            }
        })?;
        let tool = tool_name(&invocation).to_string();

        if !ALLOWED_NL_TOOLS.contains(&tool.as_str()) {
            return Err(NlPlanError::DisallowedTool { index, tool });
        }

        if let Some(detail) = forbidden_pattern_detail(&invocation) {
            return Err(NlPlanError::ForbiddenPattern { index, detail });
        }

        if let Some((prev_tool, prev_key)) = seen_targets.last() {
            let key = tool_target_key(&invocation);
            if prev_tool == &tool && prev_key == &key {
                return Err(NlPlanError::RedundantTool { index, tool });
            }
        }
        seen_targets.push((tool.clone(), tool_target_key(&invocation)));
        plan.push(invocation);
    }

    if plan.len() == 1 && matches!(plan[0], ToolInvocation::Done) {
        return Err(NlPlanError::ForbiddenPattern {
            index: 0,
            detail: "plan must include at least one action before done".into(),
        });
    }

    if plan.last().map(tool_name) != Some("done") {
        return Err(NlPlanError::ForbiddenPattern {
            index: plan.len().saturating_sub(1),
            detail: "plan must end with done".into(),
        });
    }

    Ok(plan)
}

/// Harness-only: assert tool order matches LOOP-02 gold trajectory.
pub fn validate_nl_plan_gold_trajectory(plan: &[ToolInvocation]) -> Result<(), NlPlanError> {
    for (index, expected) in LOOP02_GOLD_TOOL_ORDER.iter().enumerate() {
        let actual = plan
            .get(index)
            .map(tool_name)
            .ok_or_else(|| NlPlanError::TrajectoryMismatch {
                index,
                expected: (*expected).into(),
                actual: "<missing>".into(),
            })?;
        if actual != *expected {
            return Err(NlPlanError::TrajectoryMismatch {
                index,
                expected: (*expected).into(),
                actual: actual.into(),
            });
        }
    }

    if plan.len() != LOOP02_GOLD_TOOL_ORDER.len() {
        return Err(NlPlanError::TrajectoryMismatch {
            index: plan.len().saturating_sub(1),
            expected: format!("{} steps", LOOP02_GOLD_TOOL_ORDER.len()),
            actual: format!("{} steps", plan.len()),
        });
    }

    Ok(())
}

/// Call Ollama via `ModelRouter`, validate JSON, enforce step cap + production rules.
pub async fn run_nl_planner(
    router: &ModelRouter,
    nl_goal: &str,
    max_iterations: usize,
) -> Result<Vec<ToolInvocation>, NlPlanError> {
    let prompt = build_nl_plan_prompt(nl_goal);
    let raw = router
        .complete_json(&prompt, NL_PLAN_NUM_PREDICT)
        .await
        .map_err(|e| NlPlanError::Ollama(e.to_string()))?
        .content;
    let json = strip_json_fence(&raw);
    validate_nl_plan(&json, max_iterations)
}

fn forbidden_pattern_detail(step: &ToolInvocation) -> Option<String> {
    match step {
        ToolInvocation::FsWrite { path, content } => {
            if path.trim().is_empty() {
                return Some("fs_write requires non-empty path".into());
            }
            if content.is_empty() {
                return Some("fs_write requires content".into());
            }
        }
        ToolInvocation::FsRead { path } => {
            if path.trim().is_empty() {
                return Some("fs_read requires non-empty path".into());
            }
        }
        ToolInvocation::VerifyContains { path, text } => {
            if path.trim().is_empty() {
                return Some("verify_contains requires non-empty path".into());
            }
            if text.is_empty() {
                return Some("verify_contains requires non-empty text".into());
            }
        }
        ToolInvocation::PythonLint { source } if source.trim().is_empty() => {
            return Some("python_lint requires non-empty source".into());
        }
        ToolInvocation::GitInit { branch } if branch.trim().is_empty() => {
            return Some("git_init requires non-empty branch".into());
        }
        ToolInvocation::McpCall { server, tool, .. } => {
            if server.trim().is_empty() || tool.trim().is_empty() {
                return Some("mcp_call requires server and tool".into());
            }
        }
        ToolInvocation::SkillExecute { skill_id, .. } if skill_id.trim().is_empty() => {
            return Some("skill_execute requires skill_id".into());
        }
        _ => {}
    }
    None
}

fn tool_name(step: &ToolInvocation) -> &str {
    match step {
        ToolInvocation::FsWrite { .. } => "fs_write",
        ToolInvocation::FsRead { .. } => "fs_read",
        ToolInvocation::PythonLint { .. } => "python_lint",
        ToolInvocation::GitInit { .. } => "git_init",
        ToolInvocation::McpCall { .. } => "mcp_call",
        ToolInvocation::SkillExecute { .. } => "skill_execute",
        ToolInvocation::VerifyContains { .. } => "verify_contains",
        ToolInvocation::Done => "done",
    }
}

fn tool_target_key(step: &ToolInvocation) -> String {
    match step {
        ToolInvocation::FsWrite { path, .. } => path.clone(),
        ToolInvocation::FsRead { path } => path.clone(),
        ToolInvocation::VerifyContains { path, .. } => path.clone(),
        ToolInvocation::PythonLint { source } => source.chars().take(32).collect(),
        ToolInvocation::GitInit { branch } => branch.clone(),
        ToolInvocation::McpCall { server, tool, .. } => format!("{}:{}", server, tool),
        ToolInvocation::SkillExecute { skill_id, .. } => skill_id.clone(),
        ToolInvocation::Done => "done".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_nl_plan_accepts_gold_trajectory() {
        let json = r#"{"loop":[
            {"action":"fs_write","path":"loop02_marker.txt","content":"LOOP-02-verified"},
            {"action":"verify_contains","path":"loop02_marker.txt","text":"LOOP-02-verified"},
            {"action":"python_lint","source":"def ok():\n    return 1\n"},
            {"action":"done"}
        ]}"#;
        let plan = validate_nl_plan(json, 6).unwrap();
        assert_eq!(plan.len(), 4);
        validate_nl_plan_gold_trajectory(&plan).unwrap();
    }

    #[test]
    fn validate_nl_plan_accepts_fs_read_first_non_gold_order() {
        let json = r#"{"loop":[
            {"action":"fs_read","path":"notes.txt"},
            {"action":"done"}
        ]}"#;
        let plan = validate_nl_plan(json, 6).unwrap();
        assert_eq!(plan.len(), 2);
        assert!(validate_nl_plan_gold_trajectory(&plan).is_err());
    }

    #[test]
    fn validate_nl_plan_rejects_done_only_plan() {
        let json = r#"{"loop":[{"action":"done"}]}"#;
        let err = validate_nl_plan(json, 6).unwrap_err();
        assert!(matches!(err, NlPlanError::ForbiddenPattern { .. }));
    }

    #[test]
    fn validate_nl_plan_rejects_missing_done() {
        let json = r#"{"loop":[
            {"action":"fs_read","path":"notes.txt"}
        ]}"#;
        let err = validate_nl_plan(json, 6).unwrap_err();
        assert!(matches!(err, NlPlanError::ForbiddenPattern { .. }));
    }

    #[test]
    fn validate_nl_plan_rejects_step_cap_exceeded() {
        let json = r#"{"loop":[
            {"action":"fs_write","path":"a.txt","content":"x"},
            {"action":"verify_contains","path":"a.txt","text":"x"},
            {"action":"python_lint","source":"def ok():\n    return 1\n"},
            {"action":"fs_write","path":"b.txt","content":"y"},
            {"action":"done"}
        ]}"#;
        let err = validate_nl_plan(json, 4).unwrap_err();
        assert!(matches!(err, NlPlanError::ExceedsMaxIterations { .. }));
    }

    #[test]
    fn validate_nl_plan_rejects_empty_fs_write_path() {
        let json = r#"{"loop":[
            {"action":"fs_write","path":"","content":"x"},
            {"action":"done"}
        ]}"#;
        let err = validate_nl_plan(json, 6).unwrap_err();
        assert!(matches!(err, NlPlanError::ForbiddenPattern { .. }));
    }
}
