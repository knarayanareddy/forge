//! Context compaction with steerable instructions and thrashing guard (Phase 10.5 / COMPACT-01).
//!
//! When a session transcript exceeds budget, older turns are summarized according to a caller-
//! supplied instruction. If compaction fails to shrink the payload within bounded attempts, the
//! caller gets an explicit error instead of looping forever.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One conversational turn in the compaction input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextTurn {
    pub role: String,
    pub content: String,
}

/// Steerable compaction request. `instruction` tells the summarizer what to preserve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactRequest {
    pub instruction: String,
    /// Maximum compaction attempts before failing closed with [`CompactionError::Thrashing`].
    pub max_attempts: usize,
    /// Required fractional shrink (0.0–1.0). Default 0.10 = at least 10% smaller.
    pub min_reduction_ratio: f64,
    /// Number of trailing turns kept verbatim (never summarized away).
    pub keep_recent: usize,
}

impl Default for CompactRequest {
    fn default() -> Self {
        Self {
            instruction: "Preserve decisions, file paths, and open tasks.".into(),
            max_attempts: 3,
            min_reduction_ratio: 0.10,
            keep_recent: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CompactionError {
    #[error("nothing to compact")]
    Empty,
    #[error("compaction thrashing: {attempts} attempts failed to shrink context ({last_chars} chars remaining)")]
    Thrashing {
        attempts: usize,
        last_chars: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionResult {
    pub summary: String,
    pub recent_turns: Vec<ContextTurn>,
    pub attempts_used: usize,
    pub chars_before: usize,
    pub chars_after: usize,
}

fn total_chars(turns: &[ContextTurn]) -> usize {
    turns.iter().map(|t| t.content.len()).sum()
}

/// Deterministic mechanical summarizer used in tests and as a fallback when no LLM is wired.
pub fn mechanical_summarize(instruction: &str, turns: &[ContextTurn]) -> String {
    let mut out = format!("[compact: {instruction}] ");
    for turn in turns {
        let preview: String = turn.content.chars().take(120).collect();
        out.push_str(&format!("{}: {preview}; ", turn.role));
    }
    out
}

/// Compact `turns` using `summarize(instruction, older_turns)`.
///
/// Returns a summary of older turns plus the preserved recent tail. Fails closed after
/// `max_attempts` if the summarized form is not sufficiently smaller (thrashing guard).
pub fn compact_turns<F>(
    turns: &[ContextTurn],
    request: &CompactRequest,
    mut summarize: F,
) -> Result<CompactionResult, CompactionError>
where
    F: FnMut(&str, &[ContextTurn]) -> String,
{
    if turns.is_empty() {
        return Err(CompactionError::Empty);
    }

    let chars_before = total_chars(turns);
    if turns.len() <= request.keep_recent {
        return Ok(CompactionResult {
            summary: String::new(),
            recent_turns: turns.to_vec(),
            attempts_used: 0,
            chars_before,
            chars_after: chars_before,
        });
    }

    let split = turns.len().saturating_sub(request.keep_recent);
    let (older, recent) = turns.split_at(split);
    let recent_turns = recent.to_vec();
    let target_max = ((chars_before as f64) * (1.0 - request.min_reduction_ratio)) as usize;

    let mut attempts = 0usize;
    let mut summary = String::new();
    loop {
        attempts += 1;
        summary = summarize(&request.instruction, older);
        let chars_after = summary.len() + total_chars(&recent_turns);
        if chars_after < chars_before && chars_after <= target_max.max(1) {
            return Ok(CompactionResult {
                summary,
                recent_turns,
                attempts_used: attempts,
                chars_before,
                chars_after,
            });
        }
        if attempts >= request.max_attempts {
            return Err(CompactionError::Thrashing {
                attempts,
                last_chars: summary.len() + total_chars(&recent_turns),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turns(n: usize) -> Vec<ContextTurn> {
        (0..n)
            .map(|i| ContextTurn {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                content: format!("turn-{i}: {}", "x".repeat(200)),
            })
            .collect()
    }

    #[test]
    fn compacts_with_instruction() {
        let input = turns(6);
        let req = CompactRequest {
            instruction: "keep file names".into(),
            keep_recent: 2,
            max_attempts: 3,
            min_reduction_ratio: 0.10,
        };
        let result = compact_turns(&input, &req, mechanical_summarize).unwrap();
        assert!(result.chars_after < result.chars_before);
        assert_eq!(result.recent_turns.len(), 2);
        assert!(result.summary.contains("keep file names"));
    }

    #[test]
    fn thrashing_guard_fails_closed() {
        let input = turns(4);
        let req = CompactRequest {
            instruction: "noop".into(),
            keep_recent: 1,
            max_attempts: 2,
            min_reduction_ratio: 0.99,
        };
        let err = compact_turns(&input, &req, |instr, older| {
            // Summarizer that does not shrink enough → thrashing.
            older
                .iter()
                .map(|t| t.content.clone())
                .collect::<Vec<_>>()
                .join("\n")
                + instr
        })
        .unwrap_err();
        assert!(matches!(err, CompactionError::Thrashing { .. }));
    }
}

pub fn compact_context<F>(turns:&[ContextTurn],request:&CompactRequest,summarize:F)->Result<CompactionResult,CompactionError> where F:FnMut(&str,&[ContextTurn])->String,{compact_turns(turns,request,summarize)}
