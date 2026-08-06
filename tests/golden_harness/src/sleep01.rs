//! SLEEP-01 — sleep-time memory compute improves graph-augmented recall@k (Phase 12).
use aether_core::{payload_to_graph_inserts, validate_graph_extract};
use aether_db::{mean_recall_at_k, recall_at_k_chunks, run_sleep_memory_cycle, SLEEP01_RECALL_DELTA, Database};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
struct SleepMemoryChunk {
    chunk_id: String,
    source_uri: String,
    text: String,
    link_node_id: String,
    link_confidence: f64,
    embedding_hint: Vec<f32>,
}
#[derive(Debug, Clone, Deserialize)]
struct SleepHeldOutQuery {
    id: String,
    query: String,
    query_embedding_hint: Vec<f32>,
    expected_chunk_ids: Vec<String>,
    k: usize,
}
#[derive(Debug, Clone, Deserialize)]
struct Sleep01Fixture {
    schema_version: u32,
    session_id: String,
    extract_json: serde_json::Value,
    memory_chunks: Vec<SleepMemoryChunk>,
    held_out_queries: Vec<SleepHeldOutQuery>,
}

pub fn sleep01_fixture_ready() -> Result<usize, String> {
    Ok(load_sleep01_fixture()?.held_out_queries.len())
}

fn load_sleep01_fixture() -> Result<Sleep01Fixture, String> {
    let path = [Path::new("tests/golden_harness/fixtures/sleep01_queries.json"), Path::new("fixtures/sleep01_queries.json")]
        .into_iter()
        .find(|p| p.exists())
        .ok_or("SLEEP-01 fixture not found")?;
    let f: Sleep01Fixture = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("parse: {e}"))?;
    if f.schema_version != 1 {
        return Err("bad schema".into());
    }
    validate_graph_extract(&serde_json::to_string(&f.extract_json).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(f)
}

fn hint_to_embedding(h: &[f32]) -> Vec<f32> {
    let mut e = vec![0.0f32; 384];
    for (i, v) in h.iter().enumerate().take(384) {
        e[i] = *v;
    }
    e
}

fn seed(db: &Database, f: &Sleep01Fixture) -> Result<(), String> {
    db.conn()
        .execute(
            "INSERT INTO sessions (id,title,status) VALUES (?1,'SLEEP-01','active')",
            rusqlite::params![f.session_id],
        )
        .map_err(|e| e.to_string())?;
    let payload = validate_graph_extract(&serde_json::to_string(&f.extract_json).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let (nodes, edges) = payload_to_graph_inserts(&payload, "memory://sleep01").map_err(|e| e.to_string())?;
    for n in &nodes {
        db.insert_graph_node(n.as_new(&f.session_id)).map_err(|e| e.to_string())?;
    }
    for edge in &edges {
        db.insert_graph_edge(edge.as_new(&f.session_id)).map_err(|e| e.to_string())?;
    }
    for c in &f.memory_chunks {
        db.insert_memory_chunk(&c.chunk_id, &c.source_uri, &c.text, &hint_to_embedding(&c.embedding_hint))
            .map_err(|e| e.to_string())?;
        if !c.link_node_id.is_empty() {
            db.link_graph_chunk(&c.chunk_id, &c.link_node_id, c.link_confidence).map_err(|e| e.to_string())?;
        }
    }
    db.conn()
        .execute(
            "UPDATE query_policy SET graph_hop_depth=1, graph_weight=4.0 WHERE policy_name='default'",
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn test_sleep01_impl(db: &Database) -> Result<(), String> {
    let f = load_sleep01_fixture()?;
    seed(db, &f)?;
    let mut base = Vec::new();
    for q in &f.held_out_queries {
        let emb = hint_to_embedding(&q.query_embedding_hint);
        let top: Vec<String> = db
            .search_semantic_memory_hybrid(&q.query, &emb, q.k)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        base.push(recall_at_k_chunks(&top, &q.expected_chunk_ids, q.k));
    }
    let bm = mean_recall_at_k(&base);
    if run_sleep_memory_cycle(db, &f.session_id).map_err(|e| e.to_string())?.links_added == 0 {
        return Err("no links added".into());
    }
    let mut after = Vec::new();
    for q in &f.held_out_queries {
        let emb = hint_to_embedding(&q.query_embedding_hint);
        let top: Vec<String> = db
            .search_hybrid_with_graph(&f.session_id, &q.query, &emb, q.k)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        after.push(recall_at_k_chunks(&top, &q.expected_chunk_ids, q.k));
    }
    let am = mean_recall_at_k(&after);
    if am < bm + SLEEP01_RECALL_DELTA {
        return Err(format!("delta insufficient: {bm:.3} -> {am:.3}"));
    }
    Ok(())
}
