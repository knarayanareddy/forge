//! Session resume / fork / side-branch over JSONL (Phase 10 slice 10.2 / FORK-01).

use crate::session_log::{SessionLogPayload, SessionLogRecord, SessionLogWriter};
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkReport {
    pub source_session_id: String,
    pub fork_session_id: String,
    pub turns_copied: u32,
    pub records_written: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeSnapshot {
    pub session_id: String,
    pub turn_count: u32,
    pub last_prompt: Option<String>,
}

fn count_turns(records: &[SessionLogRecord]) -> u32 {
    records
        .iter()
        .filter(|r| matches!(r.payload, SessionLogPayload::TurnStart { .. }))
        .count() as u32
}

pub fn fork_session_at_turn(
    source_session_id: &str,
    fork_session_id: &str,
    keep_turns: u32,
) -> Result<ForkReport, String> {
    if source_session_id == fork_session_id {
        return Err("fork target must differ from source session id".into());
    }
    let writer = SessionLogWriter::from_env();
    let source = writer
        .read_session_log(source_session_id)
        .map_err(|e| e.to_string())?;
    if source.is_empty() {
        return Err(format!("source session {source_session_id} has no log to fork"));
    }
    let turns_available = count_turns(&source);
    if keep_turns > turns_available {
        return Err(format!(
            "keep_turns {keep_turns} exceeds source turn count {turns_available}"
        ));
    }

    let kept: Vec<SessionLogRecord> = source
        .iter()
        .filter(|r| r.turn_index <= keep_turns)
        .enumerate()
        .map(|(seq, record)| SessionLogRecord {
            schema_version: record.schema_version,
            session_id: fork_session_id.to_string(),
            turn_index: record.turn_index,
            seq: seq as u64,
            unix_ms: record.unix_ms,
            payload: record.payload.clone(),
        })
        .collect();

    let path = writer.path_for_session(fork_session_id);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    if !kept.is_empty() {
        let mut buf = String::new();
        for record in &kept {
            let line = serde_json::to_string(record).map_err(|e| e.to_string())?;
            buf.push_str(&line);
            buf.push('\n');
        }
        std::fs::write(&path, buf).map_err(|e| e.to_string())?;
    }

    Ok(ForkReport {
        source_session_id: source_session_id.to_string(),
        fork_session_id: fork_session_id.to_string(),
        turns_copied: keep_turns,
        records_written: kept.len(),
    })
}

pub fn side_branch_from_turn(
    parent_session_id: &str,
    branch_session_id: &str,
    at_turn: u32,
) -> Result<ForkReport, String> {
    fork_session_at_turn(parent_session_id, branch_session_id, at_turn)
}

pub fn resume_snapshot(session_id: &str) -> Result<ResumeSnapshot, String> {
    let writer = SessionLogWriter::from_env();
    let records = writer.read_session_log(session_id).map_err(|e| e.to_string())?;
    let last_prompt = records
        .iter()
        .filter(|r| matches!(r.payload, SessionLogPayload::TurnStart { .. }))
        .last()
        .and_then(|r| match &r.payload {
            SessionLogPayload::TurnStart { prompt } => Some(prompt.clone()),
            _ => None,
        });
    Ok(ResumeSnapshot {
        session_id: session_id.to_string(),
        turn_count: count_turns(&records),
        last_prompt,
    })
}
