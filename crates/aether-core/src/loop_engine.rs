use crate::cost::{audit_loop_token_usage, ProviderTokenUsage};
use crate::inject::wrap_untrusted_tool_output;
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

/// Sane default token cap for daemon loop tasks (Phase 8.1 / BUDG-01).
/// Override with `AETHER_MAX_LOOP_TOKENS`; set to `0` for unlimited.
pub const DEFAULT_MAX_LOOP_TOKENS: usize = 16_384;

/// Resolve the daemon default loop token budget from the environment.


pub fn record_provider_token_usage<F>(conn: &Connection, config: &mut LoopConfig, source: &str, usage: ProviderTokenUsage, iteration: Option<usize>, on_event: &mut F) -> Result<(), String> where F: FnMut(LoopStreamEvent) {
    if usage.is_empty() { return Ok(()); }
    config.provider_input_tokens = config.provider_input_tokens.saturating_add(usage.input_tokens);
    config.provider_output_tokens = config.provider_output_tokens.saturating_add(usage.output_tokens);
    config.tokens_used = config.tokens_used.saturating_add(usage.total());
    audit_loop_token_usage(conn, &config.session_id, source, usage, iteration)?;
    on_event(LoopStreamEvent::ProviderTokens { source: source.to_string(), input_tokens: usage.input_tokens, output_tokens: usage.output_tokens, tokens_used: config.tokens_used, iteration });
    emit_budget_telemetry(config, iteration.unwrap_or(0), on_event);
    Ok(())
}

pub fn resolve_default_max_loop_tokens() -> usize {
    std::env::var("AETHER_MAX_LOOP_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_LOOP_TOKENS)
}

#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub max_iterations: usize,
    /// Token budget cap (`0` = unlimited).
    pub max_tokens: usize,
    /// Running token tally; updated by `ReActLoopEngine` during execution.
    pub tokens_used: usize,
    pub provider_input_tokens: usize,
    pub provider_output_tokens: usize,
    pub session_id: String,
    pub workspace: PathBuf,
}

impl LoopConfig {
    pub fn new(max_iterations: usize, session_id: String, workspace: PathBuf) -> Self {
        Self {
            max_iterations,
            max_tokens: resolve_default_max_loop_tokens(),
            tokens_used: 0,
            provider_input_tokens: 0,
            provider_output_tokens: 0,
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
    /// Read a file, optionally a window of it (P1-6 / READ-01).
    ///
    /// `offset`/`limit` are in **characters** and both optional, so every existing plan shape
    /// (`{"action":"fs_read","path":"…"}`) still decodes. They exist because a bounded read that
    /// cannot be paged turns "not in the first 500 chars" into "not in the file": without a window
    /// the only recovery is to give up, and a model that cannot distinguish *absent* from
    /// *truncated* confidently reports absence.
    FsRead {
        path: String,
        /// Character offset to start from. `None` = start of file.
        #[serde(default)]
        offset: Option<usize>,
        /// Maximum characters to return. `None` = [`FS_READ_MAX_CHARS`].
        #[serde(default)]
        limit: Option<usize>,
    },
    PythonLint {
        source: String,
    },
    /// Syntax-check a Python file **that is already on disk** (CHECK-02).
    ///
    /// [`ToolInvocation::PythonLint`] checks source supplied in the plan, which proves nothing
    /// about any artifact the plan wrote — a plan can lint a trivial snippet and `done` while the
    /// `.py` file it just wrote is broken. This action closes that gap: the post-write verify shell
    /// requires a successful `python_lint_file` *on each written `.py` path*, so the thing that gets
    /// certified is the file, not an unrelated string.
    PythonLintFile {
        path: String,
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
        /// Name of a brokered secret to inject as an environment variable of the same name into
        /// the MCP server subprocess for this call only (Phase 11 slice 11.6 / SEC-01). The
        /// resolved value never enters this struct, args, the session log, or the audit log —
        /// only the name does. Resolution happens in `ToolRegistry::execute`, right before spawn.
        #[serde(default)]
        secret_env: Option<String>,
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
    /// Delegate a read-heavy batch to a subagent with its own file-count budget (Phase 10 slices
    /// 10.3-10.4 / SUB-01). Returns one distilled summary observation, not each file's full
    /// content — this is what keeps the parent's context bounded regardless of how much the
    /// subagent reads internally.
    SubagentTask {
        paths: Vec<String>,
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
        provider_input_tokens: usize,
        provider_output_tokens: usize,
    },
    ProviderTokens { source: String, input_tokens: usize, output_tokens: usize, tokens_used: usize, iteration: Option<usize>, },
    Budget {
        iteration: usize,
        max_iterations: usize,
        tokens_used: usize,
        max_tokens: usize,
        provider_input_tokens: usize,
        provider_output_tokens: usize,
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

/// Character budget for one `fs_read` observation when the plan does not request a window (P1-6).
///
/// Bounded on purpose — an unbounded read is how a 40 KB file eats a small local model's whole
/// context. The bound is only honest if the observation *reports* it, which is what
/// [`render_read_window`] does.
pub const FS_READ_MAX_CHARS: usize = 500;

/// Share of the budget given to the tail when a read is truncated without an explicit window.
///
/// Truncation is from the middle, not the head: the tail of a source file carries `return`,
/// `main`, and the closing brace, which is exactly what a summarising goal needs and exactly what
/// head-only truncation throws away.
const FS_READ_TAIL_DIVISOR: usize = 3;

/// Render one `fs_read` observation body. Returns `(text, success)`.
///
/// Three guarantees, all asserted by READ-01:
///
/// 1. a window that fits is returned **verbatim** — no marker, no decoration, so a small file read
///    is still byte-identical to the file;
/// 2. anything cut is cut **from the middle** (head + tail) unless the caller asked for an explicit
///    window, in which case exactly that window is returned;
/// 3. every cut **says it was cut**, with the true character count, how much was omitted, and the
///    literal next step that pages further. An offset past the end is a failed observation naming
///    the real size, never an empty string that reads as "not present".
pub fn render_read_window(
    path: &str,
    content: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> (String, bool) {
    let chars: Vec<char> = content.chars().collect();
    let total = chars.len();
    let start = offset.unwrap_or(0);

    if start > total {
        return (
            format!(
                "offset {start} is past the end of {path} ({total} chars) — re-read from offset 0, \
                 or page inside that size with \"offset\"/\"limit\"",
            ),
            false,
        );
    }

    let budget = limit.unwrap_or(FS_READ_MAX_CHARS).max(1);
    let available = total - start;
    if available <= budget {
        return (chars[start..].iter().collect::<String>(), true);
    }

    if limit.is_some() {
        // An explicit window is exactly what was asked for: return it undecorated except for the
        // bound report, so a caller paging through a file can concatenate the windows.
        let window: String = chars[start..start + budget].iter().collect();
        let remaining = available - budget;
        let next = start + budget;
        return (
            format!(
                "{window}\n[truncated: showing {budget} of {total} chars from offset {start}; \
                 {remaining} chars remain — next page {{\"action\":\"fs_read\",\"path\":\"{path}\",\
                 \"offset\":{next},\"limit\":{budget}}}]",
            ),
            true,
        );
    }

    // No window requested: head + tail, with the omitted middle reported.
    let tail = (budget / FS_READ_TAIL_DIVISOR).max(1);
    let head = (budget - tail).max(1);
    let head_text: String = chars[start..start + head].iter().collect();
    let tail_start = total - tail;
    let tail_text: String = chars[tail_start..].iter().collect();
    let omitted = tail_start - (start + head);
    (
        format!(
            "{head_text}\n[truncated from the middle: showing {head} head + {tail} tail of \
             {total} chars, {omitted} omitted — page with {{\"action\":\"fs_read\",\
             \"path\":\"{path}\",\"offset\":<n>,\"limit\":<n>}}]\n{tail_text}",
        ),
        true,
    )
}

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
                if let crate::HookDecision::Deny(reason) = crate::HookEngine::production().run_pre_tool_use(&full) {
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
                    // Remedy-bearing at the producing site (P1-9): the reason stays the leading
                    // substring, so every existing assertion and audit row still matches.
                    return Err(crate::ToolError::write_denied(&full_str).render());
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
            ToolInvocation::FsRead {
                path,
                offset,
                limit,
            } => {
                let full = resolve_workspace_path(&config.workspace, path)?;
                let full_str = full.to_string_lossy().to_string();
                if let crate::HookDecision::Deny(reason) = crate::HookEngine::production().run_pre_tool_use(&full) {
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
                    return Err(crate::ToolError::read_denied(&full_str).render());
                }
                let content = ProductionSandbox::read_to_string(&config.workspace, &full)
                    .map_err(|e| e.to_string())?;
                let (text, ok) = render_read_window(path, &content, *offset, *limit);
                Ok(observation(iteration, "fs_read", ok, text))
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
            ToolInvocation::PythonLintFile { path } => {
                let full = resolve_workspace_path(&config.workspace, path)?;
                let full_str = full.to_string_lossy().to_string();
                if let crate::HookDecision::Deny(reason) =
                    crate::HookEngine::production().run_pre_tool_use(&full)
                {
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
                    return Err(crate::ToolError::read_denied(&full_str).render());
                }
                // Lint the artifact *as written*. `check_syntax_in_workspace` copies the source
                // into `.aether-tmp` and runs `py_compile` there, so this adds no new side effect
                // to the workspace root and keeps exactly the same sandbox boundary as
                // `python_lint` — the only difference is where the source came from.
                match ProductionSandbox::read_to_string(&config.workspace, &full) {
                    Ok(content) => {
                        match PythonLinter::check_syntax_in_workspace(&content, &config.workspace) {
                            Ok(issues) if issues.is_empty() => Ok(observation(
                                iteration,
                                "python_lint_file",
                                true,
                                format!("syntax OK: {}", path),
                            )),
                            Ok(issues) => Ok(observation(
                                iteration,
                                "python_lint_file",
                                false,
                                format!("{} issue(s) in {}: {:?}", issues.len(), path, issues),
                            )),
                            Err(e) => Ok(observation(
                                iteration,
                                "python_lint_file",
                                false,
                                format!("{}: {}", path, e),
                            )),
                        }
                    }
                    Err(e) => Ok(observation(
                        iteration,
                        "python_lint_file",
                        false,
                        format!("cannot lint {}: {}", path, e),
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
            ToolInvocation::McpCall {
                server,
                tool,
                args,
                secret_env,
            } => {
                let allowlist = allowlist
                    .ok_or_else(|| "MCP allowlist not configured".to_string())?;
                validate_mcp_arguments_in_workspace(&config.workspace, args)?;
                let workspace_str = config.workspace.to_string_lossy().to_string();
                let extra = vec![workspace_str.clone()];
                // Resolve brokered secret by name only. The value is held ephemerally for the
                // spawn env injection below and never enters args, the observation, or the audit
                // log (Phase 11 slice 11.6 / SEC-01).
                let (extra_env, redact_values) = match secret_env {
                    Some(name) => {
                        validate_secret_env_name(name)?;
                        let value = crate::load_named_secret(name)
                            .map_err(|e| e.to_string())?
                            .ok_or_else(|| {
                                format!("brokered secret '{name}' is not configured")
                            })?;
                        (vec![(name.clone(), value.clone())], vec![value])
                    }
                    None => (Vec::new(), Vec::new()),
                };
                match invoke_with_grant(
                    conn,
                    &config.session_id,
                    &workspace_str,
                    allowlist,
                    server,
                    tool,
                    args.clone(),
                    &extra,
                    &extra_env,
                ) {
                    Ok((result, audit)) => {
                        let raw = format!(
                            "tools_hash={} result={}",
                            audit.tools_hash,
                            result.to_string().chars().take(200).collect::<String>()
                        );
                        Ok(observation(
                            iteration,
                            "mcp_call",
                            true,
                            redact_secret_values(&raw, &redact_values),
                        ))
                    }
                    Err(e) => Ok(observation(
                        iteration,
                        "mcp_call",
                        false,
                        redact_secret_values(&e.to_string(), &redact_values),
                    )),
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
            ToolInvocation::SubagentTask { paths } => {
                for path in paths {
                    let full = resolve_workspace_path(&config.workspace, path)?;
                    if let crate::HookDecision::Deny(reason) = crate::HookEngine::production().run_pre_tool_use(&full) {
                        return Err(reason);
                    }
                    let full_str = full.to_string_lossy().to_string();
                    let decision = PermissionManager::check_file_access(
                        conn,
                        &config.session_id,
                        &full_str,
                        "read",
                    )
                    .map_err(|e| e.to_string())?;
                    if decision != PermissionDecision::Approved {
                        return Err(crate::ToolError::read_denied(&full_str).render());
                    }
                }
                match crate::run_subagent_read_task(&config.workspace, paths) {
                    Ok(result) => Ok(observation(
                        iteration,
                        "subagent_task",
                        true,
                        result.distilled,
                    )),
                    Err(e) => Ok(observation(iteration, "subagent_task", false, e)),
                }
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
        // CHECK-02: `pending_writes` is *consumed* by each passing `verify_contains`, so it cannot
        // answer "which artifacts did this run produce?". These two ledgers are never consumed —
        // they are what the post-write verify shell checks the per-path lint requirement against.
        let mut written_artifacts: Vec<String> = Vec::new();
        let mut linted_artifacts: Vec<String> = Vec::new();

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
                if let Err(msg) =
                    verify_shell_before_done(&observations, &written_artifacts, &linted_artifacts)
                {
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
                // Delimit tool output at the stream/session-log boundary (Phase 11 / INJECT-01).
                // Internal `ToolObservation` stays raw so verify-shell matching is unaffected.
                let bounded = crate::post_tool_use_scrub_output(&wrap_untrusted_tool_output(
                &obs.tool,
                &obs.output,
            ));
                on_event(LoopStreamEvent::Tool {
                    iteration,
                    tool: obs.tool.clone(),
                    output: bounded.clone(),
                });
                on_event(LoopStreamEvent::Observe {
                    iteration,
                    summary: bounded,
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
            let bounded = crate::post_tool_use_scrub_output(&wrap_untrusted_tool_output(
                &obs.tool,
                &obs.output,
            ));
            on_event(LoopStreamEvent::Tool {
                iteration,
                tool: obs.tool.clone(),
                output: bounded.clone(),
            });
            on_event(LoopStreamEvent::Observe {
                iteration,
                summary: bounded,
            });

            if let ToolInvocation::FsWrite { path, .. } = &step {
                if obs.success {
                    pending_writes.push(path.clone());
                    written_artifacts.push(path.clone());
                }
            }

            if matches!(
                step,
                ToolInvocation::PythonLint { .. } | ToolInvocation::PythonLintFile { .. }
            ) {
                if let ToolInvocation::PythonLintFile { path } = &step {
                    if obs.success {
                        linted_artifacts.push(path.clone());
                    }
                }
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

        on_event(LoopStreamEvent::Done { iterations: iteration, summary: summary.clone(), tokens_used: config.tokens_used, provider_input_tokens: config.provider_input_tokens, provider_output_tokens: config.provider_output_tokens, });
        let summary_usage = ProviderTokenUsage { input_tokens: config.provider_input_tokens, output_tokens: config.provider_output_tokens };
        let _ = audit_loop_token_usage(conn, &config.session_id, "loop_structured_summary", summary_usage, None);

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

/// Extensions whose written artifacts must be linted *as written* before `done` (CHECK-02).
pub const LINTABLE_ARTIFACT_EXTENSIONS: &[&str] = &[".py"];

/// True when a written path is an artifact the verify shell must see linted on disk.
pub fn is_lintable_artifact(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    LINTABLE_ARTIFACT_EXTENSIONS
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Post-write verification gate, enforced immediately before a `done` step commits the run.
///
/// Three requirements, in increasing specificity:
/// 1. any write at all requires a passing `verify_contains` (the bytes landed);
/// 2. any write at all requires a passing lint (either flavour) — the historical LOOP-01/02/04 shape;
/// 3. **every written artifact with a lintable extension requires a passing `python_lint_file` on
///    that same path** (CHECK-02).
///
/// Requirement 3 is the one that closes the theater hole. `python_lint` checks source *supplied in
/// the plan*, so requirements 1–2 alone are satisfiable by a plan that writes broken Python and
/// lints an unrelated `def ok(): return 1` snippet — the gate passes while the artifact is broken,
/// and the run reports `done`. Certifying the file, rather than a string that was never written, is
/// the whole point of the gate.
fn verify_shell_before_done(
    observations: &[ToolObservation],
    written_artifacts: &[String],
    linted_artifacts: &[String],
) -> Result<(), String> {
    let wrote = observations
        .iter()
        .any(|o| o.tool == "fs_write" && o.success);
    if !wrote {
        return Ok(());
    }
    let verified = observations
        .iter()
        .any(|o| o.tool == "verify_contains" && o.success);
    if !verified {
        return Err("Loop blocked: done before verify_contains after fs_write".into());
    }
    let linted = observations.iter().any(|o| {
        (o.tool == "python_lint" || o.tool == "python_lint_file") && o.success
    });
    if !linted {
        return Err("Loop blocked: done before python_lint after fs_write".into());
    }
    for path in written_artifacts {
        if !is_lintable_artifact(path) {
            continue;
        }
        if linted_artifacts.iter().any(|linted| linted == path) {
            continue;
        }
        return Err(format!(
            "Loop blocked: {path} was written but never linted as an artifact. python_lint \
             checks source supplied in the plan, not the file on disk, so it cannot certify this \
             write. Add {{\"action\":\"python_lint_file\",\"path\":\"{path}\"}} after the write."
        ));
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
pub(crate) fn resolve_workspace_path(workspace: &PathBuf, rel: &str) -> Result<PathBuf, String> {
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

/// Brokered secret names become child-process env keys; keep them boring so a planner cannot
/// smuggle `PATH`/`LD_PRELOAD`-style overrides through `secret_env`.
fn validate_secret_env_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("secret_env name must be 1..=64 characters".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(
            "secret_env name must be ASCII uppercase letters, digits, or underscore".into(),
        );
    }
    if name.starts_with("AETHER_") || name == "PATH" || name == "HOME" || name == "TMPDIR" {
        return Err(format!("secret_env name '{name}' is reserved"));
    }
    Ok(())
}

fn redact_secret_values(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if !secret.is_empty() && out.contains(secret) {
            out = out.replace(secret, "[REDACTED]");
        }
    }
    out
}

fn tool_name(step: &ToolInvocation) -> &str {
    match step {
        ToolInvocation::FsWrite { .. } => "fs_write",
        ToolInvocation::FsRead { .. } => "fs_read",
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
        ToolInvocation::FsRead { path, .. } => estimate_tokens(path),
        ToolInvocation::PythonLint { source } => estimate_tokens(source),
        ToolInvocation::PythonLintFile { path } => estimate_tokens(path),
        ToolInvocation::GitInit { branch } => estimate_tokens(branch),
        ToolInvocation::McpCall {
            server,
            tool,
            args,
            secret_env,
        } => {
            // Count the secret *name* only — never resolve or estimate from the value.
            estimate_tokens(server)
                + estimate_tokens(tool)
                + estimate_tokens(&args.to_string())
                + secret_env.as_deref().map(estimate_tokens).unwrap_or(0)
        }
        ToolInvocation::SkillExecute { skill_id, variables } => {
            estimate_tokens(skill_id) + estimate_tokens(&serde_json::to_string(variables).unwrap_or_default())
        }
        ToolInvocation::VerifyContains { path, text } => {
            estimate_tokens(path) + estimate_tokens(text)
        }
        ToolInvocation::SubagentTask { paths } => {
            paths.iter().map(|p| estimate_tokens(p)).sum()
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
        provider_input_tokens: config.provider_input_tokens,
        provider_output_tokens: config.provider_output_tokens,
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
            provider_input_tokens: 0,
            provider_output_tokens: 0,
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
            provider_input_tokens: 0,
            provider_output_tokens: 0,
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
            provider_input_tokens: 0,
            provider_output_tokens: 0,
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
                if let LoopStreamEvent::Budget { tokens_used, max_tokens, iteration, max_iterations, .. } = event
                {
                    budget_snapshots.push((iteration, max_iterations, tokens_used, max_tokens));
                }
            })
            .unwrap();

        assert!(!budget_snapshots.is_empty());
        assert_eq!(budget_snapshots.last().unwrap().2, config.tokens_used);
    }
}
