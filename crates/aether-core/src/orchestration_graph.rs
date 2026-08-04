use crate::loop_engine::{
    LoopConfig, LoopRunResult, LoopStreamEvent, ReActLoopEngine, ToolInvocation,
};
use crate::LoopError;
use crate::verifier_node::{MakerCheckerGoal, VerifierNode};
use aether_mcp::McpAllowlist;
use aether_skills::SkillDefinition;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;

/// Two-node orchestration scaffold: executor plan + read-only verifier (Phase 7 slice 7.4).
pub struct OrchestrationGraph {
    pub checker_enabled: bool,
    max_iterations: usize,
}

impl OrchestrationGraph {
    pub fn new(checker_enabled: bool, max_iterations: usize) -> Self {
        Self {
            checker_enabled,
            max_iterations,
        }
    }

    pub fn checker_enabled(&self) -> bool {
        self.checker_enabled
    }

    /// Parse optional checker goal from structured prompt JSON.
    pub fn parse_checker_goal(prompt: &str) -> Option<MakerCheckerGoal> {
        let value: Value = serde_json::from_str(prompt.trim()).ok()?;
        let goal = value.get("checker_goal")?;
        let expected_content = goal
            .get("expected_content")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())?
            .to_string();
        Some(MakerCheckerGoal { expected_content })
    }

    /// Maker-checker path: verifier pre-approves every proposed `fs_write` before the executor runs.
    pub fn run_maker_checker<F>(
        &self,
        conn: &Connection,
        config: &mut LoopConfig,
        goal: &MakerCheckerGoal,
        plan: Vec<ToolInvocation>,
        allowlist: Option<&McpAllowlist>,
        skills: &HashMap<String, SkillDefinition>,
        mut on_event: F,
    ) -> Result<LoopRunResult, LoopError>
    where
        F: FnMut(LoopStreamEvent),
    {
        if self.checker_enabled {
            let verifier_session = VerifierNode::verifier_session_id(&config.session_id);
            for step in &plan {
                if let ToolInvocation::FsWrite { path, content } = step {
                    if let Err(msg) = VerifierNode::verify_fs_write_proposal(
                        conn,
                        &verifier_session,
                        &config.workspace,
                        path,
                        content,
                        goal,
                    ) {
                        on_event(LoopStreamEvent::Verify {
                            iteration: 0,
                            passed: false,
                            detail: msg.clone(),
                        });
                        on_event(LoopStreamEvent::Error {
                            message: msg.clone(),
                        });
                        return Err(LoopError::Turn(msg));
                    }
                }
            }
        }

        let engine = ReActLoopEngine::new(self.max_iterations);
        engine.run_structured(conn, config, plan, allowlist, skills, on_event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_db::Database;
    use tempfile::tempdir;

    #[test]
    fn parse_checker_goal_from_prompt() {
        let prompt = r#"{"loop":[],"checker_goal":{"expected_content":"MARK"}}"#;
        let goal = OrchestrationGraph::parse_checker_goal(prompt).unwrap();
        assert_eq!(goal.expected_content, "MARK");
    }

    #[test]
    fn maker_checker_blocks_bad_write_before_commit() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('exec', 't', 'active')",
            [],
        )
        .unwrap();

        let tmp = tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let ws = workspace.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES ('exec', ?1, 'write')",
            rusqlite::params![ws],
        )
        .unwrap();

        let mut config = LoopConfig {
            max_iterations: 4,
            max_tokens: 0,
            tokens_used: 0,
            provider_input_tokens: 0,
            provider_output_tokens: 0,
            session_id: "exec".into(),
            workspace,
        };

        let plan = vec![
            ToolInvocation::FsWrite {
                path: "bad.txt".into(),
                content: "not-the-marker".into(),
            },
            ToolInvocation::Done,
        ];
        let goal = MakerCheckerGoal {
            expected_content: "GOOD-MARKER".into(),
        };

        let graph = OrchestrationGraph::new(true, 4);
        let err = graph
            .run_maker_checker(
                &conn,
                &mut config,
                &goal,
                plan,
                None,
                &HashMap::new(),
                |_| {},
            )
            .unwrap_err();
        assert!(matches!(err, LoopError::Turn(_)));
        assert!(!tmp.path().join("bad.txt").exists());
    }
}
