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

// --- P2-12: compaction is a trust-boundary crossing -------------------------------------------

/// What compaction may never summarize, and what a summary inherits from what it did summarize.
///
/// Compaction is the one operation in the loop that rewrites history, and it crosses a trust
/// boundary while doing it: untrusted observations go in, and what comes out is presented as
/// *context*. Left unguarded it does two damaging things. It can summarize away the policy preamble,
/// so a long session quietly loses the rules it started with; and it can launder injected content —
/// the poison loses its `<tool_result trust="untrusted">` wrapper, gets promoted into a summary, and
/// the correlation gate in [`crate::inject`] (which matches against observation content) can no
/// longer see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactPolicy {
    /// Roles kept byte for byte — the policy/preamble region. A compaction that can summarize the
    /// rules has not saved context, it has removed constraints.
    pub keep_verbatim_roles: Vec<String>,
    /// Substrings marking a turn's content as untrusted. Matched case-insensitively.
    pub untrusted_markers: Vec<String>,
    /// Emitted fresh at the top of the compacted context, so a compacted session still opens with
    /// what it may and may not do (re-anchoring). `None` keeps whatever the kept-verbatim region
    /// already carried.
    pub reanchor_preamble: Option<String>,
    /// Keep untrusted turns verbatim instead of summarizing them. Off by default: in a tool-heavy
    /// session most older turns *are* untrusted observations, and protecting all of them leaves
    /// nothing to compact. Trust inheritance is the default defence; this is the stricter option for
    /// a session known to be under attack.
    pub keep_untrusted_verbatim: bool,
}

impl Default for CompactPolicy {
    fn default() -> Self {
        Self {
            keep_verbatim_roles: vec!["system".to_string(), "policy".to_string()],
            untrusted_markers: vec![
                "trust=\"untrusted\"".to_string(),
                "<tool_result".to_string(),
                "<retrieved_memory".to_string(),
            ],
            reanchor_preamble: None,
            keep_untrusted_verbatim: false,
        }
    }
}

/// A compaction that kept its guarantees, and can prove it.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardedCompactResult {
    /// The underlying compaction: summary, recent tail, and its own char accounting.
    pub compact: CompactResult,
    /// Turns kept byte for byte — the protected region, in original order.
    pub kept_verbatim: Vec<ContextTurn>,
    /// Fresh preamble emitted at the top of [`GuardedCompactResult::render`], when the policy asked
    /// for one.
    pub preamble: Option<String>,
    /// `Some("untrusted")` when the summary was derived from untrusted content and therefore
    /// inherits that tier. `untrusted_compacted > 0` with this `None` is the laundering bug.
    pub inherited_trust: Option<String>,
    /// How many older turns were handed to the summarizer.
    pub summarized: usize,
    /// How many of those summarized turns were untrusted.
    pub untrusted_compacted: usize,
    /// Whether a fresh preamble was re-anchored.
    pub reanchored: bool,
    /// True char accounting, including the region kept verbatim (which `compact.chars_before` does
    /// not see).
    pub chars_before: usize,
    pub chars_after: usize,
}

impl GuardedCompactResult {
    /// The audit line: what compaction did, and whether the rules survived it.
    ///
    /// P2-12 asked for `Compacted { chars_before, chars_after, attempts, reanchored }`. `attempts`
    /// is not here because [`compact_turns`] returns only the successful attempt's `CompactResult` —
    /// the count exists solely inside `CompactionError::Thrashing` — and inventing a number the
    /// callee never surfaced would make the audit line the least trustworthy thing in it. What it
    /// does report is the split the guarantee depends on: how many older turns were summarized, how
    /// many were kept, and how many of the summarized ones were untrusted.
    pub fn log_line(&self) -> String {
        format!(
            "Compacted {{ chars_before: {}, chars_after: {}, summarized: {}, kept_verbatim: {}, \
             untrusted_compacted: {}, inherited_trust: {:?}, reanchored: {} }}",
            self.chars_before,
            self.chars_after,
            self.summarized,
            self.kept_verbatim.len(),
            self.untrusted_compacted,
            self.inherited_trust,
            self.reanchored
        )
    }

    /// The compacted context as it re-enters a prompt.
    ///
    /// Order is the point: the re-anchored preamble first, then the summary **inside the trust tier
    /// it inherited**, then everything kept verbatim, then the recent tail, then a bound report —
    /// because a compaction that does not say how much it collapsed is the P1-6 defect again.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(preamble) = &self.preamble {
            out.push_str(preamble);
            if !preamble.ends_with('\n') {
                out.push('\n');
            }
        }
        if !self.compact.summary.is_empty() {
            match &self.inherited_trust {
                Some(trust) => out.push_str(&format!(
                    "<compacted_context trust=\"{trust}\">\n{}\n</compacted_context>\n",
                    self.compact.summary
                )),
                None => out.push_str(&format!(
                    "<compacted_context>\n{}\n</compacted_context>\n",
                    self.compact.summary
                )),
            }
        }
        for turn in &self.kept_verbatim {
            out.push_str(&format!(
                "[{} | kept verbatim] {}\n",
                turn.role, turn.content
            ));
        }
        for turn in &self.compact.recent_turns {
            out.push_str(&format!("[{}] {}\n", turn.role, turn.content));
        }
        out.push_str(&format!(
            "[compacted: {} older turn(s) summarized into {} chars, {} turn(s) kept verbatim, \
             {} of {} chars remain]{}\n",
            self.summarized,
            self.compact.summary.chars().count(),
            self.kept_verbatim.len(),
            self.chars_after,
            self.chars_before,
            match &self.inherited_trust {
                Some(trust) => format!("; the summary inherits trust=\"{trust}\""),
                None => String::new(),
            }
        ));
        out
    }
}

fn is_untrusted_turn(turn: &ContextTurn, policy: &CompactPolicy) -> bool {
    if turn.role.eq_ignore_ascii_case("tool") {
        return true;
    }
    let lower = turn.content.to_ascii_lowercase();
    policy
        .untrusted_markers
        .iter()
        .any(|marker| lower.contains(&marker.to_ascii_lowercase()))
}

/// Compact with the trust boundary guarded (P2-12).
///
/// Three guarantees over [`compact_turns`]: the protected region is never handed to the summarizer,
/// a summary derived from untrusted turns inherits `trust="untrusted"`, and the result carries
/// enough accounting to prove both. The reduction/thrashing guard is reused unchanged, over exactly
/// the turns this policy allows to be summarized.
pub fn compact_turns_guarded<F>(
    turns: &[ContextTurn],
    req: &CompactRequest,
    policy: &CompactPolicy,
    summarize: F,
) -> Result<GuardedCompactResult, CompactionError>
where
    F: Fn(&str, &[ContextTurn]) -> String,
{
    if req.keep_recent == 0 {
        return Err(CompactionError::InvalidInput("keep_recent must be >= 1".into()));
    }
    if turns.len() <= req.keep_recent {
        return Err(CompactionError::InvalidInput(
            "nothing to compact beyond recent tail".into(),
        ));
    }

    let split = turns.len().saturating_sub(req.keep_recent);
    let older = &turns[..split];
    let chars_before = turn_chars(turns);

    let mut kept_verbatim: Vec<ContextTurn> = Vec::new();
    let mut compactable: Vec<ContextTurn> = Vec::new();
    for turn in older {
        let protected_role = policy
            .keep_verbatim_roles
            .iter()
            .any(|role| role.eq_ignore_ascii_case(&turn.role));
        let untrusted = is_untrusted_turn(turn, policy);
        if protected_role || (untrusted && policy.keep_untrusted_verbatim) {
            kept_verbatim.push(turn.clone());
        } else {
            compactable.push(turn.clone());
        }
    }
    if compactable.is_empty() {
        return Err(CompactionError::InvalidInput(
            "every older turn is protected by this policy (keep_verbatim role, or untrusted with \
             keep_untrusted_verbatim); nothing may be summarized"
                .into(),
        ));
    }

    let untrusted_compacted = compactable
        .iter()
        .filter(|turn| is_untrusted_turn(*turn, policy))
        .count();
    // A summary of untrusted content is untrusted content. Not negotiable: this is the line between
    // "compaction saved tokens" and "compaction promoted an injection into context".
    let inherited_trust = if untrusted_compacted > 0 {
        Some("untrusted".to_string())
    } else {
        None
    };

    let summarized = compactable.len();
    let mut summarizer_input = compactable;
    summarizer_input.extend_from_slice(&turns[split..]);
    let compact = compact_turns(&summarizer_input, req, summarize)?;

    let preamble = policy.reanchor_preamble.clone();
    let preamble_chars = preamble.as_ref().map(|p| p.chars().count()).unwrap_or(0);
    let reanchored = preamble.is_some();
    let chars_after = compact.summary.chars().count()
        + turn_chars(&kept_verbatim)
        + turn_chars(&compact.recent_turns)
        + preamble_chars;

    Ok(GuardedCompactResult {
        compact,
        kept_verbatim,
        preamble,
        inherited_trust,
        summarized,
        untrusted_compacted,
        reanchored,
        chars_before,
        chars_after,
    })
}

/// The compacted state, as the one observation a replan must be admitted against (P2-12 fix 4).
///
/// Compaction replaces the evidence a plan was admitted against. If admission is not re-run over the
/// compacted state, a replan induced by content that now survives *only inside the summary* is
/// judged against observations that no longer exist — and the correlation gate, which matches
/// observation text, sees nothing. Handing this observation back to
/// [`crate::inject::admit_plan_against_observations`] closes that: the summary is untrusted text
/// like any other tool result, so an induced step correlates with it exactly as it would have with
/// the raw observation.
pub fn compacted_observation(
    result: &GuardedCompactResult,
    iteration: usize,
) -> crate::loop_engine::ToolObservation {
    crate::loop_engine::ToolObservation {
        iteration,
        tool: "context_compact".to_string(),
        success: true,
        output: result.render(),
    }
}
