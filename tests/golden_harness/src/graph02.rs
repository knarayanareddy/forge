//! GRAPH-02 — graph v2 multi-hop recall delta over GRAPH-01 baseline (Phase 11.13).

use crate::graph01::{
    load_graph_seed_fixture, ranked_chunk_ids, recall_at_k, rankings_differ, seed_graph_from_fixture,
    seed_memory_chunks, set_graph_hop_depth, set_graph_weight, GraphGoldQuery, GRAPH01_RECALL_K,
};
use aether_db::Database;
use std::collections::HashMap;

pub const GRAPH02_MIN_QUERIES: usize = 8;
pub const GRAPH02_RECALL_DELTA_ABS: f64 = 0.1;
pub const GRAPH02_RECALL_DELTA_REL: f64 = 0.05;
pub const GRAPH02_MIN_RANK_CHANGES: usize = 2;

fn extra_gold_queries() -> Vec<GraphGoldQuery> {
    vec![
        GraphGoldQuery {
            id: "gq-ingest-multihop".into(),
            query: "post-turn graph ingest hook on the daemon".into(),
            expected_node_ids: vec!["node-ingest-pipeline".into()],
            expect_graph_rank_change: true,
        },
        GraphGoldQuery {
            id: "gq-repo-multihop".into(),
            query: "forge repository crates layout golden harness".into(),
            expected_node_ids: vec!["node-forge-repo".into()],
            expect_graph_rank_change: true,
        },
        GraphGoldQuery {
            id: "gq-maintainer-oncall".into(),
            query: "Alex Maintainer on-call daemon releases".into(),
            expected_node_ids: vec!["node-forge-maintainer".into()],
            expect_graph_rank_change: false,
        },
    ]
}

pub fn graph02_fixture_ready() -> Result<usize, String> {
    let fixture = load_graph_seed_fixture()?;
    let total = fixture.gold_queries.len() + extra_gold_queries().len();
    if total < GRAPH02_MIN_QUERIES {
        return Err(format!(
            "GRAPH-02 requires >= {} gold queries, found {}",
            GRAPH02_MIN_QUERIES, total
        ));
    }
    Ok(total)
}

async fn mean_recall_at_depth(
    db: &Database,
    fixture_session: &str,
    embed_endpoint: &str,
    embed_model: &str,
    queries: &[GraphGoldQuery],
    chunk_to_node: &HashMap<String, String>,
    hop_depth: i32,
) -> Result<(f64, usize), String> {
    set_graph_hop_depth(db, hop_depth)?;
    let mut recalls = Vec::new();
    let mut rank_changes = 0usize;

    for gold in queries {
        let query_emb = aether_core::fetch_ollama_embedding(
            embed_endpoint,
            embed_model,
            &gold.query,
        )
        .await
        .map_err(|e| format!("query embed failed for {}: {}", gold.id, e))?;

        let baseline = db
            .search_semantic_memory_hybrid(&gold.query, &query_emb, GRAPH01_RECALL_K)
            .map_err(|e| format!("baseline search failed for {}: {}", gold.id, e))?;
        let graph_results = db
            .search_hybrid_with_graph(
                fixture_session,
                &gold.query,
                &query_emb,
                GRAPH01_RECALL_K,
            )
            .map_err(|e| format!("graph search failed for {}: {}", gold.id, e))?;

        let baseline_ids = ranked_chunk_ids(&baseline);
        let graph_ids = ranked_chunk_ids(&graph_results);
        let recall = recall_at_k(
            &graph_ids,
            chunk_to_node,
            &gold.expected_node_ids,
            GRAPH01_RECALL_K,
        );
        recalls.push(recall);

        if gold.expect_graph_rank_change && rankings_differ(&graph_ids, &baseline_ids) {
            rank_changes += 1;
        }
    }

    let mean = recalls.iter().sum::<f64>() / recalls.len().max(1) as f64;
    Ok((mean, rank_changes))
}

pub async fn test_graph02_impl(db: &Database) -> Result<(), String> {
    let fixture = load_graph_seed_fixture()?;
    let mut all_queries = fixture.gold_queries.clone();
    all_queries.extend(extra_gold_queries());

    if all_queries.len() < GRAPH02_MIN_QUERIES {
        return Err(format!(
            "GRAPH-02 needs >= {} queries, got {}",
            GRAPH02_MIN_QUERIES,
            all_queries.len()
        ));
    }

    seed_graph_from_fixture(db, &fixture)?;
    let chunk_to_node = seed_memory_chunks(db, &fixture).await?;
    set_graph_weight(db, 3.0)?;

    let (v1_mean, _) = mean_recall_at_depth(
        db,
        &fixture.session_id,
        &fixture.embed_endpoint,
        &fixture.embed_model,
        &all_queries,
        &chunk_to_node,
        1,
    )
    .await?;

    let (v2_mean, v2_changes) = mean_recall_at_depth(
        db,
        &fixture.session_id,
        &fixture.embed_endpoint,
        &fixture.embed_model,
        &all_queries,
        &chunk_to_node,
        2,
    )
    .await?;

    let abs_delta = v2_mean - v1_mean;
    let rel_delta = if v1_mean > 0.0 {
        abs_delta / v1_mean
    } else {
        abs_delta
    };

    if v1_mean >= 1.0 - f64::EPSILON {
        if v2_mean + f64::EPSILON < v1_mean {
            return Err(format!(
                "GRAPH-02 must not regress recall: v1={:.3} v2={:.3}",
                v1_mean, v2_mean
            ));
        }
    } else if abs_delta < GRAPH02_RECALL_DELTA_ABS && rel_delta < GRAPH02_RECALL_DELTA_REL {
        return Err(format!(
            "GRAPH-02 recall delta insufficient: v1={:.3} v2={:.3} abs={:.3} rel={:.3}",
            v1_mean, v2_mean, abs_delta, rel_delta
        ));
    }

    if v2_changes < GRAPH02_MIN_RANK_CHANGES {
        return Err(format!(
            "GRAPH-02 multi-hop changed ranking on {}/{} queries (need >= {})",
            v2_changes,
            all_queries.len(),
            GRAPH02_MIN_RANK_CHANGES
        ));
    }

    Ok(())
}
