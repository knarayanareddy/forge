//! Provider-reported token accounting (Phase 9 slice 9.11 / COST-01).

use aether_permissions::{PermissionDecision, PermissionManager};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderTokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

impl ProviderTokenUsage {
    pub fn total(self) -> usize {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    pub fn is_empty(self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0
    }

    pub fn merge(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}

pub fn ollama_token_usage(
    prompt_eval_count: Option<u64>,
    eval_count: Option<u64>,
) -> Option<ProviderTokenUsage> {
    match (prompt_eval_count, eval_count) {
        (None, None) => None,
        (input, output) => Some(ProviderTokenUsage {
            input_tokens: input.unwrap_or(0) as usize,
            output_tokens: output.unwrap_or(0) as usize,
        }),
    }
}

pub fn openai_token_usage(
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
) -> Option<ProviderTokenUsage> {
    match (prompt_tokens, completion_tokens) {
        (None, None) => None,
        (input, output) => Some(ProviderTokenUsage {
            input_tokens: input.unwrap_or(0) as usize,
            output_tokens: output.unwrap_or(0) as usize,
        }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoopCostAttribution {
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

pub fn audit_loop_cost(
    conn: &Connection,
    session_id: &str,
    attribution: &LoopCostAttribution,
    usage: ProviderTokenUsage,
    source: &str,
) -> rusqlite::Result<()> {
    if usage.is_empty() {
        return Ok(());
    }

    let args = serde_json::json!({
        "attribution": attribution.phase,
        "iteration": attribution.iteration,
        "tool": attribution.tool,
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "total_tokens": usage.total(),
        "source": source,
    })
    .to_string();

    PermissionManager::audit_decision(
        conn,
        session_id,
        "loop_cost",
        &args,
        &PermissionDecision::AutoAllowed,
        None,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_db::Database;

    #[test]
    fn audit_loop_cost_writes_input_and_output_tokens() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('sess-cost', 'COST', 'active')",
            [],
        )
        .unwrap();

        audit_loop_cost(
            &conn,
            "sess-cost",
            &LoopCostAttribution {
                phase: "loop_run".into(),
                iteration: Some(1),
                tool: None,
            },
            ProviderTokenUsage {
                input_tokens: 42,
                output_tokens: 7,
            },
            "ollama",
        )
        .unwrap();

        let args: String = conn
            .query_row(
                "SELECT arguments_json FROM audit_log WHERE tool_name = 'loop_cost' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&args).unwrap();
        assert_eq!(parsed["input_tokens"], 42);
        assert_eq!(parsed["output_tokens"], 7);
    }
}
