//! NL → structured tool plan for LOOP-02 (Slice 6.6).
//!
//! Plans are validated and executed by the same `ReActLoopEngine` verify shell as LOOP-01.
//! Production validation checks schema, allowed tools, step cap, and forbidden patterns —
//! **not** harness gold tool order (see `validate_nl_plan_gold_trajectory` for eval-only asserts).

use crate::cost::ProviderTokenUsage;
use crate::graph_extract::strip_json_fence;
use crate::{ModelRouter, ToolInvocation};
use serde_json::Value;
use thiserror::Error;

/// Frozen LOOP-02 eval goal — natural language, not JSON inject.
pub const LOOP02_EVAL_PROMPT: &str = "Write a file named loop02_marker.txt containing exactly LOOP-02-verified. \
Verify loop02_marker.txt contains LOOP-02-verified. Lint this Python source: def ok():\n    return 1\n. \
Then finish the plan.";

/// Gold tool trajectory for LOOP-02 harness asserts only — not enforced in production validation.
pub const LOOP02_GOLD_TOOL_ORDER: &[&str] = &["fs_write", "verify_contains", "python_lint", "done"];

const NL_PLAN_NUM_PREDICT: u32 = 1024;

const ALLOWED_NL_TOOLS: &[&str] = &[
    "fs_write",
    "fs_read",
    "fs_list",
    "verify_contains",
    "python_lint",
    "python_lint_file",
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
    #[error("Plan omits tool `{tool}` explicitly requested by the user goal")]
    MissingRequestedTool { tool: String },
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

/// JSON Schema constraining planner output at decode time (Slice 9.2).
pub const NL_PLAN_SCHEMA: &str = include_str!("../schemas/nl_plan.schema.json");

/// Parsed planner schema for `complete_json_schema`.
pub fn nl_plan_schema() -> Value {
    serde_json::from_str(NL_PLAN_SCHEMA).expect("nl_plan.schema.json is valid JSON")
}

/// Build the bounded planner prompt (Slice 9.1).
///
/// Deliberately neutral: it documents every action's required fields instead of showing one
/// worked example, because a single example teaches small models to reproduce that shape
/// regardless of the goal. Verification is described as conditional on writing, not mandatory.
pub fn build_nl_plan_prompt(nl_goal: &str) -> String {
    format!(
        r#"You convert a user goal into a JSON tool plan for AetherForge to execute.
Return ONLY valid JSON, no prose and no markdown fences, shaped as {{"loop":[ ...steps... ]}}.

Each step is an object with an "action" field. Use only these actions, with exactly these fields:

- {{"action":"fs_read","path":"<relative path>","offset":<int>,"limit":<int>}}
    Read a file. Use this for goals that only inspect or summarise existing files. "offset" and
    "limit" are optional character counts — omit both to read from the start. A long file comes
    back cut, with a marker naming its true size and how much was omitted.
- {{"action":"fs_list","path":"<relative directory>"}}
    List one directory of the workspace. "path" is optional: omit it to list the workspace root.
    Directories come back with a trailing "/". Use this instead of guessing a path.
- {{"action":"fs_write","path":"<relative path>","content":"<full file contents>"}}
    Create or overwrite a file. "content" must be the complete text and must not be empty.
- {{"action":"verify_contains","path":"<relative path>","text":"<exact substring>"}}
    Confirm a file contains a substring. "text" must be a non-empty string you expect to find.
- {{"action":"python_lint","source":"<python source code>"}}
    Syntax-check Python source given in the goal. Copy the source verbatim.
- {{"action":"python_lint_file","path":"<relative path>"}}
    Syntax-check a Python file that an earlier fs_write in this plan put on disk. This is how you
    certify the file you actually wrote; python_lint only checks source quoted in the goal and
    proves nothing about any artifact.
- {{"action":"git_init","branch":"<branch name>"}}
    Initialise a git repository. Use "main" when the goal does not name a branch.
- {{"action":"mcp_call","server":"<server name>","tool":"<tool name>","args":{{}}}}
    Call a tool on a connected MCP server.
- {{"action":"skill_execute","skill_id":"<skill id>","variables":{{}}}}
    Run a named procedural skill.
- {{"action":"done"}}
    Finish. Always the final step.

Rules:
- Include only the steps the goal actually requires. Do not add steps the goal did not ask for.
- Preserve the order in which the user requests operations. Do not move a later operation before
  an earlier one.
- If the goal only reads, opens, inspects, or summarises an existing file, use fs_read then done.
- If the goal refers to files without naming a path, or you need to know what exists before you can
  act, use fs_list first and then act on a name you actually saw. Never guess a path: a plan built
  on an invented path fails in a way that looks like a tool bug.
- If an fs_read comes back with a truncation marker and the goal needs text that was not shown,
  issue another fs_read on the same path with "offset"/"limit" to page into it. Never report
  content as absent when a marker said the read was cut.
- If the goal initialises version control, use git_init then done.
- If the goal only asks to lint Python, use python_lint then done.
- If the goal explicitly names a skill, use skill_execute with that skill id then done.
- If the goal explicitly asks for an MCP server/tool, use mcp_call then done. Do not add fs_read
  merely because the MCP tool may inspect files.
- If the goal writes a file, use fs_write, optionally verify_contains, then done.
- Add verify_contains ONLY after an fs_write whose content you can confirm, and only with a
  non-empty "text" you just wrote. Never emit verify_contains after fs_read.
- If the goal writes a Python file (a path ending in .py), follow that fs_write with BOTH a
  verify_contains on that same path and a python_lint_file on that same path. A .py write that was
  never linted as a file is rejected before done, and python_lint does not count as that lint.
- Every field listed for an action is required. Never emit an empty string for a required field.
- Do not repeat the same action on the same target twice in a row.
- Keep every path relative to the workspace. Never use absolute paths or "..".
- The last step is always {{"action":"done"}}.

User goal:
{nl_goal}"#
    )
}

/// Build the repair prompt after a rejected plan (Slice 9.3).
///
/// The validation error is fed back as structured feedback so the model can correct the specific
/// defect, rather than resampling blind.
pub fn build_nl_repair_prompt(nl_goal: &str, previous: &str, error: &NlPlanError) -> String {
    let correction = match error {
        NlPlanError::MissingRequestedTool { tool } => format!(
            "The corrected plan MUST contain an action named {tool:?}. Preserve the other \
             requested steps and add exactly that missing action with every required field. \
             Copy literal content or source text from the user goal; do not substitute another tool."
        ),
        NlPlanError::InvalidStep { index, detail } => format!(
            "Step {index} has invalid arguments: {detail}. Preserve its intended action and add \
             every field required by that action's contract."
        ),
        NlPlanError::ForbiddenPattern { index, detail } => format!(
            "Fix the forbidden pattern at step {index}: {detail}. Do not remove other operations \
             explicitly requested by the user."
        ),
        _ => "Correct the rejected plan while preserving every operation explicitly requested by \
              the user."
            .into(),
    };
    format!(
        r#"{base}

Your previous answer was rejected.

Previous answer:
{previous}

Rejection reason:
{error}

Required correction:
{correction}

Emit a corrected plan that fixes exactly this problem. Return ONLY the JSON object."#,
        base = build_nl_plan_prompt(nl_goal),
        previous = previous.trim(),
        error = error,
        correction = correction,
    )
}

/// Parse and validate planner JSON into tool invocations (production path).
pub fn validate_nl_plan(
    json: &str,
    max_iterations: usize,
) -> Result<Vec<ToolInvocation>, NlPlanError> {
    let value: Value = serde_json::from_str(json).map_err(|e| NlPlanError::Json(e.to_string()))?;

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
        let invocation: ToolInvocation =
            serde_json::from_value(step.clone()).map_err(|e| NlPlanError::InvalidStep {
                index,
                detail: e.to_string(),
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

/// Normalize only mechanically safe planner defects before semantic validation.
///
/// Small local models occasionally omit the terminal `done` step or repeat an identical JSON
/// step. Neither defect requires model judgment to repair. All other changes—including filling
/// tool arguments or changing action order—remain the bounded LLM repair loop's responsibility.
pub fn normalize_nl_plan_json(json: &str) -> Result<String, NlPlanError> {
    let mut value: Value =
        serde_json::from_str(json).map_err(|e| NlPlanError::Json(e.to_string()))?;
    let steps = value
        .get_mut("loop")
        .and_then(Value::as_array_mut)
        .ok_or(NlPlanError::MissingLoopArray)?;

    steps.dedup();

    let ends_with_done = steps
        .last()
        .and_then(|step| step.get("action"))
        .and_then(Value::as_str)
        == Some("done");
    if !ends_with_done {
        steps.push(serde_json::json!({"action": "done"}));
    }

    serde_json::to_string(&value).map_err(|e| NlPlanError::Json(e.to_string()))
}

/// Ensure a plan covers tool intent stated explicitly in the goal.
///
/// This is deliberately not a gold trajectory: it neither imposes order nor invents tools. It
/// catches only direct user language ("lint", "MCP", "skill", "version control", etc.) so an
/// otherwise valid JSON plan cannot silently drop a requested operation.
pub fn validate_goal_coverage(
    nl_goal: &str,
    plan: &[ToolInvocation],
) -> Result<(), NlPlanError> {
    let goal = nl_goal.to_ascii_lowercase();
    // Each entry is a set of *alternative* action names that satisfy one piece of stated intent.
    // Linting is the only intent with two spellings today: `python_lint` checks source quoted in
    // the goal, `python_lint_file` checks an artifact the plan wrote (CHECK-02). A goal that says
    // "lint" is satisfied by either, so requiring the literal name `python_lint` would reject a
    // correct plan that lints the file it just wrote.
    let mut required: Vec<&[&str]> = Vec::new();
    if goal.contains("write ") || goal.contains("create ") {
        required.push(&["fs_write"]);
    }
    if goal.contains("read ") || goal.contains("open ") {
        required.push(&["fs_read"]);
    }
    // P0-2: a goal that asks what exists requires the action that can answer it. Without this the
    // planner could satisfy "list the files" with a guess-shaped fs_read and still pass coverage.
    if goal.contains("list ") || goal.contains("what files") || goal.contains("which files") {
        required.push(&["fs_list"]);
    }
    if goal.contains("verify ") || goal.contains("confirm ") {
        required.push(&["verify_contains"]);
    }
    if goal.contains("lint ") || goal.contains("python source") {
        required.push(&["python_lint", "python_lint_file"]);
    }
    if goal.contains("git ") || goal.contains("repository") || goal.contains("version control") {
        required.push(&["git_init"]);
    }
    if goal.contains("mcp ") || goal.contains("mcp server") {
        required.push(&["mcp_call"]);
    }
    if goal.contains(" skill") || goal.starts_with("skill ") {
        required.push(&["skill_execute"]);
    }

    for alternatives in required {
        if !plan
            .iter()
            .any(|step| alternatives.contains(&tool_name(step)))
        {
            return Err(NlPlanError::MissingRequestedTool {
                tool: alternatives[0].into(),
            });
        }
    }
    Ok(())
}

/// Harness-only: assert tool order matches LOOP-02 gold trajectory.
pub fn validate_nl_plan_gold_trajectory(plan: &[ToolInvocation]) -> Result<(), NlPlanError> {
    for (index, expected) in LOOP02_GOLD_TOOL_ORDER.iter().enumerate() {
        let actual =
            plan.get(index)
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

#[derive(Debug, Clone)]
pub struct NlPlannerResult { pub plan: Vec<ToolInvocation>, pub token_usage: ProviderTokenUsage, }

/// Total planner attempts: one initial generation plus `MAX_PLAN_REPAIRS` repairs (Slice 9.3).
const MAX_PLAN_REPAIRS: usize = 2;

/// Call the router with a schema-constrained request, validate, and repair on rejection.
///
/// Slice 9.1–9.3: neutral prompt, decode-time schema constraint, and a bounded repair loop that
/// feeds the validation error back to the model. A validation error is a signal to act on, not a
/// terminal condition.
pub async fn run_nl_planner(
    router: &ModelRouter,
    nl_goal: &str,
    max_iterations: usize,
    context: Option<&PlannerContext>,
 ) -> Result<NlPlannerResult, NlPlanError> {
    let schema = nl_plan_schema();
    let mut prompt = with_capability_context(build_nl_plan_prompt(nl_goal), context);
    let mut last_error: Option<NlPlanError> = None;
    let mut total_usage = ProviderTokenUsage::default();

    for attempt in 0..=MAX_PLAN_REPAIRS {
        let completion = router.complete_json_schema(&prompt, NL_PLAN_NUM_PREDICT, &schema).await.map_err(|e| NlPlanError::Ollama(e.to_string()))?;
        if let Some(u) = completion.token_usage { total_usage.merge(u); }
        let raw = completion.content;
        let json = strip_json_fence(&raw);
        let normalized = match normalize_nl_plan_json(&json) {
            Ok(value) => value,
            Err(e) => {
                if attempt < MAX_PLAN_REPAIRS {
                    prompt = with_capability_context(
                        build_nl_repair_prompt(nl_goal, &json, &e),
                        context,
                    );
                }
                last_error = Some(e);
                continue;
            }
        };

        match validate_nl_plan(&normalized, max_iterations)
            .and_then(|plan| {
                validate_goal_coverage(nl_goal, &plan)?;
                Ok(plan)
            })
        {
            Ok(plan) => return Ok(NlPlannerResult { plan, token_usage: total_usage }),
            Err(e) => {
                if attempt < MAX_PLAN_REPAIRS {
                    prompt = with_capability_context(
                        build_nl_repair_prompt(nl_goal, &normalized, &e),
                        context,
                    );
                }
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or(NlPlanError::EmptyPlan))
}

/// Build a replan prompt after a step failed verification mid-execution (Phase 9 slice 9.9-9.10 /
/// LOOP-04). Distinct from [`build_nl_repair_prompt`]: that one fixes a rejected *plan* before any
/// tool ran; this one fixes a plan whose execution already ran partway and hit a real tool
/// failure, so the model must continue from the current state rather than start over.
///
/// `remedy` and `constraint` come from [`crate::ToolError`]: the classification of this failure
/// into what would make it succeed, plus the machine-readable values the retry needs (the path, the
/// expected substring, the connected-server inventory). Carrying them here is what makes a rejection
/// repair-enabling rather than a dead end — the model corrects against the actual constraint instead
/// of resampling blind (P1-9).
pub fn build_nl_verify_repair_prompt(
    nl_goal: &str,
    completed_tools: &[String],
    failed_tool: &str,
    failure_detail: &str,
    remedy: &str,
    constraint: Option<&Value>,
) -> String {
    let constraint_line = match constraint {
        Some(value) => format!("\nConstraint values for the retry: {value}"),
        None => String::new(),
    };
    format!(
        r#"{base}

Execution already started and is NOT starting over. These steps already ran successfully, in
order: {completed:?}. The next step, "{failed_tool}", FAILED verification with this detail:
{failure_detail}

What would make it succeed: {remedy}{constraint_line}

Produce a corrected JSON plan for ONLY the remaining work needed to reach the original goal above,
given what already happened. Do not repeat the already-completed steps. Fix whatever caused the
failure — for example, rewrite the file with the correct content, or verify the substring that is
actually present — then finish with {{"action":"done"}}."#,
        base = build_nl_plan_prompt(nl_goal),
        completed = completed_tools,
        failed_tool = failed_tool,
        failure_detail = failure_detail,
        remedy = remedy,
        constraint_line = constraint_line,
    )
}

/// Replan after a verify failure mid-execution (Phase 9 slice 9.9-9.10 / LOOP-04).
///
/// Structurally identical decode-and-repair loop to [`run_nl_planner`], but validates only
/// structural correctness (`validate_nl_plan`) rather than [`validate_goal_coverage`] against the
/// full original goal: a remediation plan is inherently partial by design (it does not repeat
/// already-completed steps), so checking it against the complete original goal's keyword
/// requirements would reject correct, minimal fixes.
pub async fn run_nl_planner_repair(
    router: &ModelRouter,
    nl_goal: &str,
    completed_tools: &[String],
    failed_tool: &str,
    failure_detail: &str,
    remedy: &str,
    constraint: Option<&Value>,
    max_iterations: usize,
    context: Option<&PlannerContext>,
 ) -> Result<NlPlannerResult, NlPlanError> {
    let schema = nl_plan_schema();
    let mut prompt = with_capability_context(
        build_nl_verify_repair_prompt(
            nl_goal,
            completed_tools,
            failed_tool,
            failure_detail,
            remedy,
            constraint,
        ),
        context,
    );
    let mut last_error: Option<NlPlanError> = None;
    let mut total_usage = ProviderTokenUsage::default();

    for attempt in 0..=MAX_PLAN_REPAIRS {
        let completion = router.complete_json_schema(&prompt, NL_PLAN_NUM_PREDICT, &schema).await.map_err(|e| NlPlanError::Ollama(e.to_string()))?;
        if let Some(u) = completion.token_usage { total_usage.merge(u); }
        let raw = completion.content;
        let json = strip_json_fence(&raw);
        let normalized = match normalize_nl_plan_json(&json) {
            Ok(value) => value,
            Err(e) => {
                if attempt < MAX_PLAN_REPAIRS {
                    prompt = with_capability_context(
                        build_nl_repair_prompt(nl_goal, &json, &e),
                        context,
                    );
                }
                last_error = Some(e);
                continue;
            }
        };

        match validate_nl_plan(&normalized, max_iterations) {
            Ok(plan) => return Ok(NlPlannerResult { plan, token_usage: total_usage }),
            Err(e) => {
                if attempt < MAX_PLAN_REPAIRS {
                    prompt = with_capability_context(
                        build_nl_repair_prompt(nl_goal, &normalized, &e),
                        context,
                    );
                }
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or(NlPlanError::EmptyPlan))
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
        ToolInvocation::FsRead { path, offset, limit } => {
            if path.trim().is_empty() {
                return Some("fs_read requires non-empty path".into());
            }
            if let (Some(offset), Some(limit)) = (offset, limit) {
                if limit == &0 {
                    return Some("fs_read limit must be greater than 0".into());
                }
                // Both are usize, so only the pairing needs a sanity check; an offset past the end
                // is reported by the read itself, with the file's real size (P1-6).
                let _ = offset;
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
        ToolInvocation::PythonLintFile { path } if path.trim().is_empty() => {
            return Some("python_lint_file requires non-empty path".into());
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

/// Action name for a plan step — shared by validation, harness asserts, and telemetry.
pub fn plan_tool_name(step: &ToolInvocation) -> &'static str {
    tool_name(step)
}

fn tool_name(step: &ToolInvocation) -> &'static str {
    match step {
        ToolInvocation::FsWrite { .. } => "fs_write",
        ToolInvocation::FsRead { .. } => "fs_read",
        ToolInvocation::FsList { .. } => "fs_list",
        ToolInvocation::PythonLint { .. } => "python_lint",
        ToolInvocation::PythonLintFile { .. } => "python_lint_file",
        ToolInvocation::GitInit { .. } => "git_init",
        ToolInvocation::McpCall { .. } => "mcp_call",
        ToolInvocation::SkillExecute { .. } => "skill_execute",
        ToolInvocation::VerifyContains { .. } => "verify_contains",
        ToolInvocation::SubagentTask { .. } => "subagent_task",
        ToolInvocation::Done => "done",
    }
}

fn tool_target_key(step: &ToolInvocation) -> String {
    match step {
        ToolInvocation::FsWrite { path, .. } => path.clone(),
        ToolInvocation::FsRead { path, .. } => path.clone(),
        ToolInvocation::FsList { path } => path.clone().unwrap_or_else(|| ".".to_string()),
        ToolInvocation::VerifyContains { path, .. } => path.clone(),
        ToolInvocation::PythonLint { source } => source.chars().take(32).collect(),
        ToolInvocation::PythonLintFile { path } => path.clone(),
        ToolInvocation::GitInit { branch } => branch.clone(),
        ToolInvocation::McpCall { server, tool, .. } => format!("{}:{}", server, tool),
        ToolInvocation::SkillExecute { skill_id, .. } => skill_id.clone(),
        ToolInvocation::SubagentTask { paths } => paths.join(","),
        ToolInvocation::Done => "done".into(),
    }
}

/// What the planner may assume exists right now (P0-2 / PLAN-02).
///
/// A planner that is not told what is connected invents MCP servers; one that cannot see the
/// workspace invents paths. Both produce failures that look like tool bugs and burn a replan. This
/// is the *push* half of discovery — `fs_list` is the pull half.
///
/// Volatile by design: it is appended **after** [`build_nl_plan_prompt`], which is the stable prefix
/// the prefix cache measures (CACHE-01), so a changed workspace cannot invalidate a cached prefix.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannerContext {
    /// MCP servers connected and pinned right now.
    pub mcp_servers: Vec<String>,
    /// Skill ids installed right now.
    pub skills: Vec<String>,
    /// Top-level workspace entries, directories suffixed with `/`.
    pub workspace_entries: Vec<String>,
}

/// Most workspace entries named to the planner. The rest are discoverable with `fs_list`, which is
/// the point: the context exists to stop guessing, not to replace looking.
pub const PLANNER_CONTEXT_MAX_ENTRIES: usize = 40;

/// Render the capability block appended to a planner prompt.
///
/// Absence is stated, never omitted. A section that is simply missing reads as "unknown", and
/// "unknown" is what a model fills with an invention; "none connected" is a fact it can plan around.
pub fn build_capability_context(context: &PlannerContext) -> String {
    let mut out = String::from(
        "\n\nWhat is actually available right now — do not assume anything else exists:\n",
    );
    out.push_str(&format!(
        "- MCP servers connected: {}\n",
        list_or_none(&context.mcp_servers)
    ));
    out.push_str(&format!(
        "- Skills installed: {}\n",
        list_or_none(&context.skills)
    ));
    let entries: Vec<&String> = context
        .workspace_entries
        .iter()
        .take(PLANNER_CONTEXT_MAX_ENTRIES)
        .collect();
    let hidden = context.workspace_entries.len() - entries.len();
    out.push_str(&format!(
        "- Workspace top level: {}{}\n",
        if entries.is_empty() {
            "empty — nothing has been written here yet".to_string()
        } else {
            entries
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .join(", ")
        },
        if hidden > 0 {
            format!(" (+{hidden} more; use fs_list to see them)")
        } else {
            String::new()
        }
    ));
    out.push_str(
        "- To look inside a directory use {\"action\":\"fs_list\",\"path\":\"<relative dir>\"}; \
         to read a file use fs_read. If a path you want is not listed above and you have not listed \
         its directory, list first rather than guessing.\n",
    );
    if context.mcp_servers.is_empty() {
        out.push_str(
            "- No MCP server is connected, so any mcp_call step fails immediately with the \
             connected inventory in its remedy. Do not emit one.\n",
        );
    }
    if context.skills.is_empty() {
        out.push_str(
            "- No skill is installed, so any skill_execute step fails immediately. Do not emit one.\n",
        );
    }
    out
}

/// Append the volatile capability block to a planner prompt, or return it unchanged when the caller
/// has no inventory to declare (`None` means "unknown", which is not the same as "none").
fn with_capability_context(prompt: String, context: Option<&PlannerContext>) -> String {
    match context {
        Some(context) => format!("{prompt}{}", build_capability_context(context)),
        None => prompt,
    }
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        return "none".to_string();
    }
    items.join(", ")
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

    #[test]
    fn normalize_adds_terminal_done_without_changing_actions() {
        let json = r#"{"loop":[{"action":"fs_read","path":"notes.txt"}]}"#;
        let normalized = normalize_nl_plan_json(json).unwrap();
        let plan = validate_nl_plan(&normalized, 6).unwrap();
        assert_eq!(plan.len(), 2);
        assert!(matches!(plan[0], ToolInvocation::FsRead { .. }));
        assert!(matches!(plan[1], ToolInvocation::Done));
    }

    #[test]
    fn normalize_removes_only_adjacent_identical_steps() {
        let json = r#"{"loop":[
            {"action":"verify_contains","path":"a.txt","text":"x"},
            {"action":"verify_contains","path":"a.txt","text":"x"},
            {"action":"done"}
        ]}"#;
        let normalized = normalize_nl_plan_json(json).unwrap();
        let plan = validate_nl_plan(&normalized, 6).unwrap();
        assert_eq!(plan.len(), 2);
        assert!(matches!(plan[0], ToolInvocation::VerifyContains { .. }));
    }

    #[test]
    fn planner_schema_requires_action_specific_fields() {
        let schema = nl_plan_schema();
        let variants = schema
            .pointer("/properties/loop/items/anyOf")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(variants.len(), ALLOWED_NL_TOOLS.len());
        let fs_write_required = variants[0]
            .get("required")
            .and_then(Value::as_array)
            .unwrap();
        assert!(fs_write_required.iter().any(|value| value == "content"));
    }

    #[test]
    fn repair_prompt_contains_rejection_and_previous_answer() {
        let error = NlPlanError::ForbiddenPattern {
            index: 0,
            detail: "missing done".into(),
        };
        let prompt = build_nl_repair_prompt(
            "Read notes.txt",
            r#"{"loop":[{"action":"fs_read","path":"notes.txt"}]}"#,
            &error,
        );
        assert!(prompt.contains("Previous answer:"));
        assert!(prompt.contains("missing done"));
        assert!(prompt.contains("Read notes.txt"));
    }

    #[test]
    fn goal_coverage_requires_explicitly_requested_tools_without_ordering() {
        let plan = validate_nl_plan(
            r#"{"loop":[
                {"action":"fs_write","path":"a.py","content":"def ok():\n    return 1\n"},
                {"action":"done"}
            ]}"#,
            6,
        )
        .unwrap();
        let err = validate_goal_coverage(
            "Write a.py, then lint this Python source: def ok(): return 1",
            &plan,
        )
        .unwrap_err();
        assert_eq!(
            err,
            NlPlanError::MissingRequestedTool {
                tool: "python_lint".into()
            }
        );
    }

    #[test]
    fn goal_coverage_accepts_requested_tools_in_any_order() {
        let plan = validate_nl_plan(
            r#"{"loop":[
                {"action":"python_lint","source":"def ok():\n    return 1\n"},
                {"action":"fs_write","path":"a.py","content":"def ok():\n    return 1\n"},
                {"action":"done"}
            ]}"#,
            6,
        )
        .unwrap();
        validate_goal_coverage("Write a.py and lint this Python source", &plan).unwrap();
    }
}
