//! Context compaction (Phase 10 slice 10.5 / COMPACT-01).

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextTurn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct CompactRequest {
    pub instruction: String,
    pub max_attempts: usize,
    pub min_reduction_ratio: f64,
    pub keep_recent: usize,
}

impl Default for CompactRequest {
    fn default() -> Self {
        Self {
            instruction: String::new(),
            max_attempts: 3,
            min_reduction_ratio: 0.15,
            keep_recent: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompactResult {
    pub summary: String,
    pub recent_turns: Vec<ContextTurn>,
    pub chars_before: usize,
    pub chars_after: usize,
}

#[derive(Debug, Error, PartialEq)]
pub enum CompactionError {
    #[error("compaction thrashing after {attempts} attempts (ratio {last_ratio:.3} < {required:.3})")]
    Thrashing {
        attempts: usize,
        last_ratio: f64,
        required: f64,
    },
    #[error("invalid compaction input: {0}")]
    InvalidInput(String),
}

fn turn_chars(turns: &[ContextTurn]) -> usize {
    turns.iter().map(|t| t.content.chars().count()).sum()
}

pub fn mechanical_summarize(instruction: &str, older: &[ContextTurn]) -> String {
    format!(
        "Compact summary ({instruction}): {} older turns, {} chars",
        older.len(),
        turn_chars(older)
    )
}

pub fn compact_turns<F>(
    turns: &[ContextTurn],
    req: &CompactRequest,
    summarize: F,
) -> Result<CompactResult, CompactionError>
where
    F: Fn(&str, &[ContextTurn]) -> String,
{
    if req.keep_recent == 0 {
        return Err(CompactionError::InvalidInput(
            "keep_recent must be >= 1".into(),
        ));
    }
    if turns.len() <= req.keep_recent {
        return Err(CompactionError::InvalidInput(
            "nothing to compact beyond recent tail".into(),
        ));
    }

    let split = turns.len().saturating_sub(req.keep_recent);
    let older = &turns[..split];
    let recent = turns[split..].to_vec();
    let chars_before = turn_chars(turns);

    let mut last_ratio = 0.0f64;
    for attempt in 1..=req.max_attempts.max(1) {
        let summary = summarize(&req.instruction, older);
        let chars_after = summary.chars().count() + turn_chars(&recent);
        last_ratio = if chars_before == 0 {
            0.0
        } else {
            1.0 - (chars_after as f64 / chars_before as f64)
        };

        if last_ratio + f64::EPSILON >= req.min_reduction_ratio {
            return Ok(CompactResult {
                summary,
                recent_turns: recent,
                chars_before,
                chars_after,
            });
        }

        let _ = attempt;
    }

    Err(CompactionError::Thrashing {
        attempts: req.max_attempts.max(1),
        last_ratio,
        required: req.min_reduction_ratio,
    })
}
