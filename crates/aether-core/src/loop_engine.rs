use crate::{GitOps, LoopError, PythonLinter};
use aether_mcp::{invoke_with_grant, McpAllowlist};
use aether_permissions::{PermissionDecision, PermissionManager};
use aether_skills::{SkillDefinition, SkillExecutor};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub max_iterations: usize,
    pub session_id: String,
    pub workspace: PathBuf,
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

#[derive(Debug, Clone)]
pub struct LoopRunResult {
    pub iterations: usize,
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
                let full = config.workspace.join(path);
                let workspace_str = config.workspace.to_string_lossy().to_string();
                let decision = PermissionManager::check_file_access(
                    conn,
                    &config.session_id,
                    &workspace_str,
                    "write",
                )
                .map_err(|e| e.to_string())?;
                if decision != PermissionDecision::Approved {
                    return Err(format!(
                        "Write denied for workspace {}",
                        workspace_str
                    ));
                }
                fs::write(&full, content).map_err(|e| e.to_string())?;
                Ok(observation(
                    iteration,
                    "fs_write",
                    true,
                    format!("Wrote {} bytes to {}", content.len(), path),
                ))
            }
            ToolInvocation::FsRead { path } => {
                let full = config.workspace.join(path);
                let full_str = full.to_string_lossy().to_string();
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
                let content = fs::read_to_string(&full).map_err(|e| e.to_string())?;
                Ok(observation(
                    iteration,
                    "fs_read",
                    true,
                    content.chars().take(500).collect(),
                ))
            }
            ToolInvocation::PythonLint { source } => {
                match PythonLinter::check_syntax(source) {
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
                    Ok(()) => Ok(observation(
                        iteration,
                        "git_init",
                        true,
                        format!("Initialized repo on branch {}", branch),
                    )),
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
                let full = config.workspace.join(path);
                let content = fs::read_to_string(&full).map_err(|e| e.to_string())?;
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
        config: &LoopConfig,
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

            if matches!(step, ToolInvocation::Done) {
                let obs = ToolRegistry::execute(
                    conn,
                    config,
                    allowlist,
                    skills,
                    iteration,
                    &step,
                )
                .map_err(LoopError::Turn)?;
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
            on_event(LoopStreamEvent::Tool {
                iteration,
                tool: obs.tool.clone(),
                output: obs.output.clone(),
            });
            on_event(LoopStreamEvent::Observe {
                iteration,
                summary: obs.output.clone(),
            });

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
                    return Err(LoopError::Turn(obs.output));
                }
                continue;
            }

            if matches!(step, ToolInvocation::VerifyContains { .. }) {
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
                    return Err(LoopError::Turn(obs.output));
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
        });

        Ok(LoopRunResult {
            iterations: iteration,
            observations,
            summary,
            done: true,
        })
    }

    pub fn run_with_stop_hook<S: StopHook>(
        &self,
        conn: &Connection,
        config: &LoopConfig,
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

        let config = LoopConfig {
            max_iterations: 5,
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
            ToolInvocation::Done,
        ];

        let engine = ReActLoopEngine::new(5);
        let result = engine
            .run_structured(&conn, &config, plan, None, &HashMap::new(), |_| {})
            .unwrap();

        assert!(result.done);
        assert!(result.iterations >= 2);
    }
}
