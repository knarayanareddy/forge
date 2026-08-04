//! GRAPH-01 — seeded graph recall@k via production graph_extract + hybrid query (Slice 6.5).

use aether_core::{payload_to_graph_inserts, validate_graph_extract};
use aether_db::Database;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const GRAPH_SEED_FIXTURE_PATH: &str = "tests/golden_harness/fixtures/graph_seed.json";
pub const GRAPH01_MIN_QUERIES: usize = 5;
pub const GRAPH01_RECALL_K: usize = 3;
pub const GRAPH01_MIN_GRAPH_BOOSTS: usize = 2;

#[derive(Debug, Clone, Deserialize)]
pub struct GraphSeedChunk {
    pub chunk_id: String,
    pub source_uri: String,
    pub text: String,
    pub link_node_id: String,
    pub link_confidence: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphGoldQuery {
    pub id: String,
    pub query: String,
    pub expected_node_ids: Vec<String>,
    #[serde(default)]
    pub expect_graph_rank_change: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphSeedFixture {
    pub schema_version: u32,
    pub description: String,
    pub session_id: String,
    pub embed_model: String,
    pub embed_endpoint: String,
    pub extract_json: serde_json::Value,
    pub memory_chunks: Vec<GraphSeedChunk>,
    pub gold_queries: Vec<GraphGoldQuery>,
}

pub fn graph01_fixture_ready() -> Result<usize, String> {
    let fixture = load_graph_seed_fixture()?;
    Ok(fixture.gold_queries.len())
}

pub fn load_graph_seed_fixture() -> Result<GraphSeedFixture, String> {
    let path = resolve_fixture_path()?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let fixture: GraphSeedFixture =
        serde_json::from_str(&content).map_err(|e| format!("parse GRAPH-01 fixture: {}", e))?;
    validate_fixture(&fixture)?;
    Ok(fixture)
}

fn resolve_fixture_path() -> Result<PathBuf, String> {
    let candidates = [
        Path::new(GRAPH_SEED_FIXTURE_PATH),
        Path::new("fixtures/graph_seed.json"),
        Path::new("tests/golden_harness/fixtures/graph_seed.json"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(format!(
        "GRAPH-01 fixture not found (tried {:?})",
        candidates
    ))
}

fn validate_fixture(fixture: &GraphSeedFixture) -> Result<(), String> {
    if fixture.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {} (expected 1)",
            fixture.schema_version
        ));
    }

    if fixture.gold_queries.len() < GRAPH01_MIN_QUERIES {
        return Err(format!(
            "GRAPH-01 requires >= {} gold queries, found {}",
            GRAPH01_MIN_QUERIES,
            fixture.gold_queries.len()
        ));
    }

    if fixture.memory_chunks.is_empty() {
        return Err("GRAPH-01 fixture must include memory_chunks".into());
    }

    let extract_str = serde_json::to_string(&fixture.extract_json)
        .map_err(|e| format!("serialize extract_json: {}", e))?;
    validate_graph_extract(&extract_str)
        .map_err(|e| format!("extract_json invalid for graph_extract: {}", e))?;

    let node_ids: HashSet<&str> = fixture
        .extract_json
        .get("nodes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "extract_json missing nodes array".to_string())?
        .iter()
        .filter_map(|n| n.get("id").and_then(|v| v.as_str()))
        .collect();

    for chunk in &fixture.memory_chunks {
        if !node_ids.contains(chunk.link_node_id.as_str()) {
            return Err(format!(
                "chunk {} links unknown node {}",
                chunk.chunk_id, chunk.link_node_id
            ));
        }
    }

    for query in &fixture.gold_queries {
        if query.expected_node_ids.is_empty() {
            return Err(format!("gold query {} missing expected_node_ids", query.id));
        }
        for node_id in &query.expected_node_ids {
            if !node_ids.contains(node_id.as_str()) {
                return Err(format!(
                    "gold query {} expects unknown node {}",
                    query.id, node_id
                ));
            }
        }
    }

    let boost_expected = fixture
        .gold_queries
        .iter()
        .filter(|q| q.expect_graph_rank_change)
        .count();
    if boost_expected < GRAPH01_MIN_GRAPH_BOOSTS {
        return Err(format!(
            "GRAPH-01 requires >= {} queries marked expect_graph_rank_change, found {}",
            GRAPH01_MIN_GRAPH_BOOSTS, boost_expected
        ));
    }

    Ok(())
}

fn seed_session(db: &Database, session_id: &str) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'GRAPH-01 Session', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn seed_graph_from_fixture(db: &Database, fixture: &GraphSeedFixture) -> Result<(), String> {
    seed_session(db, &fixture.session_id)?;

    let extract_str = serde_json::to_string(&fixture.extract_json)
        .map_err(|e| format!("serialize extract_json: {}", e))?;
    let payload = validate_graph_extract(&extract_str)
        .map_err(|e| format!("validate_graph_extract: {}", e))?;
    let source_uri = "memory://graph-seed/fixture";
    let (nodes, edges) = payload_to_graph_inserts(&payload, source_uri)
        .map_err(|e| format!("payload_to_graph_inserts: {}", e))?;

    for node in &nodes {
        db.insert_graph_node(node.as_new(&fixture.session_id))
            .map_err(|e| format!("insert_graph_node {}: {}", node.id, e))?;
    }
    for edge in &edges {
        db.insert_graph_edge(edge.as_new(&fixture.session_id))
            .map_err(|e| format!("insert_graph_edge: {}", e))?;
    }

    Ok(())
}

pub async fn seed_memory_chunks(
    db: &Database,
    fixture: &GraphSeedFixture,
) -> Result<HashMap<String, String>, String> {
    aether_core::OllamaProvider::health_check(&fixture.embed_endpoint)
        .await
        .map_err(|e| {
            format!(
                "Ollama offline or unreachable: {}. (Rule: GRAPH-01 must fail if Ollama is down)",
                e
            )
        })?;

    let mut chunk_to_node = HashMap::new();

    for chunk in &fixture.memory_chunks {
        let embedding = aether_core::fetch_ollama_embedding(
            &fixture.embed_endpoint,
            &fixture.embed_model,
            &chunk.text,
        )
        .await
        .map_err(|e| format!("Ollama embed failed for {}: {}", chunk.chunk_id, e))?;

        db.insert_memory_chunk(
            &chunk.chunk_id,
            &chunk.source_uri,
            &chunk.text,
            &embedding,
        )
        .map_err(|e| format!("insert_memory_chunk {}: {}", chunk.chunk_id, e))?;

        db.link_graph_chunk(&chunk.chunk_id, &chunk.link_node_id, chunk.link_confidence)
            .map_err(|e| {
                format!(
                    "link_graph_chunk {} -> {}: {}",
                    chunk.chunk_id, chunk.link_node_id, e
                )
            })?;

        chunk_to_node.insert(chunk.chunk_id.clone(), chunk.link_node_id.clone());
    }

    Ok(chunk_to_node)
}

pub fn set_graph_weight(db: &Database, weight: f64) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "UPDATE query_policy SET graph_weight = ?1 WHERE policy_name = 'default'",
        rusqlite::params![weight],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_graph_hop_depth(db: &Database, depth: i32) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "UPDATE query_policy SET graph_hop_depth = ?1 WHERE policy_name = 'default'",
        rusqlite::params![depth],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn ranked_chunk_ids(results: &[(String, String, f32)]) -> Vec<String> {
    results.iter().map(|(id, _, _)| id.clone()).collect()
}

pub fn recall_at_k(
    top_chunk_ids: &[String],
    chunk_to_node: &HashMap<String, String>,
    expected_nodes: &[String],
    k: usize,
) -> f64 {
    if expected_nodes.is_empty() {
        return 0.0;
    }

    let mut hit_nodes = HashSet::new();
    for chunk_id in top_chunk_ids.iter().take(k) {
        if let Some(node_id) = chunk_to_node.get(chunk_id) {
            hit_nodes.insert(node_id.as_str());
        }
    }

    let hits = expected_nodes
        .iter()
        .filter(|node| hit_nodes.contains(node.as_str()))
        .count();

    hits as f64 / expected_nodes.len() as f64
}

pub fn rankings_differ(a: &[String], b: &[String]) -> bool {
    a != b
}

pub async fn test_graph_01_impl(db: &Database) -> Result<(), String> {
    let fixture = load_graph_seed_fixture()?;

    seed_graph_from_fixture(db, &fixture)?;
    let chunk_to_node = seed_memory_chunks(db, &fixture).await?;

    set_graph_hop_depth(db, 1)?;
    set_graph_weight(db, 3.0)?;

    let mut query_recalls = Vec::new();
    let mut graph_boost_hits = 0usize;

    for gold in &fixture.gold_queries {
        let query_emb = aether_core::fetch_ollama_embedding(
            &fixture.embed_endpoint,
            &fixture.embed_model,
            &gold.query,
        )
        .await
        .map_err(|e| format!("query embed failed for {}: {}", gold.id, e))?;

        let baseline = db
            .search_semantic_memory_hybrid(&gold.query, &query_emb, GRAPH01_RECALL_K)
            .map_err(|e| format!("baseline search failed for {}: {}", gold.id, e))?;
        let graph_results = db
            .search_hybrid_with_graph(
                &fixture.session_id,
                &gold.query,
                &query_emb,
                GRAPH01_RECALL_K,
            )
            .map_err(|e| format!("graph search failed for {}: {}", gold.id, e))?;

        let baseline_ids = ranked_chunk_ids(&baseline);
        let graph_ids = ranked_chunk_ids(&graph_results);

        let recall = recall_at_k(
            &graph_ids,
            &chunk_to_node,
            &gold.expected_node_ids,
            GRAPH01_RECALL_K,
        );
        query_recalls.push((gold.id.clone(), recall));

        if recall < 1.0 {
            return Err(format!(
                "GRAPH-01 recall@{} failed for {} (recall={:.2}, expected nodes {:?}, top chunks {:?})",
                GRAPH01_RECALL_K,
                gold.id,
                recall,
                gold.expected_node_ids,
                graph_ids,
            ));
        }

        if gold.expect_graph_rank_change && rankings_differ(&graph_ids, &baseline_ids) {
            graph_boost_hits += 1;
        }
    }

    let mean_recall: f64 =
        query_recalls.iter().map(|(_, r)| r).sum::<f64>() / query_recalls.len() as f64;

    if mean_recall < 1.0 {
        return Err(format!(
            "GRAPH-01 mean recall@{} {:.2} below required 1.0",
            GRAPH01_RECALL_K, mean_recall
        ));
    }

    if graph_boost_hits < GRAPH01_MIN_GRAPH_BOOSTS {
        return Err(format!(
            "GRAPH-01 graph hop changed ranking on {}/{} queries (need >= {})",
            graph_boost_hits,
            fixture.gold_queries.len(),
            GRAPH01_MIN_GRAPH_BOOSTS
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_seed_fixture_loads_and_validates() {
        let fixture = load_graph_seed_fixture().expect("fixture must load");
        assert!(fixture.gold_queries.len() >= GRAPH01_MIN_QUERIES);
        assert!(fixture
            .gold_queries
            .iter()
            .any(|q| q.query.contains("maintains the AetherForge daemon")));
        assert!(fixture
            .gold_queries
            .iter()
            .filter(|q| q.expect_graph_rank_change)
            .count() >= GRAPH01_MIN_GRAPH_BOOSTS);
    }

    #[test]
    fn recall_at_k_all_hit_is_one() {
        let mut map = HashMap::new();
        map.insert("c1".into(), "node-forge-maintainer".into());
        let recall = recall_at_k(
            &["c1".into(), "c2".into()],
            &map,
            &["node-forge-maintainer".into()],
            3,
        );
        assert!((recall - 1.0).abs() < f64::EPSILON);
    }
}
