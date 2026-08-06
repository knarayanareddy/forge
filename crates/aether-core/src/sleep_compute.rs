//! Sleep-time semantic memory consolidation (Phase 12 / SLEEP-01).
//!
//! Background sleep may write **only** `semantic_memory` rows. Held-out recall@k after
//! consolidation must exceed the no-sleep baseline by a measurable delta.

use aether_db::Database;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SLEEP01_RECALL_K: usize = 3;
pub const SLEEP01_MIN_RECALL_DELTA: f64 = 0.15;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeldOutQuery {
    pub id: String,
    pub query: String,
    pub expected_chunk_id: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepSourceChunk {
    pub chunk_id: String,
    pub source_uri: String,
    pub text: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SleepCycleInput {
    pub session_id: String,
    pub source_chunks: Vec<SleepSourceChunk>,
    pub held_out_queries: Vec<HeldOutQuery>,
    pub summary_chunk_id: String,
    pub summary_source_uri: String,
    pub summary_text: String,
    pub summary_embedding: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SleepCycleResult {
    pub baseline_recall: f64,
    pub post_sleep_recall: f64,
    pub recall_delta: f64,
}

#[derive(Debug, Error)]
pub enum SleepComputeError {
    #[error("database error: {0}")]
    Database(String),
    #[error("embedding dimension must be 384, got {0}")]
    BadDimension(usize),
}

pub fn recall_at_k(
    db: &Database,
    query_text: &str,
    query_embedding: &[f32],
    expected_chunk_id: &str,
    k: usize,
) -> Result<f64, SleepComputeError> {
    if query_embedding.len() != 384 {
        return Err(SleepComputeError::BadDimension(query_embedding.len()));
    }
    let results = db
        .search_semantic_memory_hybrid(query_text, query_embedding, k)
        .map_err(|e| SleepComputeError::Database(e.to_string()))?;
    Ok(if results
        .iter()
        .any(|(chunk_id, _, _)| chunk_id == expected_chunk_id)
    {
        1.0
    } else {
        0.0
    })
}

pub fn mean_recall_at_k(
    db: &Database,
    queries: &[HeldOutQuery],
    k: usize,
) -> Result<f64, SleepComputeError> {
    if queries.is_empty() {
        return Ok(0.0);
    }
    let sum = queries
        .iter()
        .map(|q| {
            recall_at_k(
                db,
                &q.query,
                &q.embedding,
                &q.expected_chunk_id,
                k,
            )
        })
        .try_fold(0.0, |acc, r| r.map(|v| acc + v))?;
    Ok(sum / queries.len() as f64)
}

pub fn run_sleep_memory_cycle(
    db: &Database,
    input: &SleepCycleInput,
) -> Result<SleepCycleResult, SleepComputeError> {
    for chunk in &input.source_chunks {
        if chunk.embedding.len() != 384 {
            return Err(SleepComputeError::BadDimension(chunk.embedding.len()));
        }
        db.insert_memory_chunk(
            &chunk.chunk_id,
            &chunk.source_uri,
            &chunk.text,
            &chunk.embedding,
        )
        .map_err(|e| SleepComputeError::Database(e.to_string()))?;
    }

    let baseline_recall = mean_recall_at_k(db, &input.held_out_queries, SLEEP01_RECALL_K)?;

    if input.summary_embedding.len() != 384 {
        return Err(SleepComputeError::BadDimension(
            input.summary_embedding.len(),
        ));
    }
    db.insert_memory_chunk(
        &input.summary_chunk_id,
        &input.summary_source_uri,
        &input.summary_text,
        &input.summary_embedding,
    )
    .map_err(|e| SleepComputeError::Database(e.to_string()))?;

    let post_sleep_recall = mean_recall_at_k(db, &input.held_out_queries, SLEEP01_RECALL_K)?;
    Ok(SleepCycleResult {
        baseline_recall,
        post_sleep_recall,
        recall_delta: post_sleep_recall - baseline_recall,
    })
}
