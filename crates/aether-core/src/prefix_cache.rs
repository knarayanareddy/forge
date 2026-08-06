//! Prompt-prefix / KV-reuse stability (Phase 9 slice 9.12 / CACHE-01).

use crate::build_nl_plan_prompt;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub const CACHE01_MIN_REUSE_RATIO: f64 = 0.70;

pub fn static_tools_prefix() -> String {
    build_nl_plan_prompt("")
        .trim_end()
        .trim_end_matches("User goal:")
        .trim_end()
        .to_string()
}

pub fn assemble_context_prompt(static_prefix: &str, volatile_parts: &[&str]) -> String {
    let mut out = String::with_capacity(
        static_prefix.len() + volatile_parts.iter().map(|p| p.len()).sum::<usize>() + 16,
    );
    out.push_str(static_prefix);
    for part in volatile_parts {
        out.push_str(part);
    }
    out
}

pub fn sort_tool_results_deterministic(results: &mut [(String, String)]) {
    results.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
}

pub fn prefix_fingerprint(prefix: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    prefix.hash(&mut hasher);
    hasher.finish()
}

pub fn measure_prefix_reuse(previous: &str, next: &str, static_prefix: &str) -> f64 {
    if !next.starts_with(static_prefix) || !previous.starts_with(static_prefix) {
        return 0.0;
    }
    if next.len() <= previous.len() {
        return if next == previous { 1.0 } else { 0.0 };
    }
    if !next.starts_with(previous) || next.is_empty() {
        return 0.0;
    }
    previous.len() as f64 / next.len() as f64
}

pub fn build_volatile_replan_tail(
    tool_results: &mut [(String, String)],
    user_goal: &str,
) -> String {
    sort_tool_results_deterministic(tool_results);
    let mut tail = String::from("\n\n<volatile_state>\n");
    for (tool, output) in tool_results {
        tail.push_str(&format!("tool={tool}\n{output}\n"));
    }
    tail.push_str("</volatile_state>\n\nUser goal:\n");
    tail.push_str(user_goal);
    tail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuse_ratio_meets_turn2_target() {
        let prefix = static_tools_prefix();
        let turn1 = assemble_context_prompt(&prefix, &["\n\nUser goal:\ngoal-a"]);
        let turn2 = assemble_context_prompt(
            &prefix,
            &["\n\nUser goal:\ngoal-a", "\n\n<observation>ok</observation>"],
        );
        assert!(measure_prefix_reuse(&turn1, &turn2, &prefix) >= CACHE01_MIN_REUSE_RATIO);
    }
}
