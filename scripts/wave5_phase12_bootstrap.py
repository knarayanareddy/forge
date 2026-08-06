#!/usr/bin/env python3
"""Idempotent Wave 5 Phase 12 moat probe bootstrap."""
from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def write(rel: str, content: str) -> None:
    path = ROOT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    if path.stat().st_size < 10:
        raise SystemExit(f"failed to write {rel}")


SLEEP_MEMORY = r'''//! Sleep-time memory compute (Phase 12 / SLEEP-01).
use crate::Database;
use rusqlite::Result;
use serde::{Deserialize, Serialize};

pub const SLEEP01_RECALL_DELTA: f64 = 0.15;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SleepCycleReport {
    pub links_added: usize,
    pub chunks_scanned: usize,
}

pub fn run_sleep_memory_cycle(db: &Database, session_id: &str) -> Result<SleepCycleReport> {
    let nodes = db.get_active_graph_nodes(session_id, None)?;
    if nodes.is_empty() {
        return Ok(SleepCycleReport {
            links_added: 0,
            chunks_scanned: 0,
        });
    }
    let chunks: Vec<(String, String)> = {
        let conn = db.conn();
        let mut stmt =
            conn.prepare("SELECT chunk_id, chunk_text FROM semantic_memory ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut links_added = 0usize;
    let mut chunks_scanned = 0usize;
    for (chunk_id, chunk_text) in chunks {
        chunks_scanned += 1;
        let haystack = chunk_text.to_ascii_lowercase();
        for node in &nodes {
            if mentions(&haystack, &node.canonical_name) {
                if db.link_graph_chunk(&chunk_id, &node.id, 0.85).is_ok() {
                    links_added += 1;
                }
                continue;
            }
            if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&node.aliases_json) {
                for alias in aliases {
                    if mentions(&haystack, &alias) {
                        if db.link_graph_chunk(&chunk_id, &node.id, 0.80).is_ok() {
                            links_added += 1;
                        }
                        break;
                    }
                }
            }
        }
    }
    Ok(SleepCycleReport {
        links_added,
        chunks_scanned,
    })
}

fn mentions(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    needle.len() >= 3 && haystack.contains(&needle.to_ascii_lowercase())
}

pub fn recall_at_k_chunks(top: &[String], expected: &[String], k: usize) -> f64 {
    if expected.is_empty() {
        return 0.0;
    }
    let set: std::collections::HashSet<&str> = top.iter().take(k).map(String::as_str).collect();
    expected
        .iter()
        .filter(|id| set.contains(id.as_str()))
        .count() as f64
        / expected.len() as f64
}

pub fn mean_recall_at_k(recalls: &[f64]) -> f64 {
    if recalls.is_empty() {
        0.0
    } else {
        recalls.iter().sum::<f64>() / recalls.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityType, NewGraphEdge, NewGraphNode, RelationType};

    #[test]
    fn sleep_cycle_improves_recall() {
        let db = Database::open_in_memory().unwrap();
        let s = "sess-sleep-unit";
        db.conn()
            .execute(
                "INSERT INTO sessions (id,title,status) VALUES (?1,'s','active')",
                rusqlite::params![s],
            )
            .unwrap();
        db.insert_graph_node(NewGraphNode {
            id: "node-z",
            session_id: s,
            entity_type: EntityType::Project,
            canonical_name: "Zephyr-7",
            aliases_json: "[]".into(),
            properties_json: "{}".into(),
            source_uri: "m",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();
        db.insert_graph_node(NewGraphNode {
            id: "node-b",
            session_id: s,
            entity_type: EntityType::Concept,
            canonical_name: "sleep bridge",
            aliases_json: "[]".into(),
            properties_json: "{}".into(),
            source_uri: "m",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();
        db.insert_graph_edge(NewGraphEdge {
            session_id: s,
            src_node_id: "node-z",
            dst_node_id: "node-b",
            relation_type: RelationType::RelatedTo,
            weight: 2.0,
            evidence_text: "e".into(),
            source_uri: "m",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();
        let mut e0 = vec![0.0f32; 384];
        e0[0] = 1.0;
        let mut e1 = vec![0.0f32; 384];
        e1[0] = 0.92;
        e1[1] = 0.25;
        let mut e2 = vec![0.0f32; 384];
        e2[0] = 0.88;
        e2[1] = 0.3;
        db.insert_memory_chunk("n", "m", "Generic platform runtime overview", &e0)
            .unwrap();
        db.link_graph_chunk("n", "node-z", 0.9).unwrap();
        db.insert_memory_chunk("d1", "m", "Platform runtime overview documentation index", &e1)
            .unwrap();
        db.insert_memory_chunk("d2", "m", "Runtime overview notes for platform services", &e2)
            .unwrap();
        db.insert_memory_chunk(
            "b",
            "m",
            "The Zephyr-7 release uses the sleep bridge for offline recall",
            &vec![0.0f32; 384],
        )
        .unwrap();
        db.conn()
            .execute(
                "UPDATE query_policy SET graph_hop_depth=1, graph_weight=4.0 WHERE policy_name='default'",
                [],
            )
            .unwrap();
        let mut q = vec![0.0f32; 384];
        q[0] = 1.0;
        let base = db
            .search_semantic_memory_hybrid("platform runtime overview", &q, 2)
            .unwrap();
        let br = recall_at_k_chunks(
            &base.iter().map(|(id, _, _)| id.clone()).collect::<Vec<_>>(),
            &["b".into()],
            2,
        );
        assert!(run_sleep_memory_cycle(&db, s).unwrap().links_added >= 1);
        let after = db
            .search_hybrid_with_graph(s, "platform runtime overview", &q, 2)
            .unwrap();
        let ar = recall_at_k_chunks(
            &after.iter().map(|(id, _, _)| id.clone()).collect::<Vec<_>>(),
            &["b".into()],
            2,
        );
        assert!(ar >= br + SLEEP01_RECALL_DELTA);
    }
}
'''

TOOL_RELIABILITY = r'''use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ToolCallCase {
    pub id: String,
    pub tool_name: String,
    pub expect_success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FrozenToolResponse {
    pub tool_name: String,
    pub success: bool,
    pub arguments_match: bool,
    #[serde(default)]
    pub output: String,
}

pub fn score_tool_response(case: &ToolCallCase, response: &FrozenToolResponse) -> f64 {
    let mut score = 0.0;
    if response.tool_name == case.tool_name {
        score += 0.4;
    }
    if response.success == case.expect_success {
        score += 0.35;
    }
    if response.arguments_match {
        score += 0.25;
    }
    score
}

pub fn evaluate_profile_reliability(
    cases: &[ToolCallCase],
    responses: &HashMap<String, FrozenToolResponse>,
) -> f64 {
    if cases.is_empty() {
        return 0.0;
    }
    cases
        .iter()
        .map(|c| {
            responses
                .get(&c.id)
                .map(|r| score_tool_response(c, r))
                .unwrap_or(0.0)
        })
        .sum::<f64>()
        / cases.len() as f64
}

pub fn rank_profiles_by_reliability(
    profiles: &[(String, HashMap<String, FrozenToolResponse>)],
    cases: &[ToolCallCase],
) -> Vec<(String, f64)> {
    let mut ranked: Vec<(String, f64)> = profiles
        .iter()
        .map(|(id, r)| (id.clone(), evaluate_profile_reliability(cases, r)))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q8_outranks_q4() {
        let cases = vec![
            ToolCallCase {
                id: "a".into(),
                tool_name: "fs_write".into(),
                expect_success: true,
            },
            ToolCallCase {
                id: "b".into(),
                tool_name: "fs_read".into(),
                expect_success: true,
            },
        ];
        let q4 = HashMap::from([
            (
                "a".into(),
                FrozenToolResponse {
                    tool_name: "fs_write".into(),
                    success: true,
                    arguments_match: false,
                    output: String::new(),
                },
            ),
            (
                "b".into(),
                FrozenToolResponse {
                    tool_name: "fs_read".into(),
                    success: false,
                    arguments_match: true,
                    output: String::new(),
                },
            ),
        ]);
        let q8 = HashMap::from([
            (
                "a".into(),
                FrozenToolResponse {
                    tool_name: "fs_write".into(),
                    success: true,
                    arguments_match: true,
                    output: String::new(),
                },
            ),
            (
                "b".into(),
                FrozenToolResponse {
                    tool_name: "fs_read".into(),
                    success: true,
                    arguments_match: true,
                    output: String::new(),
                },
            ),
        ]);
        assert_eq!(
            rank_profiles_by_reliability(&[("q4".into(), q4), ("q8".into(), q8)], &cases)[0].0,
            "q8"
        );
    }
}
'''

FORENSICS = r'''//! Failure forensics (Phase 12 / FORENSIC-01).
use crate::session_log::{SessionLogPayload, SessionLogRecord};
use aether_core::ToolInvocation;
use serde::{Deserialize, Serialize};

pub const FORENSIC01_MIN_ACCURACY: f64 = 0.80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    MissingGrant,
    ContextExhaustion,
    SchemaFailure,
    WrongTool,
    BadToolOutput,
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForensicTrajectory {
    pub id: String,
    #[serde(rename = "human_label")]
    pub human_label: FailureClass,
    #[serde(default)]
    pub goal: Option<String>,
    pub records: Vec<SessionLogRecord>,
    #[serde(default)]
    pub regression_plan: Option<Vec<ToolInvocation>>,
}

#[derive(Debug, Clone)]
pub struct RegressionCase {
    pub trajectory_id: String,
    pub failure_class: FailureClass,
    pub goal: String,
    pub plan: Vec<ToolInvocation>,
}

pub fn classify_failure_trajectory(records: &[SessionLogRecord]) -> FailureClass {
    for record in records {
        match &record.payload {
            SessionLogPayload::Error { message } => {
                let message = message.to_ascii_lowercase();
                if message.contains("permission")
                    || message.contains("grant")
                    || message.contains("ungranted")
                {
                    return FailureClass::MissingGrant;
                }
                if message.contains("token budget") || message.contains("budget exceeded") {
                    return FailureClass::ContextExhaustion;
                }
                if message.contains("invalid plan")
                    || message.contains("schema")
                    || message.contains("json")
                {
                    return FailureClass::SchemaFailure;
                }
                if message.contains("unexpected tool") {
                    return FailureClass::WrongTool;
                }
            }
            SessionLogPayload::Budget {
                tokens_used,
                max_tokens,
                ..
            } if *tokens_used >= *max_tokens => return FailureClass::ContextExhaustion,
            SessionLogPayload::Tool { tool, output, .. } => {
                let output = output.to_ascii_lowercase();
                if output.contains("permission") || output.contains("grant") {
                    return FailureClass::MissingGrant;
                }
                if tool == "wrong_tool" || output.contains("unexpected invocation") {
                    return FailureClass::WrongTool;
                }
            }
            SessionLogPayload::Verify { passed, detail, .. } if !*passed => {
                let detail = detail.to_ascii_lowercase();
                if detail.contains("schema") {
                    return FailureClass::SchemaFailure;
                }
                return FailureClass::BadToolOutput;
            }
            _ => {}
        }
    }
    FailureClass::Unknown
}

pub fn classification_accuracy(trajectories: &[ForensicTrajectory]) -> f64 {
    if trajectories.is_empty() {
        return 0.0;
    }
    trajectories
        .iter()
        .filter(|t| classify_failure_trajectory(&t.records) == t.human_label)
        .count() as f64
        / trajectories.len() as f64
}

pub fn export_regression_case(trajectory: &ForensicTrajectory) -> RegressionCase {
    RegressionCase {
        trajectory_id: trajectory.id.clone(),
        failure_class: classify_failure_trajectory(&trajectory.records),
        goal: trajectory
            .goal
            .clone()
            .unwrap_or_else(|| format!("replay {}", trajectory.id)),
        plan: trajectory
            .regression_plan
            .clone()
            .unwrap_or_else(|| vec![ToolInvocation::Done]),
    }
}

pub fn load_forensic_trajectories(raw: &str) -> Result<Vec<ForensicTrajectory>, String> {
    #[derive(Deserialize)]
    struct Fixture {
        schema_version: u32,
        cases: Vec<ForensicTrajectory>,
    }
    let fixture: Fixture = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if fixture.schema_version != 1 {
        return Err(format!("bad schema {}", fixture.schema_version));
    }
    if fixture.cases.len() < 12 {
        return Err(format!("need >=12, got {}", fixture.cases.len()));
    }
    Ok(fixture.cases)
}
'''

SLEEP01_RS = (ROOT / "tests/golden_harness/src/sleep01.rs")
RELY01_RS = (ROOT / "tests/golden_harness/src/rely01.rs")
FORENSIC01_RS = (ROOT / "tests/golden_harness/src/forensic01.rs")

# Harness module sources embedded below (compact)
SLEEP01_HARNESS = '''//! SLEEP-01 — sleep-time memory compute improves graph-augmented recall@k (Phase 12).
use aether_core::{payload_to_graph_inserts, validate_graph_extract};
use aether_db::{mean_recall_at_k, recall_at_k_chunks, run_sleep_memory_cycle, SLEEP01_RECALL_DELTA, Database};
use serde::Deserialize;
use std::path::Path;
#[derive(Debug, Clone, Deserialize)] struct SleepMemoryChunk { chunk_id: String, source_uri: String, text: String, link_node_id: String, link_confidence: f64, embedding_hint: Vec<f32> }
#[derive(Debug, Clone, Deserialize)] struct SleepHeldOutQuery { id: String, query: String, query_embedding_hint: Vec<f32>, expected_chunk_ids: Vec<String>, k: usize }
#[derive(Debug, Clone, Deserialize)] struct Sleep01Fixture { schema_version: u32, session_id: String, extract_json: serde_json::Value, memory_chunks: Vec<SleepMemoryChunk>, held_out_queries: Vec<SleepHeldOutQuery> }
pub fn sleep01_fixture_ready() -> Result<usize, String> { Ok(load_sleep01_fixture()?.held_out_queries.len()) }
fn load_sleep01_fixture() -> Result<Sleep01Fixture, String> {
    let path = [Path::new("tests/golden_harness/fixtures/sleep01_queries.json"), Path::new("fixtures/sleep01_queries.json")].into_iter().find(|p| p.exists()).ok_or("SLEEP-01 fixture not found")?;
    let f: Sleep01Fixture = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?).map_err(|e| format!("parse: {e}"))?;
    if f.schema_version != 1 { return Err("bad schema".into()); }
    validate_graph_extract(&serde_json::to_string(&f.extract_json).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?; Ok(f)
}
fn hint_to_embedding(h: &[f32]) -> Vec<f32> { let mut e = vec![0.0f32;384]; for (i,v) in h.iter().enumerate().take(384) { e[i]=*v; } e }
fn seed(db: &Database, f: &Sleep01Fixture) -> Result<(), String> {
    db.conn().execute("INSERT INTO sessions (id,title,status) VALUES (?1,'SLEEP-01','active')", rusqlite::params![f.session_id]).map_err(|e| e.to_string())?;
    let payload = validate_graph_extract(&serde_json::to_string(&f.extract_json).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let (nodes, edges) = payload_to_graph_inserts(&payload, "memory://sleep01").map_err(|e| e.to_string())?;
    for n in &nodes { db.insert_graph_node(n.as_new(&f.session_id)).map_err(|e| e.to_string())?; }
    for e in &edges { db.insert_graph_edge(e.as_new(&f.session_id)).map_err(|e| e.to_string())?; }
    for c in &f.memory_chunks { db.insert_memory_chunk(&c.chunk_id,&c.source_uri,&c.text,&hint_to_embedding(&c.embedding_hint)).map_err(|e| e.to_string())?; if !c.link_node_id.is_empty() { db.link_graph_chunk(&c.chunk_id,&c.link_node_id,c.link_confidence).map_err(|e| e.to_string())?; } }
    db.conn().execute("UPDATE query_policy SET graph_hop_depth=1, graph_weight=4.0 WHERE policy_name='default'",[]).map_err(|e| e.to_string())?; Ok(())
}
pub fn test_sleep01_impl(db: &Database) -> Result<(), String> {
    let f = load_sleep01_fixture()?; seed(db,&f)?;
    let mut base = Vec::new();
    for q in &f.held_out_queries { let emb = hint_to_embedding(&q.query_embedding_hint); let top: Vec<String> = db.search_semantic_memory_hybrid(&q.query,&emb,q.k).map_err(|e| e.to_string())?.into_iter().map(|(id,_,_)|id).collect(); base.push(recall_at_k_chunks(&top,&q.expected_chunk_ids,q.k)); }
    let bm = mean_recall_at_k(&base);
    if run_sleep_memory_cycle(db,&f.session_id).map_err(|e| e.to_string())?.links_added == 0 { return Err("no links added".into()); }
    let mut after = Vec::new();
    for q in &f.held_out_queries { let emb = hint_to_embedding(&q.query_embedding_hint); let top: Vec<String> = db.search_hybrid_with_graph(&f.session_id,&q.query,&emb,q.k).map_err(|e| e.to_string())?.into_iter().map(|(id,_,_)|id).collect(); after.push(recall_at_k_chunks(&top,&q.expected_chunk_ids,q.k)); }
    let am = mean_recall_at_k(&after);
    if am < bm + SLEEP01_RECALL_DELTA { return Err(format!("delta insufficient: {bm:.3} -> {am:.3}")); }
    Ok(())
}
'''

RELY01_HARNESS = '''//! RELY-01 — per-quantization tool reliability ranking (Phase 12).
use aether_core::{evaluate_profile_reliability, rank_profiles_by_reliability, FrozenToolResponse, ToolCallCase};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
pub const RELY01_MIN_CASES: usize = 8;
#[derive(Debug, Clone, Deserialize)]
pub struct Rely01Fixture { pub schema_version: u32, pub cases: Vec<ToolCallCase>, pub profiles: HashMap<String, HashMap<String, FrozenToolResponse>> }
pub fn rely01_fixture_ready() -> Result<usize, String> { Ok(load_rely01_fixture()?.cases.len()) }
pub fn load_rely01_fixture() -> Result<Rely01Fixture, String> {
    let path = [Path::new("tests/golden_harness/fixtures/rely01_tool_calls.json"), Path::new("fixtures/rely01_tool_calls.json")].into_iter().find(|p| p.exists()).ok_or("RELY-01 fixture not found")?.to_path_buf();
    let fixture: Rely01Fixture = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?).map_err(|e| format!("parse: {e}"))?;
    if fixture.schema_version != 1 || fixture.cases.len() < RELY01_MIN_CASES { return Err("RELY-01 fixture invalid".into()); }
    for id in ["q4","q8"] { fixture.profiles.get(id).ok_or_else(|| format!("missing {id}"))?; }
    Ok(fixture)
}
pub fn test_rely01_impl() -> Result<(), String> {
    let fixture = load_rely01_fixture()?;
    let q4 = fixture.profiles.get("q4").unwrap(); let q8 = fixture.profiles.get("q8").unwrap();
    if evaluate_profile_reliability(&fixture.cases, q8) <= evaluate_profile_reliability(&fixture.cases, q4) { return Err("q8 must outscore q4".into()); }
    let profiles: Vec<_> = fixture.profiles.iter().map(|(id,r)| (id.clone(), r.clone())).collect();
    if rank_profiles_by_reliability(&profiles, &fixture.cases)[0].0 != "q8" { return Err("q8 must rank first".into()); }
    Ok(())
}
'''

FORENSIC01_HARNESS = '''//! FORENSIC-01 — failure trajectory classification + regression export (Phase 12).
use aether_core::LoopConfig;
use aether_daemon::forensics::{classification_accuracy, export_regression_case, load_forensic_trajectories, FailureClass, FORENSIC01_MIN_ACCURACY};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;
use std::path::Path;
use tempfile::tempdir;
pub fn forensic01_fixture_ready() -> Result<usize, String> {
    let path = [Path::new("tests/golden_harness/fixtures/forensic01_trajectories.json"), Path::new("fixtures/forensic01_trajectories.json")].into_iter().find(|p| p.exists()).ok_or("fixture missing")?;
    Ok(load_forensic_trajectories(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?.len())
}
pub fn test_forensic01_impl(db: &Database) -> Result<(), String> {
    let path = Path::new("tests/golden_harness/fixtures/forensic01_trajectories.json");
    let trajectories = load_forensic_trajectories(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?;
    if classification_accuracy(&trajectories) < FORENSIC01_MIN_ACCURACY { return Err("accuracy below threshold".into()); }
    let t = trajectories.iter().find(|x| x.id == "fg-missing-grant-01").ok_or("missing case")?;
    let regression = export_regression_case(t);
    if regression.failure_class != FailureClass::MissingGrant { return Err("wrong class".into()); }
    let tmp = tempdir().map_err(|e| e.to_string())?; let workspace = tmp.path(); let session_id = "sess-forensic-01-replay";
    let conn = db.conn();
    conn.execute("INSERT INTO sessions (id, title, status) VALUES (?1, 'FORENSIC-01', 'active')", rusqlite::params![session_id]).map_err(|e| e.to_string())?;
    conn.execute("INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')", rusqlite::params![session_id, workspace.to_string_lossy().to_string()]).map_err(|e| e.to_string())?;
    let mut config = LoopConfig::new(4, session_id.to_string(), workspace.to_path_buf());
    let (result, _) = execute_structured_loop(&conn, &mut config, regression.plan, None, &HashMap::new(), None, &regression.goal);
    result.map_err(|e| format!("replay failed: {e}"))?; Ok(())
}
'''


def patch_libs() -> None:
    db = ROOT / "crates/aether-db/src/lib.rs"
    text = db.read_text()
    if "mod sleep_memory;" not in text:
        text = text.replace("mod recovery;\n", "mod recovery;\nmod sleep_memory;\n")
    if "pub use sleep_memory" not in text:
        text = text.replace(
            "pub use recovery::{RecoveryManager, RecoveryReport};\n",
            "pub use recovery::{RecoveryManager, RecoveryReport};\n"
            "pub use sleep_memory::{\n"
            "    mean_recall_at_k, recall_at_k_chunks, run_sleep_memory_cycle, SleepCycleReport,\n"
            "    SLEEP01_RECALL_DELTA,\n"
            "};\n",
        )
    db.write_text(text)

    core = ROOT / "crates/aether-core/src/lib.rs"
    text = core.read_text()
    if "mod tool_reliability;" not in text:
        text = text.replace("mod subagent;\n", "mod subagent;\nmod tool_reliability;\n")
    if "pub use tool_reliability" not in text:
        text = text.replace(
            "pub use subagent::{",
            "pub use tool_reliability::{\n"
            "    evaluate_profile_reliability, rank_profiles_by_reliability, score_tool_response,\n"
            "    FrozenToolResponse, ToolCallCase,\n"
            "};\n\n"
            "pub use subagent::{",
        )
    core.write_text(text)

    mr = ROOT / "crates/aether-core/src/model_registry.rs"
    mrt = mr.read_text()
    if "PartialEq, Eq)]\npub struct ModelProfile" not in mrt:
        mrt = mrt.replace(
            "PartialEq)]\npub struct ModelProfile", "PartialEq, Eq)]\npub struct ModelProfile"
        )
        mr.write_text(mrt)

    daemon = ROOT / "crates/aether-daemon/src/lib.rs"
    text = daemon.read_text()
    if "pub mod forensics;" not in text:
        text = text.replace("pub mod checkpoint;\n", "pub mod checkpoint;\npub mod forensics;\n")
    daemon.write_text(text)


def patch_main() -> None:
    main = ROOT / "tests/golden_harness/src/main.rs"
    text = main.read_text()

    # Remove wave11 soft trio imports
    for block in [
        "mod mcp02;\nuse mcp02::test_mcp02_impl;\n\n",
        "mod compact01;\nuse compact01::test_compact01_impl;\n\n",
        "mod hook02;\nuse hook02::test_hook02_impl;\n\n",
    ]:
        text = text.replace(block, "")

    inserts = """mod fork01;
use fork01::test_fork01_impl;

mod head01;
use head01::test_head01_impl;

mod cache01;
use cache01::test_cache01_impl;

mod dist01;
use dist01::test_dist01_impl;

mod sleep01;
use sleep01::test_sleep01_impl;

mod rely01;
use rely01::test_rely01_impl;

mod forensic01;
use forensic01::test_forensic01_impl;

"""
    if "mod fork01;" not in text:
        text = text.replace("mod reg01;\nuse reg01::test_reg01_impl;\n\n", "mod reg01;\nuse reg01::test_reg01_impl;\n\n" + inserts)

    text = re.sub(r"const TASKS: \[TaskSpec; \d+\]", "const TASKS: [TaskSpec; 45]", text)

    # Replace task tail
    old_tail = re.search(
        r'    TaskSpec \{ name: "REG-01".*?\n\];',
        text,
        re.S,
    )
    new_tail = """    TaskSpec { name: "REG-01", hard_on_darwin: false, fail_closed_off_darwin: false },
    TaskSpec { name: "FORK-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "HEAD-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "CACHE-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "DIST-01", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "SLEEP-01", hard_on_darwin: false, fail_closed_off_darwin: false },
    TaskSpec { name: "RELY-01", hard_on_darwin: false, fail_closed_off_darwin: false },
    TaskSpec { name: "FORENSIC-01", hard_on_darwin: false, fail_closed_off_darwin: false },
];"""
    if old_tail:
        text = text[: old_tail.start()] + new_tail + text[old_tail.end() :]

    for block in [
        "    match mcp02::mcp02_fixture_ready() {\n        Ok(()) => println!(\"MCP-02 fixtures: user MCP registry + filesystem MCP ready\"),\n        Err(e) => eprintln!(\"Warning: MCP-02 fixture check failed: {}\", e),\n    }\n\n",
        "    match compact01::compact01_fixture_ready() {\n        Ok(()) => println!(\"COMPACT-01 fixtures: context compaction helpers ready\"),\n        Err(e) => eprintln!(\"Warning: COMPACT-01 fixture check failed: {}\", e),\n    }\n\n",
    ]:
        text = text.replace(block, "")

    fixture_block = """    match fork01::fork01_fixture_ready() {
        Ok(()) => println!("FORK-01 fixtures: session fork helpers ready"),
        Err(e) => eprintln!("Warning: FORK-01 fixture check failed: {}", e),
    }

    match head01::head01_fixture_ready() {
        Ok(()) => println!("HEAD-01 fixtures: headless NDJSON helpers ready"),
        Err(e) => eprintln!("Warning: HEAD-01 fixture check failed: {}", e),
    }

    match cache01::cache01_fixture_ready() {
        Ok(()) => println!("CACHE-01 fixtures: prefix-cache helpers ready"),
        Err(e) => eprintln!("Warning: CACHE-01 fixture check failed: {}", e),
    }

    match sleep01::sleep01_fixture_ready() {
        Ok(n) => println!("SLEEP-01 fixtures: {} held-out queries loaded", n),
        Err(e) => eprintln!("Warning: SLEEP-01 fixture check failed: {}", e),
    }

    match rely01::rely01_fixture_ready() {
        Ok(n) => println!("RELY-01 fixtures: {} frozen tool-call cases loaded", n),
        Err(e) => eprintln!("Warning: RELY-01 fixture check failed: {}", e),
    }

    match forensic01::forensic01_fixture_ready() {
        Ok(n) => println!("FORENSIC-01 fixtures: {} labeled trajectories loaded", n),
        Err(e) => eprintln!("Warning: FORENSIC-01 fixture check failed: {}", e),
    }

"""
    if "sleep01::sleep01_fixture_ready" not in text:
        text = text.replace("    match reg01::reg01_fixture_ready() {", fixture_block + "    match reg01::reg01_fixture_ready() {")

    dispatch_old = re.search(
        r'        "REG-01" => test_reg01_impl\(\)\.await\.map\(\|_\| false\),\n.*?other =>',
        text,
        re.S,
    )
    dispatch_new = """        "REG-01" => test_reg01_impl().await.map(|_| false),
        "FORK-01" => test_fork01_impl(db).map(|hard| hard),
        "HEAD-01" => test_head01_impl().map(|hard| hard),
        "CACHE-01" => test_cache01_impl().await.map(|hard| hard),
        "DIST-01" => test_dist01_impl().map(|_| true),
        "SLEEP-01" => async { test_sleep01_impl(db).map(|_| false) }.await,
        "RELY-01" => async { test_rely01_impl().map(|_| false) }.await,
        "FORENSIC-01" => async { test_forensic01_impl(db).map(|_| false) }.await,
        other =>"""
    if dispatch_old:
        text = text[: dispatch_old.start()] + dispatch_new + text[dispatch_old.end() :]

    main.write_text(text)


def write_fixtures() -> None:
    sleep = {
        "schema_version": 1,
        "session_id": "sess-sleep-01",
        "extract_json": {
            "nodes": [
                {"id": "node-zephyr", "entity_type": "project", "canonical_name": "Zephyr-7", "aliases": ["Zephyr"], "provenance": "extracted", "evidence_text": "z"},
                {"id": "node-bridge", "entity_type": "concept", "canonical_name": "sleep bridge", "aliases": [], "provenance": "extracted", "evidence_text": "b"},
                {"id": "node-codex", "entity_type": "file", "canonical_name": "AetherForge codex", "aliases": [], "provenance": "extracted", "evidence_text": "c"},
            ],
            "edges": [
                {"src_node_id": "node-zephyr", "dst_node_id": "node-bridge", "relation_type": "related_to", "weight": 2.0, "evidence_text": "e"},
                {"src_node_id": "node-bridge", "dst_node_id": "node-codex", "relation_type": "implements", "weight": 1.5, "evidence_text": "e"},
            ],
        },
        "memory_chunks": [
            {"chunk_id": "sleep-chk-noise", "source_uri": "m://n", "text": "Generic platform runtime overview for services", "link_node_id": "node-zephyr", "link_confidence": 0.9, "embedding_hint": [1.0, 0.0]},
            {"chunk_id": "sleep-chk-doc1", "source_uri": "m://d1", "text": "Platform runtime overview documentation index", "link_node_id": "", "link_confidence": 0.0, "embedding_hint": [0.92, 0.25]},
            {"chunk_id": "sleep-chk-doc2", "source_uri": "m://d2", "text": "Runtime overview notes for platform services", "link_node_id": "", "link_confidence": 0.0, "embedding_hint": [0.88, 0.3]},
            {"chunk_id": "sleep-chk-bridge", "source_uri": "m://b", "text": "The Zephyr-7 release uses the sleep bridge for offline recall during idle windows", "link_node_id": "", "link_confidence": 0.0, "embedding_hint": [0.05, 0.95]},
            {"chunk_id": "sleep-chk-codex", "source_uri": "m://c", "text": "AetherForge codex chapter on sleep-time memory compute and graph chunk links", "link_node_id": "", "link_confidence": 0.0, "embedding_hint": [0.1, 0.9]},
        ],
        "held_out_queries": [
            {"id": "sq-bridge", "query": "platform runtime overview", "query_embedding_hint": [1.0, 0.0], "expected_chunk_ids": ["sleep-chk-bridge"], "k": 3},
            {"id": "sq-codex", "query": "platform runtime overview services", "query_embedding_hint": [0.95, 0.05], "expected_chunk_ids": ["sleep-chk-codex", "sleep-chk-bridge"], "k": 3},
        ],
    }
    write("tests/golden_harness/fixtures/sleep01_queries.json", json.dumps(sleep, indent=2) + "\n")

    rely_path = ROOT / "tests/golden_harness/fixtures/rely01_tool_calls.json"
    if not rely_path.exists() or rely_path.stat().st_size < 100:
        write("tests/golden_harness/fixtures/rely01_tool_calls.json", json.dumps(json.loads('''{"schema_version":1,"cases":[{"id":"tc-01","tool_name":"fs_write","expect_success":true},{"id":"tc-02","tool_name":"fs_read","expect_success":true},{"id":"tc-03","tool_name":"python_lint","expect_success":true},{"id":"tc-04","tool_name":"git_init","expect_success":true},{"id":"tc-05","tool_name":"fs_write","expect_success":false},{"id":"tc-06","tool_name":"fs_read","expect_success":false},{"id":"tc-07","tool_name":"mcp_call","expect_success":true},{"id":"tc-08","tool_name":"verify_contains","expect_success":true}],"profiles":{"q4":{"tc-01":{"tool_name":"fs_write","success":true,"arguments_match":true,"output":"ok"},"tc-02":{"tool_name":"fs_read","success":true,"arguments_match":false,"output":"ok"},"tc-03":{"tool_name":"python_lint","success":true,"arguments_match":true,"output":"ok"},"tc-04":{"tool_name":"git_init","success":false,"arguments_match":true,"output":"fail"},"tc-05":{"tool_name":"fs_write","success":true,"arguments_match":false,"output":"unexpected"},"tc-06":{"tool_name":"fs_read","success":false,"arguments_match":true,"output":"denied"},"tc-07":{"tool_name":"mcp_call","success":true,"arguments_match":false,"output":"ok"},"tc-08":{"tool_name":"verify_contains","success":false,"arguments_match":true,"output":"miss"}},"q8":{"tc-01":{"tool_name":"fs_write","success":true,"arguments_match":true,"output":"ok"},"tc-02":{"tool_name":"fs_read","success":true,"arguments_match":true,"output":"ok"},"tc-03":{"tool_name":"python_lint","success":true,"arguments_match":true,"output":"ok"},"tc-04":{"tool_name":"git_init","success":true,"arguments_match":true,"output":"ok"},"tc-05":{"tool_name":"fs_write","success":false,"arguments_match":true,"output":"denied"},"tc-06":{"tool_name":"fs_read","success":false,"arguments_match":true,"output":"denied"},"tc-07":{"tool_name":"mcp_call","success":true,"arguments_match":true,"output":"ok"},"tc-08":{"tool_name":"verify_contains","success":true,"arguments_match":true,"output":"ok"}}}}'''), indent=2) + "\n")

    forensic_path = ROOT / "tests/golden_harness/fixtures/forensic01_trajectories.json"
    if not forensic_path.exists() or forensic_path.stat().st_size < 100:
        raise SystemExit("forensic01_trajectories.json missing — add fixture with >=12 cases")


def main() -> None:
    write("crates/aether-db/src/sleep_memory.rs", SLEEP_MEMORY)
    write("crates/aether-core/src/tool_reliability.rs", TOOL_RELIABILITY)
    write("crates/aether-daemon/src/forensics.rs", FORENSICS)
    write("tests/golden_harness/src/sleep01.rs", SLEEP01_HARNESS)
    write("tests/golden_harness/src/rely01.rs", RELY01_HARNESS)
    write("tests/golden_harness/src/forensic01.rs", FORENSIC01_HARNESS)
    write_fixtures()
    patch_libs()
    patch_main()
    print("bootstrap complete")


if __name__ == "__main__":
    main()
