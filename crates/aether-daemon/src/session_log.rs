//! Append-only JSONL session log — the source-of-truth transcript for a daemon turn
//! (Phase 9 slices 9.5-9.6).
//!
//! Every `plan`/`tool`/`observe`/`verify`/`budget`/`done`/`error` event a structured loop emits is
//! written here, in order, alongside the originating prompt. This is what lets a session's history
//! be reconstructed and verified (checkpoints, fork, replay, forensics) without re-running
//! inference — see `docs/ROADMAP_PHASES_9-13.md` Phase 9 slice 9.5-9.6 and Phase 10.

use aether_core::LoopStreamEvent;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Bump when the on-disk record shape changes. Readers must reject unknown versions rather than
/// guess — silent schema drift is exactly the kind of theater this project's docs warn against.
pub const SESSION_LOG_SCHEMA_VERSION: u32 = 3;

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
    #[serde(default)]
    pub prev_hash: String,
    #[serde(default)]
    pub content_hash: String,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Session ids can be attacker-influenced (IPC params, gateway/automation config). Never let one
/// escape the log directory via path separators or traversal segments.
fn session_file_key(session_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aether-session-file-v1\0");
    hasher.update(session_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn integrity_key() -> Vec<u8> {
    std::env::var("AETHER_LOG_INTEGRITY_KEY")
        .unwrap_or_else(|_| "aether-test-log-key".into())
        .into_bytes()
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut normalized = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        normalized[..digest.len()].copy_from_slice(&digest);
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; BLOCK];
    let mut outer_pad = [0x5cu8; BLOCK];
    for index in 0..BLOCK {
        inner_pad[index] ^= normalized[index];
        outer_pad[index] ^= normalized[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner.finalize());
    format!("{:x}", outer.finalize())
}

fn record_hash(
    key: &[u8],
    prev_hash: &str,
    session_id: &str,
    turn_index: u32,
    seq: u64,
    unix_ms: u128,
    payload: &SessionLogPayload,
) -> io::Result<String> {
    let payload = serde_json::to_vec(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(prev_hash.as_bytes());
    bytes.extend_from_slice(session_id.as_bytes());
    bytes.extend_from_slice(&turn_index.to_le_bytes());
    bytes.extend_from_slice(&seq.to_le_bytes());
    bytes.extend_from_slice(&unix_ms.to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(hmac_sha256_hex(key, &bytes))
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
    enabled: bool,
}

impl SessionLogWriter {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, enabled: true }
    }

    /// Resolves `AETHER_SESSION_LOG_DIR`, falling back to `~/.aether/sessions`. Production code
    /// (`task_runner::execute_structured_loop`) uses this; tests that need isolation should use
    /// [`SessionLogWriter::new`] with an explicit temp directory instead of mutating process env.
    pub fn from_env() -> Self {
        let enabled = std::env::var("AETHER_ENABLE_SESSION_LOGS")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            || std::env::var_os("AETHER_SESSION_LOG_DIR").is_some();
        Self {
            dir: default_log_dir(),
            enabled,
        }
    }

    pub fn purge_expired_logs(&self, retention_days: u32) -> io::Result<usize> {
        if !self.enabled || !self.dir.is_dir() {
            return Ok(0);
        }
        let cutoff = SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(
                retention_days.max(1) as u64 * 86_400,
            ))
            .unwrap_or(UNIX_EPOCH);
        let mut removed = 0;
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            if entry.metadata()?.modified().unwrap_or(SystemTime::now()) < cutoff {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    pub fn path_for_session(&self, session_id: &str) -> PathBuf {
        self.dir
            .join(format!("{}.jsonl", session_file_key(session_id)))
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
        if !self.enabled {
            return Ok(0);
        }
        fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        }
        let path = self.path_for_session(session_id);
        let existing = self.read_session_log(session_id).unwrap_or_default();
        let turn_index = existing
            .iter()
            .filter(|r| matches!(r.payload, SessionLogPayload::TurnStart { .. }))
            .count() as u32
            + 1;
        let mut seq = existing.len() as u64;

        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        let key = integrity_key();
        let mut prev_hash = existing
            .last()
            .map(|record| record.content_hash.clone())
            .unwrap_or_else(|| "GENESIS".into());

        let mut write_record = |seq: &mut u64, payload: SessionLogPayload| -> io::Result<()> {
            let timestamp = now_ms();
            let content_hash = record_hash(
                &key,
                &prev_hash,
                session_id,
                turn_index,
                *seq,
                timestamp,
                &payload,
            )?;
            let record = SessionLogRecord {
                schema_version: SESSION_LOG_SCHEMA_VERSION,
                session_id: session_id.to_string(),
                turn_index,
                seq: *seq,
                unix_ms: timestamp,
                payload,
                prev_hash: prev_hash.clone(),
                content_hash: content_hash.clone(),
            };
            prev_hash = content_hash;
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
        if !self.enabled {
            return Ok(());
        }
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
        let temporary = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
        fs::write(&temporary, buf)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(temporary, path)
    }

    /// Parse the full on-disk log for a session. Returns an empty vec if no log exists yet —
    /// "never ran" and "ran with zero events" are different states callers can still tell apart
    /// via the caller's own bookkeeping, but for read purposes both yield no records.
    pub fn read_session_log(&self, session_id: &str) -> io::Result<Vec<SessionLogRecord>> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        let path = self.path_for_session(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path)?;
        let mut records = Vec::with_capacity(content.lines().count());
        let key = integrity_key();
        let mut expected_prev = "GENESIS".to_string();
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
            if record.session_id != session_id || record.seq != records.len() as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session log owner or sequence mismatch",
                ));
            }
            let expected_hash = record_hash(
                &key,
                &expected_prev,
                &record.session_id,
                record.turn_index,
                record.seq,
                record.unix_ms,
                &record.payload,
            )?;
            if record.prev_hash != expected_prev || record.content_hash != expected_hash {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session log integrity verification failed",
                ));
            }
            expected_prev = record.content_hash.clone();
            records.push(record);
        }
        Ok(records)
    }
}

pub(crate) fn rechain_records(
    records: impl IntoIterator<Item = SessionLogRecord>,
    session_id: &str,
) -> io::Result<Vec<SessionLogRecord>> {
    let key = integrity_key();
    let mut previous = "GENESIS".to_string();
    let mut output = Vec::new();
    for (sequence, mut record) in records.into_iter().enumerate() {
        record.schema_version = SESSION_LOG_SCHEMA_VERSION;
        record.session_id = session_id.to_string();
        record.seq = sequence as u64;
        record.prev_hash = previous.clone();
        record.content_hash = record_hash(
            &key,
            &previous,
            session_id,
            record.turn_index,
            record.seq,
            record.unix_ms,
            &record.payload,
        )?;
        previous = record.content_hash.clone();
        output.push(record);
    }
    Ok(output)
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
                provider_input_tokens: 0,
                provider_output_tokens: 0,
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
