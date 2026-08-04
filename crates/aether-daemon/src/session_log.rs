//! Append-only JSONL session log — the source-of-truth transcript for a daemon turn
//! (Phase 9 slices 9.5-9.6).
//!
//! Every `plan`/`tool`/`observe`/`verify`/`budget`/`done`/`error` event a structured loop emits is
//! written here, in order, alongside the originating prompt. This is what lets a session's history
//! be reconstructed and verified (checkpoints, fork, replay, forensics) without re-running
//! inference — see `docs/ROADMAP_PHASES_9-13.md` Phase 9 slice 9.5-9.6 and Phase 10.

use aether_core::LoopStreamEvent;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Bump when the on-disk record shape changes. Readers must reject unknown versions rather than
/// guess — silent schema drift is exactly the kind of theater this project's docs warn against.
pub const SESSION_LOG_SCHEMA_VERSION: u32 = 2;

/// One entry in a session's JSONL transcript. `TurnStart` brackets every other payload so a
/// session log with N turns always contains exactly N `TurnStart` records.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionLogPayload {
    TurnStart { prompt: String },
    Plan { iteration: usize, action: String },
    Tool { iteration: usize, tool: String, output: String },
    Observe { iteration: usize, summary: String },
    Verify { iteration: usize, passed: bool, detail: String },
    Budget { iteration: usize, max_iterations: usize, tokens_used: usize, max_tokens: usize, provider_input_tokens: usize, provider_output_tokens: usize, },
    ProviderTokens { source: String, input_tokens: usize, output_tokens: usize, tokens_used: usize, iteration: Option<usize>, },
    Done { iterations: usize, summary: String, tokens_used: usize, provider_input_tokens: usize, provider_output_tokens: usize, },
    Error { message: String },
}

impl From<&LoopStreamEvent> for SessionLogPayload {
    fn from(event: &LoopStreamEvent) -> Self {
        match event {
            LoopStreamEvent::Plan { iteration, action } => SessionLogPayload::Plan {
                iteration: *iteration,
                action: action.clone(),
            },
            LoopStreamEvent::Tool {
                iteration,
                tool,
                output,
            } => SessionLogPayload::Tool {
                iteration: *iteration,
                tool: tool.clone(),
                output: output.clone(),
            },
            LoopStreamEvent::Observe { iteration, summary } => SessionLogPayload::Observe {
                iteration: *iteration,
                summary: summary.clone(),
            },
            LoopStreamEvent::Verify {
                iteration,
                passed,
                detail,
            } => SessionLogPayload::Verify {
                iteration: *iteration,
                passed: *passed,
                detail: detail.clone(),
            },
            LoopStreamEvent::Budget { iteration, max_iterations, tokens_used, max_tokens, provider_input_tokens, provider_output_tokens, } => SessionLogPayload::Budget { iteration: *iteration, max_iterations: *max_iterations, tokens_used: *tokens_used, max_tokens: *max_tokens, provider_input_tokens: *provider_input_tokens, provider_output_tokens: *provider_output_tokens, },
            LoopStreamEvent::ProviderTokens { source, input_tokens, output_tokens, tokens_used, iteration, } => SessionLogPayload::ProviderTokens { source: source.clone(), input_tokens: *input_tokens, output_tokens: *output_tokens, tokens_used: *tokens_used, iteration: *iteration, },
            LoopStreamEvent::Done { iterations, summary, tokens_used, provider_input_tokens, provider_output_tokens, } => SessionLogPayload::Done { iterations: *iterations, summary: summary.clone(), tokens_used: *tokens_used, provider_input_tokens: *provider_input_tokens, provider_output_tokens: *provider_output_tokens, },
            LoopStreamEvent::Error { message } => SessionLogPayload::Error {
                message: message.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionLogRecord {
    pub schema_version: u32,
    pub session_id: String,
    /// 1-based index of the turn this record belongs to within the session.
    pub turn_index: u32,
    /// Strictly increasing within a session's log file; the on-disk write order.
    pub seq: u64,
    pub unix_ms: u128,
    pub payload: SessionLogPayload,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Session ids can be attacker-influenced (IPC params, gateway/automation config). Never let one
/// escape the log directory via path separators or traversal segments.
fn sanitize_session_id(session_id: &str) -> String {
    let cleaned: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unknown-session".to_string()
    } else {
        cleaned
    }
}

pub fn default_log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("AETHER_SESSION_LOG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    Path::new(&home).join(".aether").join("sessions")
}

pub struct SessionLogWriter {
    dir: PathBuf,
}

impl SessionLogWriter {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Resolves `AETHER_SESSION_LOG_DIR`, falling back to `~/.aether/sessions`. Production code
    /// (`task_runner::execute_structured_loop`) uses this; tests that need isolation should use
    /// [`SessionLogWriter::new`] with an explicit temp directory instead of mutating process env.
    pub fn from_env() -> Self {
        Self::new(default_log_dir())
    }

    pub fn path_for_session(&self, session_id: &str) -> PathBuf {
        self.dir
            .join(format!("{}.jsonl", sanitize_session_id(session_id)))
    }

    /// Append one turn (a `TurnStart` record followed by every emitted event, in order) to the
    /// session's log file, creating it if absent. Returns the 1-based turn index used.
    ///
    /// Reads the existing log to compute the next `turn_index`/`seq` before appending — O(session
    /// length) per call. Acceptable at MVP scale; a persistent counter or index file is the
    /// natural follow-up once sessions grow large (see `docs/ROADMAP_PHASES_9-13.md` Phase 10).
    pub fn append_turn(
        &self,
        session_id: &str,
        prompt: &str,
        events: &[LoopStreamEvent],
    ) -> io::Result<u32> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path_for_session(session_id);
        let existing = self.read_session_log(session_id).unwrap_or_default();
        let turn_index = existing
            .iter()
            .filter(|r| matches!(r.payload, SessionLogPayload::TurnStart { .. }))
            .count() as u32
            + 1;
        let mut seq = existing.len() as u64;

        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;

        let mut write_record = |seq: &mut u64, payload: SessionLogPayload| -> io::Result<()> {
            let record = SessionLogRecord {
                schema_version: SESSION_LOG_SCHEMA_VERSION,
                session_id: session_id.to_string(),
                turn_index,
                seq: *seq,
                unix_ms: now_ms(),
                payload,
            };
            let line = serde_json::to_string(&record)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            writeln!(file, "{line}")?;
            *seq += 1;
            Ok(())
        };

        write_record(
            &mut seq,
            SessionLogPayload::TurnStart {
                prompt: prompt.to_string(),
            },
        )?;
        for event in events {
            write_record(&mut seq, SessionLogPayload::from(event))?;
        }

        Ok(turn_index)
    }

    /// Truncate a session's log back to its first `keep_turns` turns, dropping every record from
    /// later turns. Used by checkpoint rewind (Phase 10 slice 10.1 / CKPT-01) to keep the session
    /// log consistent with the files a rewind restores — a checkpoint is only meaningful if both
    /// the filesystem and the transcript agree on what happened.
    ///
    /// `keep_turns = 0` empties the log entirely (matching "never ran" read semantics). A missing
    /// log is a no-op, not an error.
    pub fn truncate_after_turn(&self, session_id: &str, keep_turns: u32) -> io::Result<()> {
        let path = self.path_for_session(session_id);
        if !path.exists() {
            return Ok(());
        }
        let records = self.read_session_log(session_id)?;
        let kept: Vec<&SessionLogRecord> = records
            .iter()
            .filter(|r| r.turn_index <= keep_turns)
            .collect();

        let mut buf = String::new();
        for record in kept {
            let line = serde_json::to_string(record)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            buf.push_str(&line);
            buf.push('\n');
        }
        fs::write(&path, buf)
    }

    /// Parse the full on-disk log for a session. Returns an empty vec if no log exists yet —
    /// "never ran" and "ran with zero events" are different states callers can still tell apart
    /// via the caller's own bookkeeping, but for read purposes both yield no records.
    pub fn read_session_log(&self, session_id: &str) -> io::Result<Vec<SessionLogRecord>> {
        let path = self.path_for_session(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path)?;
        let mut records = Vec::with_capacity(content.lines().count());
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let record: SessionLogRecord = serde_json::from_str(line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            if record.schema_version != SESSION_LOG_SCHEMA_VERSION {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "unsupported session log schema_version {} (expected {})",
                        record.schema_version, SESSION_LOG_SCHEMA_VERSION
                    ),
                ));
            }
            records.push(record);
        }
        Ok(records)
    }
}

/// Reconstruct the ordered tool-invocation trajectory purely from a parsed log — no re-execution
/// or inference required. This is the property session logs must have to be useful for replay,
/// forensics, and eval: the on-disk record alone is sufficient to recover what actually ran.
pub fn trajectory_from_log(records: &[SessionLogRecord]) -> Vec<String> {
    records
        .iter()
        .filter_map(|record| match &record.payload {
            SessionLogPayload::Tool { tool, .. } => Some(tool.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_events() -> Vec<LoopStreamEvent> {
        vec![
            LoopStreamEvent::Plan {
                iteration: 1,
                action: "fs_write".into(),
            },
            LoopStreamEvent::Tool {
                iteration: 1,
                tool: "fs_write".into(),
                output: "wrote 5 bytes".into(),
            },
            LoopStreamEvent::Observe {
                iteration: 1,
                summary: "wrote 5 bytes".into(),
            },
            LoopStreamEvent::Done {
                iterations: 1,
                summary: "wrote 5 bytes".into(),
                tokens_used: 3,
            },
        ]
    }

    #[test]
    fn append_and_read_round_trips_records_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        let turn = writer
            .append_turn("sess-a", "do the thing", &sample_events())
            .unwrap();
        assert_eq!(turn, 1);

        let records = writer.read_session_log("sess-a").unwrap();
        assert_eq!(records.len(), 5); // TurnStart + 4 events
        assert!(matches!(
            records[0].payload,
            SessionLogPayload::TurnStart { .. }
        ));
        assert!(matches!(
            records.last().unwrap().payload,
            SessionLogPayload::Done { .. }
        ));
        for window in records.windows(2) {
            assert!(window[1].seq > window[0].seq);
        }
        assert!(records.iter().all(|r| r.schema_version == SESSION_LOG_SCHEMA_VERSION));
    }

    #[test]
    fn second_turn_appends_and_increments_turn_index() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        writer.append_turn("sess-b", "first", &sample_events()).unwrap();
        let second_turn = writer
            .append_turn("sess-b", "second", &sample_events())
            .unwrap();
        assert_eq!(second_turn, 2);

        let records = writer.read_session_log("sess-b").unwrap();
        assert_eq!(records.len(), 10);
        let turn_indices: std::collections::HashSet<u32> =
            records.iter().map(|r| r.turn_index).collect();
        assert_eq!(turn_indices, std::collections::HashSet::from([1, 2]));
    }

    #[test]
    fn trajectory_from_log_extracts_tool_names_only() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        writer.append_turn("sess-c", "goal", &sample_events()).unwrap();
        let records = writer.read_session_log("sess-c").unwrap();
        assert_eq!(trajectory_from_log(&records), vec!["fs_write".to_string()]);
    }

    #[test]
    fn session_id_cannot_escape_log_directory() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        let malicious = "../../etc/cron.d/evil";
        let path = writer.path_for_session(malicious);
        // The path separators inside the session id must not survive into a real path component —
        // otherwise the "sanitized" name would still walk back out of `dir`.
        assert_eq!(path.parent().unwrap(), dir.path());
        assert!(path.starts_with(dir.path()));
        assert_eq!(
            path.components().count(),
            dir.path().components().count() + 1,
            "sanitized session id must not introduce extra path components"
        );
    }

    #[test]
    fn missing_log_reads_as_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        assert_eq!(writer.read_session_log("never-ran").unwrap(), Vec::new());
    }

    #[test]
    fn truncate_after_turn_drops_only_later_turns() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        writer.append_turn("sess-trunc", "first", &sample_events()).unwrap();
        writer.append_turn("sess-trunc", "second", &sample_events()).unwrap();
        writer.append_turn("sess-trunc", "third", &sample_events()).unwrap();

        writer.truncate_after_turn("sess-trunc", 1).unwrap();

        let records = writer.read_session_log("sess-trunc").unwrap();
        assert_eq!(records.len(), 5); // just turn 1's TurnStart + 4 events
        assert!(records.iter().all(|r| r.turn_index == 1));

        // A subsequent turn must append as turn 2, not turn 4 — truncation must actually rewrite
        // the on-disk turn count the next writer sees, not just hide old records from this reader.
        let next_turn = writer.append_turn("sess-trunc", "fourth", &sample_events()).unwrap();
        assert_eq!(next_turn, 2);
    }

    #[test]
    fn truncate_after_turn_zero_empties_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        writer.append_turn("sess-trunc-zero", "first", &sample_events()).unwrap();

        writer.truncate_after_turn("sess-trunc-zero", 0).unwrap();

        assert_eq!(writer.read_session_log("sess-trunc-zero").unwrap(), Vec::new());
    }

    #[test]
    fn truncate_after_turn_on_missing_log_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        writer.truncate_after_turn("never-existed", 3).unwrap();
        assert_eq!(writer.read_session_log("never-existed").unwrap(), Vec::new());
    }

    #[test]
    fn error_events_are_logged_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let writer = SessionLogWriter::new(dir.path().to_path_buf());
        let events = vec![LoopStreamEvent::Error {
            message: "verify_contains failed".into(),
        }];
        writer.append_turn("sess-err", "bad plan", &events).unwrap();
        let records = writer.read_session_log("sess-err").unwrap();
        assert!(records
            .iter()
            .any(|r| matches!(&r.payload, SessionLogPayload::Error { message } if message == "verify_contains failed")));
    }
}
