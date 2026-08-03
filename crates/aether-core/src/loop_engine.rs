use crate::{GitOps, LoopError, PythonLinter};
use aether_mcp::{invoke_with_grant, McpAllowlist};
use aether_permissions::{path_is_subpath, PermissionDecision, PermissionManager};
use aether_sandbox::ProductionSandbox;
use aether_skills::{SkillDefinition, SkillExecutor};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Default token cap for daemon loop tasks (`0` = unlimited).
pub const DEFAULT_MAX_LOOP_TOKENS: usize = 0;

#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub max_iterations: usize,
    /// Token budget cap (`0` = unlimited).
    pub max_tokens: usize,
    /// Running token tally; updated by `ReActLoopEngine` during execution.
    pub tokens_used: usize,
    pub session_id: String,
    pub workspace: PathBuf,
}

impl LoopConfig {
    pub fn new(max_iterations: usize, session_id: String, workspace: PathBuf) -> Self {
        Self {
            max_iterations,
            max_tokens: DEFAULT_MAX_LOOP_TOKENS,
            tokens_used: 0,
            session_id,
            workspace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolObservation {
    pub iteration: usize,
    pub tool: String,
    pub success: bool,
    pub output: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ToolInvocation {
    FsWrite {
        path: String,
        content: String,
    },
    FsRead {
        path: String,
    },
    PythonLint {
        source: String,
    },
    GitInit {
        /// Defaults to `main` when a small local planner omits the branch.
        #[serde(default = "default_git_branch")]
        branch: String,
    },
    McpCall {
        server: String,
        tool: String,
        #[serde(default)]
        args: Value,
    },
    SkillExecute {
        skill_id: String,
        #[serde(default)]
        variables: HashMap<String, String>,
    },
    VerifyContains {
        path: String,
        text: String,
    },
    Done,
}

fn default_git_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone)]
pub struct LoopRunResult {
    pub iterations: usize,
    pub tokens_used: usize,
    pub observations: Vec<ToolObservation>,
    pub summary: String,
    pub done: bool,
}

#[derive(Debug, Clone)]
pub enum LoopStreamEvent {
    Plan {
        iteration: usize,
        action: String,
    },
    Tool {
        iteration: usize,
        tool: String,
        output: String,
    },
    Observe {
        iteration: usize,
        summary: String,
    },
    Verify {
        iteration: usize,
        passed: bool,
        detail: String,
    },
    Done {
        iterations: usize,
        summary: String,
        tokens_used: usize,
    },
    Budget {
        iteration: usize,
        max_iterations: usize,
        tokens_used: usize,
        max_tokens: usize,
    },
    Error {
        message: String,
    },
}

pub trait Verifier {
    fn verify(&self, observations: &[ToolObservation]) -> Result<(), String>;
}

/// CODE-01 style verifier: last python_lint observation must be clean.
pub struct PythonLintVerifier;

impl Verifier for PythonLintVerifier {
    fn verify(&self, observations: &[ToolObservation]) -> Result<(), String> {
        let lint = observations
            .iter()
            .rev()
            .find(|o| o.tool == "python_lint")
            .ok_or_else(|| "No python_lint observation to verify".to_string())?;

        if lint.success {
            Ok(())
        } else {
            Err(format!("Python lint failed: {}", lint.output))
        }
    }
}

pub trait StopHook {
    fn should_stop(&self, iteration: usize, observations: &[ToolObservation], plan_done: bool) -> bool;
}

pub struct GoalStopHook {
    pub marker: String,
}

impl StopHook for GoalStopHook {
    fn should_stop(&self, _iteration: usize, observations: &[ToolObservation], plan_done: bool) -> bool {
        if plan_done {
            return true;
        }
        observations.iter().any(|o| o.success && o.output.contains(&self.marker))
    }
}

pub struct MaxIterationStopHook;

impl StopHook for MaxIterationStopHook {
    fn should_stop(&self, iteration: usize, _observations: &[ToolObservation], plan_done: bool) -> bool {
        plan_done || iteration == 0
    }
}

pub struct ToolRegistry;

impl ToolRegistry {
    pub fn execute(
        conn: &Connection,
        config: &LoopConfig,
        allowlist: Option<&McpAllowlist>,
        skills: &HashMap<String, SkillDefinition>,
        iteration: usize,
        invocation: &ToolInvocation,
    ) -> Result<ToolObservation, String> {
        match invocation {
            ToolInvocation::FsWrite { path, content } => {
                let full = resolve_workspace_path(&config.workspace, path)?;
                let full_str = full.to_string_lossy().to_string();
                if let crate::HookDecision::Deny(reason) = crate::pre_tool_use_path_check(&full) {
                    return Err(reason);
                }
                let decision = PermissionManager::check_file_access(
                    conn,
                    &config.session_id,
                    &full_str,
                    "write",
                )
                .map_err(|e| e.to_string())?;
                if decision != PermissionDecision::Approved {
                    return Err(format!("Write denied for target path {}", full_str));
                }
                aether_permissions::journal_file_write(
                    conn,
                    &config.session_id,
                    &config.workspace,
                    &full,
                    content,
                )
                .map_err(|e| e.to_string())?;
                Ok(observation(
                    iteration,
                    "fs_write",
                    true,
                    format!("Wrote {} bytes to {}", content.len(), path),
                ))
            }
            ToolInvocation::FsRead { path } => {
                let full = resolve_workspace_path(&config.workspace, path)?;
                let full_str = full.to_string_lossy().to_string();
                if let crate::HookDecision::Deny(reason) = crate::pre_tool_use_path_check(&full) {
                    return Err(reason);
                }
                let decision = PermissionManager::check_file_access(
                    conn,
                    &config.session_id,
                    &full_str,
                    "read",
                )
                .map_err(|e| e.to_string())?;
                if decision != PermissionDecision::Approved {
                    return Err(format!("Read denied for {}", full_str));
                }
                let content = ProductionSandbox::read_to_string(&config.workspace, &full)
                    .map_err(|e| e.to_string())?;
                Ok(observation(
                    iteration,
                    "fs_read",
                    true,
                    content.chars().take(500).collect(),
                ))
            }
            ToolInvocation::PythonLint { source } => {
                match PythonLinter::check_syntax_in_workspace(source, &config.workspace) {
                    Ok(issues) if issues.is_empty() => Ok(observation(
                        iteration,
                        "python_lint",
                        true,
                        "syntax OK".into(),
                    )),
                    Ok(issues) => Ok(observation(
                        iteration,
                        "python_lint",
                        false,
                        format!("{} issue(s): {:?}", issues.len(), issues),
                    )),
                    Err(e) => Ok(observation(
                        iteration,
                        "python_lint",
                        false,
                        e.to_string(),
                    )),
                }
            }
            ToolInvocation::GitInit { branch } => {
                match GitOps::init_commit_and_branch(
                    conn,
                    &config.session_id,
                    &config.workspace,
                    branch,
                ) {
                    Ok(()) => {
                        // Best-effort bookkeeping: git already succeeded, so a journal failure
                        // here must not fail the tool call. It only means `undo_pending_writes`
                        // will not be able to report this git_init as a known non-undoable step.
                        let _ = aether_permissions::journal_git_init(
                            conn,
                            &config.session_id,
                            &config.workspace,
                            branch,
                        );
                        Ok(observation(
                            iteration,
                            "git_init",
                            true,
                            format!("Initialized repo on branch {}", branch),
                        ))
                    }
                    Err(e) => Ok(observation(
                        iteration,
                        "git_init",
                        false,
                        e.to_string(),
                    )),
                }
            }
            ToolInvocation::McpCall { server, tool, args } => {
                let allowlist = allowlist
                    .ok_or_else(|| "MCP allowlist not configured".to_string())?;
                validate_mcp_arguments_in_workspace(&config.workspace, args)?;
                let workspace_str = config.workspace.to_string_lossy().to_string();
                let extra = vec![workspace_str.clone()];
                match invoke_with_grant(
                    conn,
                    &config.session_id,
                    &workspace_str,
                    allowlist,
                    server,
                    tool,
                    args.clone(),
                    &extra,
                ) {
                    Ok((result, audit)) => Ok(observation(
                        iteration,
                        "mcp_call",
                        true,
                        format!(
                            "tools_hash={} result={}",
                            audit.tools_hash,
                            result.to_string().chars().take(200).collect::<String>()
                        ),
                    )),
                    Err(e) => Ok(observation(iteration, "mcp_call", false, e.to_string())),
                }
            }
            ToolInvocation::SkillExecute { skill_id, variables } => {
                let skill = skills
                    .get(skill_id)
                    .ok_or_else(|| format!("Unknown skill_id {}", skill_id))?;
                match SkillExecutor::execute(
                    conn,
                    &config.session_id,
                    skill,
                    &config.workspace,
                    variables,
                ) {
                    Ok(()) => Ok(observation(
                        iteration,
                        "skill_execute",
                        true,
                        format!("Executed skill {}", skill.name),
                    )),
                    Err(e) => Ok(observation(
                        iteration,
                        "skill_execute",
                        false,
                        e.to_string(),
                    )),
                }
            }
            ToolInvocation::VerifyContains { path, text } => {
                let full = resolve_workspace_path(&config.workspace, path)?;
                let content = ProductionSandbox::read_to_string(&config.workspace, &full)
                    .map_err(|e| e.to_string())?;
                let ok = content.contains(text);
                Ok(observation(
                    iteration,
                    "verify_contains",
                    ok,
                    if ok {
                        format!("Found {:?} in {}", text, path)
                    } else {
                        format!("Missing {:?} in {}", text, path)
                    },
                ))
            }
            ToolInvocation::Done => Ok(observation(iteration, "done", true, "plan complete".into())),
        }
    }
}

pub struct ReActLoopEngine {
    pub max_iterations: usize,
}

impl ReActLoopEngine {
    pub fn new(max_iterations: usize) -> Self {
        Self { max_iterations }
    }

    /// Parse a structured loop plan from prompt JSON: `{"loop":[...]}`.
    pub fn parse_plan_from_prompt(prompt: &str) -> Option<Vec<ToolInvocation>> {
        let trimmed = prompt.trim();
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            if let Some(steps) = value.get("loop").and_then(|v| v.as_array()) {
                let mut plan = Vec::new();
                for step in steps {
                    if let Ok(inv) = serde_json::from_value(step.clone()) {
                        plan.push(inv);
                    } else {
                        return None;
                    }
                }
                if !plan.is_empty() {
                    return Some(plan);
                }
            }
        }
        None
    }

    pub fn run_structured<F>(
        &self,
        conn: &Connection,
        config: &mut LoopConfig,
        plan: Vec<ToolInvocation>,
        allowlist: Option<&McpAllowlist>,
        skills: &HashMap<String, SkillDefinition>,
        mut on_event: F,
    ) -> Result<LoopRunResult, LoopError>
    where
        F: FnMut(LoopStreamEvent),
    {
        let mut observations = Vec::new();
        let mut iteration = 0usize;
        let mut pending_writes: Vec<String> = Vec::new();

        for step in plan {
            if iteration >= self.max_iterations {
                let msg = format!("Max iterations ({}) exceeded", self.max_iterations);
                on_event(LoopStreamEvent::Error {
                    message: msg.clone(),
                });
                return Err(LoopError::MaxIterations(self.max_iterations));
            }

            iteration += 1;
            let action = tool_name(&step);
            on_event(LoopStreamEvent::Plan {
                iteration,
                action: action.to_string(),
            });

            config.tokens_used = config
                .tokens_used
                .saturating_add(estimate_invocation_tokens(&step));
            emit_budget_telemetry(config, iteration, &mut on_event);
            if let Err(err) = check_token_budget(conn, config, iteration, &mut on_event) {
                return Err(err);
            }

            if matches!(step, ToolInvocation::Done) {
                if let Err(msg) = require_verified_writes(&pending_writes) {
                    on_event(LoopStreamEvent::Error {
                        message: msg.clone(),
                    });
                    return Err(LoopError::Turn(msg));
                }
                if let Err(msg) = verify_shell_before_done(&observations) {
                    on_event(LoopStreamEvent::Error {
                        message: msg.clone(),
                    });
                    return Err(LoopError::Turn(msg));
                }
                let obs = ToolRegistry::execute(
                    conn,
                    config,
                    allowlist,
                    skills,
                    iteration,
                    &step,
                )
                .map_err(LoopError::Turn)?;
                config.tokens_used = config
                    .tokens_used
                    .saturating_add(estimate_tokens(&obs.output));
                emit_budget_telemetry(config, iteration, &mut on_event);
                if let Err(err) = check_token_budget(conn, config, iteration, &mut on_event) {
                    return Err(err);
                }
                observations.push(obs.clone());
                on_event(LoopStreamEvent::Tool {
                    iteration,
                    tool: obs.tool.clone(),
                    output: obs.output.clone(),
                });
                on_event(LoopStreamEvent::Observe {
                    iteration,
                    summary: obs.output.clone(),
                });
                break;
            }

            let obs = ToolRegistry::execute(conn, config, allowlist, skills, iteration, &step)
                .map_err(LoopError::Turn)?;
            config.tokens_used = config
                .tokens_used
                .saturating_add(estimate_tokens(&obs.output));
            emit_budget_telemetry(config, iteration, &mut on_event);
            if let Err(err) = check_token_budget(conn, config, iteration, &mut on_event) {
                return Err(err);
            }
            on_event(LoopStreamEvent::Tool {
                iteration,
                tool: obs.tool.clone(),
                output: obs.output.clone(),
            });
            on_event(LoopStreamEvent::Observe {
                iteration,
                summary: obs.output.clone(),
            });

            if let ToolInvocation::FsWrite { path, .. } = &step {
                if obs.success {
                    pending_writes.push(path.clone());
                }
            }

            if matches!(step, ToolInvocation::PythonLint { .. }) {
                observations.push(obs.clone());
                let passed = obs.success;
                on_event(LoopStreamEvent::Verify {
                    iteration,
                    passed,
                    detail: obs.output.clone(),
                });
                if !passed {
                    on_event(LoopStreamEvent::Error {
                        message: obs.output.clone(),
                    });
                    return Err(LoopError::VerifyFailed {
                        failed_tool: obs.tool.clone(),
                        detail: obs.output.clone(),
                        iterations_used: iteration,
                        observations,
                    });
                }
                continue;
            }

            if matches!(&step, ToolInvocation::VerifyContains { .. }) {
                if let ToolInvocation::VerifyContains { path, .. } = &step {
                    if obs.success {
                        pending_writes.retain(|p| p != path);
                    }
                }
                observations.push(obs.clone());
                on_event(LoopStreamEvent::Verify {
                    iteration,
                    passed: obs.success,
                    detail: obs.output.clone(),
                });
                if !obs.success {
                    on_event(LoopStreamEvent::Error {
                        message: obs.output.clone(),
                    });
                    return Err(LoopError::VerifyFailed {
                        failed_tool: obs.tool.clone(),
                        detail: obs.output.clone(),
                        iterations_used: iteration,
                        observations,
                    });
                }
                continue;
            }

            observations.push(obs);
        }

        let summary = observations
            .last()
            .map(|o| o.output.clone())
            .unwrap_or_else(|| "loop finished".into());

        on_event(LoopStreamEvent::Done {
            iterations: iteration,
            summary: summary.clone(),
            tokens_used: config.tokens_used,
        });

        Ok(LoopRunResult {
            iterations: iteration,
            tokens_used: config.tokens_used,
            observations,
            summary,
            done: true,
        })
    }

    pub fn run_with_stop_hook<S: StopHook>(
        &self,
        conn: &Connection,
        config: &mut LoopConfig,
        plan: Vec<ToolInvocation>,
        allowlist: Option<&McpAllowlist>,
        skills: &HashMap<String, SkillDefinition>,
        stop_hook: &S,
        mut on_event: impl FnMut(LoopStreamEvent),
    ) -> Result<LoopRunResult, LoopError> {
        let result = self.run_structured(conn, config, plan, allowlist, skills, &mut on_event)?;
        if stop_hook.should_stop(result.iterations, &result.observations, result.done) {
            return Ok(result);
        }
        Ok(result)
    }
}

fn verify_shell_before_done(observations: &[ToolObservation]) -> Result<(), String> {
    let wrote = observations.iter().any(|o| o.tool == "fs_write" && o.success);
    if !wrote {
        return Ok(());
    }
    let verified = observations
        .iter()
        .any(|o| o.tool == "verify_contains" && o.success);
    if !verified {
        return Err("Loop blocked: done before verify_contains after fs_write".into());
    }
    let linted = observations
        .iter()
        .any(|o| o.tool == "python_lint" && o.success);
    if !linted {
        return Err("Loop blocked: done before python_lint after fs_write".into());
    }
    Ok(())
}

fn validate_mcp_arguments_in_workspace(workspace: &Path, args: &Value) -> Result<(), String> {
    let Some(path_val) = args.get("path").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    resolve_workspace_path(&workspace.to_path_buf(), path_val)?;
    Ok(())
}

fn require_verified_writes(pending_writes: &[String]) -> Result<(), String> {
    if pending_writes.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Loop blocked: fs_write without verify_contains for {:?}",
        pending_writes
    ))
}

/// Resolve a workspace-relative path and reject escapes (absolute paths, `..`, encoded segments).
fn resolve_workspace_path(workspace: &PathBuf, rel: &str) -> Result<PathBuf, String> {
    use aether_permissions::canonicalize_access_path;

    let workspace_canon = workspace
        .canonicalize()
        .map_err(|e| format!("Workspace canonicalize failed: {}", e))?;

    let rel = rel.trim_start_matches('\u{FEFF}');
    if rel.starts_with('/') || rel.starts_with('\\') {
        return Err(format!("Absolute path denied outside workspace: {}", rel));
    }

    let joined = workspace_canon.join(rel);
    let joined_str = joined.to_string_lossy().to_string();
    let resolved = match canonicalize_access_path(&joined_str) {
        Ok(p) => p,
        Err(e) => return Err(e),
    };

    if !path_is_subpath(&resolved, &workspace_canon) {
        return Err(format!("Path escapes workspace grant: {}", rel));
    }

    Ok(resolved)
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

fn observation(iteration: usize, tool: &str, success: bool, output: String) -> ToolObservation {
    ToolObservation {
        iteration,
        tool: tool.to_string(),
        success,
        output,
    }
}

/// Conservative byte-length estimate (~4 bytes per token).
fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    (text.len() + 3) / 4
}

fn estimate_invocation_tokens(step: &ToolInvocation) -> usize {
    match step {
        ToolInvocation::FsWrite { content, path } => {
            estimate_tokens(content) + estimate_tokens(path)
        }
        ToolInvocation::FsRead { path } => estimate_tokens(path),
        ToolInvocation::PythonLint { source } => estimate_tokens(source),
        ToolInvocation::GitInit { branch } => estimate_tokens(branch),
        ToolInvocation::McpCall { server, tool, args } => {
            estimate_tokens(server)
                + estimate_tokens(tool)
                + estimate_tokens(&args.to_string())
        }
        ToolInvocation::SkillExecute { skill_id, variables } => {
            estimate_tokens(skill_id) + estimate_tokens(&serde_json::to_string(variables).unwrap_or_default())
        }
        ToolInvocation::VerifyContains { path, text } => {
            estimate_tokens(path) + estimate_tokens(text)
        }
        ToolInvocation::Done => 1,
    }
}

fn emit_budget_telemetry<F>(config: &LoopConfig, iteration: usize, on_event: &mut F)
where
    F: FnMut(LoopStreamEvent),
{
    on_event(LoopStreamEvent::Budget {
        iteration,
        max_iterations: config.max_iterations,
        tokens_used: config.tokens_used,
        max_tokens: config.max_tokens,
    });
}

fn check_token_budget<F>(
    conn: &Connection,
    config: &LoopConfig,
    iteration: usize,
    on_event: &mut F,
) -> Result<(), LoopError>
where
    F: FnMut(LoopStreamEvent),
{
    if config.max_tokens == 0 || config.tokens_used <= config.max_tokens {
        return Ok(());
    }

    let used = config.tokens_used;
    let max = config.max_tokens;
    let msg = format!("Token budget exceeded: {} / {}", used, max);
    let args = serde_json::json!({
        "reason": "token_budget_exceeded",
        "tokens_used": used,
        "max_tokens": max,
        "iteration": iteration,
    })
    .to_string();

    let _ = PermissionManager::audit_decision(
        conn,
        &config.session_id,
        "loop_budget",
        &args,
        &PermissionDecision::Denied,
        Some(1),
        None,
    );

    on_event(LoopStreamEvent::Error {
        message: msg.clone(),
    });
    Err(LoopError::BudgetExceeded { used, max })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_db::Database;

    #[test]
    fn test_parse_plan_json() {
        let prompt = r#"{"loop":[{"action":"fs_write","path":"a.txt","content":"hi"},{"action":"done"}]}"#;
        let plan = ReActLoopEngine::parse_plan_from_prompt(prompt).unwrap();
        assert_eq!(plan.len(), 2);
    }

    #[test]
    fn test_structured_loop_fs_write_verify() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('s1', 't', 'active')",
            [],
        )
        .unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let ws = workspace.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES ('s1', ?1, 'write')",
            rusqlite::params![ws],
        )
        .unwrap();

        let mut config = LoopConfig {
            max_iterations: 5,
            max_tokens: 0,
            tokens_used: 0,
            session_id: "s1".into(),
            workspace,
        };

        let plan = vec![
            ToolInvocation::FsWrite {
                path: "marker.txt".into(),
                content: "LOOP-TEST".into(),
            },
            ToolInvocation::VerifyContains {
                path: "marker.txt".into(),
                text: "LOOP-TEST".into(),
            },
            ToolInvocation::PythonLint {
                source: "def ok():\n    return 1\n".into(),
            },
            ToolInvocation::Done,
        ];

        let engine = ReActLoopEngine::new(5);
        let result = engine
            .run_structured(&conn, &mut config, plan, None, &HashMap::new(), |_| {})
            .unwrap();

        assert!(result.done);
        assert!(result.iterations >= 2);
        assert!(result.tokens_used > 0);
    }

    #[test]
    fn test_token_budget_hard_stop_with_audit() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('s-budget', 't', 'active')",
            [],
        )
        .unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let ws = workspace.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES ('s-budget', ?1, 'write')",
            rusqlite::params![ws],
        )
        .unwrap();

        let mut config = LoopConfig {
            max_iterations: 5,
            max_tokens: 8,
            tokens_used: 0,
            session_id: "s-budget".into(),
            workspace,
        };

        let plan = vec![
            ToolInvocation::FsWrite {
                path: "big.txt".into(),
                content: "x".repeat(128),
            },
            ToolInvocation::Done,
        ];

        let engine = ReActLoopEngine::new(5);
        let mut budget_events = 0usize;
        let err = engine
            .run_structured(&conn, &mut config, plan, None, &HashMap::new(), |event| {
                if matches!(event, LoopStreamEvent::Budget { .. }) {
                    budget_events += 1;
                }
            })
            .unwrap_err();

        assert_eq!(
            err,
            LoopError::BudgetExceeded {
                used: config.tokens_used,
                max: 8
            }
        );
        assert!(config.tokens_used > 8);
        assert!(budget_events >= 1);

        let audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE tool_name = 'loop_budget' AND decision = 'denied';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(audit_count, 1);
    }

    #[test]
    fn test_budget_telemetry_emitted_each_step() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('s-tel', 't', 'active')",
            [],
        )
        .unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let ws = workspace.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES ('s-tel', ?1, 'write')",
            rusqlite::params![ws],
        )
        .unwrap();

        let mut config = LoopConfig {
            max_iterations: 5,
            max_tokens: 0,
            tokens_used: 0,
            session_id: "s-tel".into(),
            workspace,
        };

        let plan = vec![
            ToolInvocation::FsWrite {
                path: "a.txt".into(),
                content: "hi".into(),
            },
            ToolInvocation::VerifyContains {
                path: "a.txt".into(),
                text: "hi".into(),
            },
            ToolInvocation::PythonLint {
                source: "def ok():\n    return 1\n".into(),
            },
            ToolInvocation::Done,
        ];

        let engine = ReActLoopEngine::new(5);
        let mut budget_snapshots = Vec::new();
        engine
            .run_structured(&conn, &mut config, plan, None, &HashMap::new(), |event| {
                if let LoopStreamEvent::Budget {
                    tokens_used,
                    max_tokens,
                    iteration,
                    max_iterations,
                } = event
                {
                    budget_snapshots.push((iteration, max_iterations, tokens_used, max_tokens));
                }
            })
            .unwrap();

        assert!(!budget_snapshots.is_empty());
        assert_eq!(budget_snapshots.last().unwrap().2, config.tokens_used);
    }
}
