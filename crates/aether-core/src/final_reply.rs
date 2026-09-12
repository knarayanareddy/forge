//! The reply a finished run owes its requester (P1-8 / REPLY-01).
//!
//! Before this module the only thing a completed structured loop handed back was
//! `LoopRunResult::summary`, which is *the last observation* — for any plan ending in `done` that is
//! the literal string `"plan complete"`. A sign-off is not an answer: it names nothing that was
//! produced, so a caller (gateway channel, automation trigger, the SwiftUI app) either shows the
//! user "plan complete" or invents its own summary. Wave 1 gave the gateway path a private
//! `compose_gateway_reply`; this is that idea promoted to a contract with a pre-emit self-check, so
//! every execution path emits the same kind of reply and the invariant is assertable rather than
//! hoped for.
//!
//! The rules come from the reference prompt's reply discipline, restated as checkable invariants:
//!
//! 1. **state the answer, not the status** — a reply that is only a status token (`done`, `ok`,
//!    `complete`, `plan complete`) is rejected;
//! 2. **present what you produced** — every artifact the run wrote must be named in the reply,
//!    because an artifact nobody is told about is unreachable;
//! 3. **stay deliverable** — bounded length, with the artifact list protected from the bound (the
//!    step-by-step detail is what gets dropped first, and the drop is reported).
//!
//! [`FinalReply::compose`] satisfies all three by construction and then checks itself
//! ([`FinalReply::validate`]), which is what makes rule 1–3 a contract instead of a comment.

use crate::loop_engine::ToolObservation;

/// Ceiling on a reply's length. Long enough to state an answer plus the steps that produced it,
/// short enough that a 40-step run cannot generate an undeliverable message.
pub const MAX_FINAL_REPLY_CHARS: usize = 2_000;

/// Replies that sign off without answering. Compared case-insensitively against the trimmed text,
/// with trailing punctuation stripped, so `"Done."` and `"DONE"` are caught too.
const BARE_STATUS_REPLIES: [&str; 9] = [
    "done",
    "ok",
    "okay",
    "complete",
    "completed",
    "finished",
    "success",
    "plan complete",
    "loop finished",
];

/// The user-facing reply for one finished run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalReply {
    /// The answer: what was produced and what the steps concluded. Never empty, never a bare
    /// status token — see [`FinalReply::validate`].
    pub text: String,
    /// Workspace-relative paths this run wrote, in write order. These are the artifacts the text is
    /// required to present; an adapter delivers or opens them from here.
    pub artifacts: Vec<String>,
}

impl FinalReply {
    /// True when `text` is a sign-off rather than an answer.
    ///
    /// Public because the same vocabulary check is useful to callers that receive a summary from
    /// somewhere else (a model's own `done` text, an automation result) and need to know whether it
    /// can be shown to a person as-is.
    pub fn is_bare_status(text: &str) -> bool {
        let trimmed = text.trim().trim_end_matches(['.', '!', ' ']).to_ascii_lowercase();
        trimmed.is_empty() || BARE_STATUS_REPLIES.iter().any(|token| trimmed == *token)
    }

    /// Compose the reply for a finished run, then check it.
    ///
    /// `done_summary` is what the run itself offers as an answer (`LoopRunResult::summary`). It is
    /// used verbatim only when it actually answers — non-empty, not a bare status token, and
    /// accounting for every artifact. Otherwise the reply is derived from what the run produced,
    /// which is always possible: the observations and the written paths are facts about the run.
    ///
    /// A substantive summary that ignores an artifact keeps its own wording and gets the accounting
    /// appended — the model's answer is preferred, the invariant is not negotiable.
    pub fn compose(
        done_summary: &str,
        iterations: usize,
        observations: &[ToolObservation],
        artifacts: &[String],
    ) -> Self {
        let reply = Self::compose_unchecked(done_summary, iterations, observations, artifacts);
        let defects = reply.validate();
        if defects.is_empty() {
            return reply;
        }
        // Unreachable by construction: every branch below is written to satisfy `validate`. If a
        // future branch does not, the violation is stated in the reply instead of being emitted
        // silently — a self-check that cannot fail loudly is decoration.
        Self {
            text: bound(&format!(
                "{} [reply contract violated: {}]",
                reply.text,
                defects.join("; ")
            )),
            artifacts: reply.artifacts,
        }
    }

    fn compose_unchecked(
        done_summary: &str,
        iterations: usize,
        observations: &[ToolObservation],
        artifacts: &[String],
    ) -> Self {
        let owned: Vec<String> = artifacts.to_vec();
        let candidate = done_summary.trim();

        if !Self::is_bare_status(candidate) {
            let unnamed = unnamed_artifacts(candidate, &owned);
            if unnamed.is_empty() {
                // The run's own words already answer and already present everything.
                return Self {
                    text: bound(candidate),
                    artifacts: owned,
                };
            }
            // Substantive but incomplete. The candidate is held to half the budget so the
            // accounting it owes always fits (rule 2 outranks rule 3's completeness).
            let kept = bound_to(candidate, MAX_FINAL_REPLY_CHARS / 2);
            let headline = also_produced(&unnamed);
            return Self {
                text: with_steps(&format!("{kept}\n{headline}"), observations),
                artifacts: owned,
            };
        }

        let headline = produced_headline(iterations, &owned);
        Self {
            text: with_steps(&headline, observations),
            artifacts: owned,
        }
    }

    /// Pre-emit self-check. Empty means the reply is deliverable as an answer.
    ///
    /// Each defect is a sentence a caller can log verbatim, and each one is a rule from the module
    /// docs — so the list is the contract, expressed as data.
    pub fn validate(&self) -> Vec<String> {
        let mut defects = Vec::new();
        if self.text.trim().is_empty() {
            defects.push("the reply is empty; it must state what the run produced".to_string());
        }
        if Self::is_bare_status(&self.text) {
            defects.push(format!(
                "the reply is a status token, not an answer: {:?}",
                self.text.trim()
            ));
        }
        let unnamed = unnamed_artifacts(&self.text, &self.artifacts);
        if !unnamed.is_empty()
            && !self.text.contains(&format!("(+{} more", unnamed.len()))
        {
            defects.push(format!(
                "the reply does not present {} artifact(s): {}",
                unnamed.len(),
                unnamed.join(", ")
            ));
        }
        if self.text.chars().count() > MAX_FINAL_REPLY_CHARS {
            defects.push(format!(
                "the reply is {} chars, over the {} char bound",
                self.text.chars().count(),
                MAX_FINAL_REPLY_CHARS
            ));
        }
        defects
    }
}

/// Artifacts the text does not name, in write order.
fn unnamed_artifacts(text: &str, artifacts: &[String]) -> Vec<String> {
    artifacts
        .iter()
        .filter(|path| !text.contains(path.as_str()))
        .cloned()
        .collect()
}

/// First sentence of a derived reply: what the run did, and which paths it produced.
fn produced_headline(iterations: usize, artifacts: &[String]) -> String {
    if artifacts.is_empty() {
        return format!("Completed {iterations} step(s); produced no files.");
    }
    let prefix = format!(
        "Completed {iterations} step(s); produced {} file(s): ",
        artifacts.len()
    );
    listing(&prefix, artifacts)
}

/// Appended when a substantive summary ignored some of what the run wrote: the model's wording is
/// kept, the accounting it owes is added.
fn also_produced(unnamed: &[String]) -> String {
    listing("Also produced: ", unnamed)
}

/// Name as many of `paths` as fit after `prefix`, and account for the rest numerically.
///
/// `(+N more` is the form [`FinalReply::validate`] accepts in place of naming them, so an absurd
/// artifact list still yields a bounded reply that makes a checkable claim instead of silently
/// dropping paths.
fn listing(prefix: &str, paths: &[String]) -> String {
    let room = MAX_FINAL_REPLY_CHARS.saturating_sub(prefix.chars().count() + 32);
    let mut named: Vec<&str> = Vec::new();
    let mut used = 0usize;
    for path in paths {
        let cost = path.chars().count() + if named.is_empty() { 0 } else { 2 };
        if used + cost > room {
            break;
        }
        used += cost;
        named.push(path);
    }
    if named.is_empty() {
        // Even one path does not fit, which means the bound itself is the story.
        return bound(&format!(
            "{prefix}(+{} more artifacts than this reply can name)",
            paths.len()
        ));
    }
    let listed = named.join(", ");
    let overflow = paths.len() - named.len();
    if overflow == 0 {
        format!("{prefix}{listed}.")
    } else {
        format!("{prefix}{listed} (+{overflow} more).")
    }
}

/// Append the per-step conclusions after the headline, dropping the tail of the detail when the
/// bound bites — and saying how much was dropped.
///
/// The headline is never truncated to make room for step detail: it carries the artifacts, and rule
/// 2 outranks rule 3's completeness. Room for the "omitted" note is reserved up front so the note
/// itself cannot be what the bound cuts.
fn with_steps(headline: &str, observations: &[ToolObservation]) -> String {
    // Worst realistic note, `[9999 further step line(s) omitted]`, plus slack.
    const NOTE_RESERVE: usize = 48;

    let mut text = headline.to_string();
    let steps: Vec<&ToolObservation> = observations.iter().filter(|o| o.tool != "done").collect();
    let room = MAX_FINAL_REPLY_CHARS.saturating_sub(text.chars().count());
    let fill = room.saturating_sub(NOTE_RESERVE);
    let mut used = 0usize;
    let mut included = 0usize;
    for obs in &steps {
        let line = format!(
            "\n{} {}: {}",
            if obs.success { "ok" } else { "failed" },
            obs.tool,
            obs.output
        );
        if used + line.chars().count() > fill {
            break;
        }
        used += line.chars().count();
        text.push_str(&line);
        included += 1;
    }
    if included < steps.len() {
        text.push_str(&format!(
            "\n[{} further step line(s) omitted]",
            steps.len() - included
        ));
    }
    bound(&text)
}

/// Hard-bound a reply to [`MAX_FINAL_REPLY_CHARS`], marking the cut.
fn bound(text: &str) -> String {
    bound_to(text, MAX_FINAL_REPLY_CHARS)
}

/// Hard-bound a reply to `max` chars, marking the cut. A bounded reply that does not say it was
/// bounded is the same defect P1-6 fixed for reads.
fn bound_to(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    const MARK: &str = "\u{2026}[reply truncated]";
    let keep = max.saturating_sub(MARK.chars().count());
    let mut out: String = text.chars().take(keep).collect();
    out.push_str(MARK);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(tool: &str, success: bool, output: &str) -> ToolObservation {
        ToolObservation {
            iteration: 0,
            tool: tool.to_string(),
            success,
            output: output.to_string(),
        }
    }

    #[test]
    fn a_bare_status_sign_off_is_replaced_by_an_answer() {
        let reply = FinalReply::compose(
            "plan complete",
            4,
            &[
                obs("fs_write", true, "Wrote 21 bytes to hello.txt"),
                obs("verify_contains", true, "found"),
                obs("python_lint_file", true, "ok"),
                obs("done", true, "plan complete"),
            ],
            &["hello.txt".to_string()],
        );
        assert!(reply.validate().is_empty(), "{:?}", reply.validate());
        assert!(reply.text.contains("hello.txt"), "{}", reply.text);
        assert!(!FinalReply::is_bare_status(&reply.text));
        assert_eq!(reply.artifacts, vec!["hello.txt".to_string()]);
    }

    #[test]
    fn a_substantive_summary_is_kept_verbatim() {
        let summary = "Wrote hello.txt containing the greeting, verified and linted.";
        let reply = FinalReply::compose(
            summary,
            3,
            &[obs("done", true, "plan complete")],
            &["hello.txt".to_string()],
        );
        assert_eq!(reply.text, summary);
        assert!(reply.validate().is_empty(), "{:?}", reply.validate());
    }

    #[test]
    fn a_summary_that_ignores_an_artifact_gets_the_accounting_appended() {
        let reply = FinalReply::compose(
            "Both files are ready and linted clean.",
            6,
            &[obs("done", true, "plan complete")],
            &["a.txt".to_string(), "b.py".to_string()],
        );
        assert!(reply.text.starts_with("Both files are ready"));
        assert!(reply.text.contains("a.txt") && reply.text.contains("b.py"));
        assert!(reply.validate().is_empty(), "{:?}", reply.validate());
    }

    #[test]
    fn the_self_check_has_teeth() {
        // compose() cannot produce these, so they are built by hand: the point is that validate()
        // names the rule that was broken rather than returning an empty list by default.
        let bare = FinalReply {
            text: "Done.".to_string(),
            artifacts: vec!["out.txt".to_string()],
        };
        let defects = bare.validate();
        assert_eq!(defects.len(), 2, "{defects:?}");
        assert!(defects[0].contains("status token"), "{defects:?}");
        assert!(defects[1].contains("out.txt"), "{defects:?}");

        let empty = FinalReply {
            text: "   ".to_string(),
            artifacts: vec![],
        };
        assert!(empty.validate().iter().any(|d| d.contains("empty")));

        let long = FinalReply {
            text: "x".repeat(MAX_FINAL_REPLY_CHARS + 1),
            artifacts: vec![],
        };
        assert!(long.validate().iter().any(|d| d.contains("over the")));
    }

    #[test]
    fn status_vocabulary_covers_punctuation_and_case() {
        for token in ["done", "Done.", "DONE!", " ok ", "plan complete", "loop finished"] {
            assert!(FinalReply::is_bare_status(token), "{token:?}");
        }
        for answer in ["done — wrote hello.txt", "ok: 3 files", "completed the migration of a.txt"] {
            assert!(!FinalReply::is_bare_status(answer), "{answer:?}");
        }
    }

    #[test]
    fn a_read_only_run_answers_with_what_it_read() {
        let reply = FinalReply::compose(
            "line 1 of notes.txt\n[truncated from the middle: showing 334 head]",
            2,
            &[obs("fs_read", true, "line 1 of notes.txt")],
            &[],
        );
        assert!(reply.validate().is_empty(), "{:?}", reply.validate());
        assert!(reply.text.contains("notes.txt"));
        assert!(reply.artifacts.is_empty());
    }

    #[test]
    fn an_absurd_artifact_list_stays_inside_the_bound_and_accounts_numerically() {
        let artifacts: Vec<String> = (0..400).map(|i| format!("dir{i}/artifact-{i}.txt")).collect();
        let reply = FinalReply::compose("plan complete", 401, &[], &artifacts);
        assert!(reply.validate().is_empty(), "{:?}", reply.validate());
        assert!(reply.text.chars().count() <= MAX_FINAL_REPLY_CHARS);
        assert!(reply.text.contains("produced 400 file(s)"));
        assert!(reply.text.contains("(+"), "{}", reply.text);
        assert_eq!(reply.artifacts.len(), 400);
    }

    #[test]
    fn step_detail_is_dropped_before_artifacts_and_says_so() {
        let observations: Vec<ToolObservation> = (0..60)
            .map(|i| obs("fs_write", true, &format!("Wrote {} bytes to file{i}.txt", 40 + i)))
            .collect();
        let artifacts: Vec<String> = (0..3).map(|i| format!("file{i}.txt")).collect();
        let reply = FinalReply::compose("plan complete", 61, &observations, &artifacts);
        assert!(reply.validate().is_empty(), "{:?}", reply.validate());
        for path in &artifacts {
            assert!(reply.text.contains(path), "missing {path}");
        }
        assert!(reply.text.chars().count() <= MAX_FINAL_REPLY_CHARS);
    }
}
