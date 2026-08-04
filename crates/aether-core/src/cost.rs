//! Provider-reported token accounting (Phase 9.11 / COST-01).
use aether_permissions::{PermissionDecision, PermissionManager};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTokenUsage { pub input_tokens: usize, pub output_tokens: usize }
impl ProviderTokenUsage {
    pub fn is_empty(self) -> bool { self.input_tokens == 0 && self.output_tokens == 0 }
    pub fn total(self) -> usize { self.input_tokens.saturating_add(self.output_tokens) }
    pub fn merge(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
    }
}
pub fn ollama_token_usage(prompt_eval_count: Option<u64>, eval_count: Option<u64>) -> Option<ProviderTokenUsage> {
    let input = prompt_eval_count? as usize; let output = eval_count? as usize;
    if input == 0 && output == 0 { return None; }
    Some(ProviderTokenUsage { input_tokens: input, output_tokens: output })
}
pub fn openai_token_usage(prompt_tokens: Option<u32>, completion_tokens: Option<u32>) -> Option<ProviderTokenUsage> {
    let input = prompt_tokens? as usize; let output = completion_tokens? as usize;
    if input == 0 && output == 0 { return None; }
    Some(ProviderTokenUsage { input_tokens: input, output_tokens: output })
}
pub fn audit_loop_token_usage(conn: &Connection, session_id: &str, source: &str, usage: ProviderTokenUsage, iteration: Option<usize>) -> Result<(), String> {
    if usage.is_empty() { return Ok(()); }
    let args = serde_json::json!({"source": source, "input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens, "iteration": iteration}).to_string();
    PermissionManager::audit_decision(conn, session_id, "loop_token_usage", &args, &PermissionDecision::AutoAllowed, None, None).map_err(|e| e.to_string())
}
