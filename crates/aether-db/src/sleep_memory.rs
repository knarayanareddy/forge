//! Sleep-time memory compute (Phase 12 / SLEEP-01).
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
        return Ok(SleepCycleReport { links_added: 0, chunks_scanned: 0 });
    }
    let chunks: Vec<(String, String)> = {
        let conn = db.conn();
        let mut stmt = conn.prepare("SELECT chunk_id, chunk_text FROM semantic_memory ORDER BY id")?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<Vec<_>, _>>()?
    };
    let mut links_added = 0usize;
    for (chunk_id, chunk_text) in &chunks {
        let haystack = chunk_text.to_ascii_lowercase();
        for node in &nodes {
            let mut matched = mentions(&haystack, &node.canonical_name);
            if !matched {
                if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&node.aliases_json) {
                    matched = aliases.iter().any(|a| mentions(&haystack, a));
                }
            }
            if matched && db.link_graph_chunk(chunk_id, &node.id, 0.85).is_ok() {
                links_added += 1;
            }
        }
    }
    Ok(SleepCycleReport { links_added, chunks_scanned: chunks.len() })
}

fn mentions(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    needle.len() >= 3 && haystack.contains(&needle.to_ascii_lowercase())
}

pub fn recall_at_k_chunks(top: &[String], expected: &[String], k: usize) -> f64 {
    if expected.is_empty() { return 0.0; }
    let set: std::collections::HashSet<&str> = top.iter().take(k).map(String::as_str).collect();
    expected.iter().filter(|id| set.contains(id.as_str())).count() as f64 / expected.len() as f64
}

pub fn mean_recall_at_k(recalls: &[f64]) -> f64 {
    if recalls.is_empty() { 0.0 } else { recalls.iter().sum::<f64>() / recalls.len() as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityType, NewGraphEdge, NewGraphNode, RelationType};

    #[test]
    fn sleep_cycle_improves_recall() {
        let db = Database::open_in_memory().unwrap();
        let s = "sess-sleep-unit";
        db.conn().execute("INSERT INTO sessions (id,title,status) VALUES (?1,'s','active')", rusqlite::params![s]).unwrap();
        db.insert_graph_node(NewGraphNode { id: "node-z", session_id: s, entity_type: EntityType::Project, canonical_name: "Zephyr-7", aliases_json: "[]", properties_json: "{}", source_uri: "m", valid_from: None, valid_to: None }).unwrap();
        db.insert_graph_node(NewGraphNode { id: "node-b", session_id: s, entity_type: EntityType::Concept, canonical_name: "sleep bridge", aliases_json: "[]", properties_json: "{}", source_uri: "m", valid_from: None, valid_to: None }).unwrap();
        db.insert_graph_edge(NewGraphEdge { session_id: s, src_node_id: "node-z", dst_node_id: "node-b", relation_type: RelationType::RelatedTo, weight: 2.0, evidence_text: "e", source_uri: "m", valid_from: None, valid_to: None }).unwrap();
        let mut e0 = vec![0.0f32; 384]; e0[0] = 1.0;
        let mut e1 = vec![0.0f32; 384]; e1[0] = 0.92; e1[1] = 0.25;
        let mut e2 = vec![0.0f32; 384]; e2[0] = 0.88; e2[1] = 0.3;
        db.insert_memory_chunk("n", "m", "Generic platform runtime overview", &e0).unwrap();
        db.link_graph_chunk("n", "node-z", 0.9).unwrap();
        db.insert_memory_chunk("d1", "m", "Platform runtime overview documentation index", &e1).unwrap();
        db.insert_memory_chunk("d2", "m", "Runtime overview notes for platform services", &e2).unwrap();
        db.insert_memory_chunk("b", "m", "The Zephyr-7 release uses the sleep bridge for offline recall", &vec![0.0f32; 384]).unwrap();
        db.conn().execute("UPDATE query_policy SET graph_hop_depth=1, graph_weight=4.0 WHERE policy_name='default'", []).unwrap();
        let mut q = vec![0.0f32; 384]; q[0] = 1.0;
        let base = db.search_semantic_memory_hybrid("platform runtime overview", &q, 2).unwrap();
        let br = recall_at_k_chunks(&base.iter().map(|(id,_,_)| id.clone()).collect::<Vec<_>>(), &["b".into()], 2);
        assert!(run_sleep_memory_cycle(&db, s).unwrap().links_added >= 1);
        let after = db.search_hybrid_with_graph(s, "platform runtime overview", &q, 2).unwrap();
        let ar = recall_at_k_chunks(&after.iter().map(|(id,_,_)| id.clone()).collect::<Vec<_>>(), &["b".into()], 2);
        assert!(ar >= br + SLEEP01_RECALL_DELTA);
    }
}
