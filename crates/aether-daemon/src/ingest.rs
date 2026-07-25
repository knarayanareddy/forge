//! Session turn ingest hook (Meetily pipeline borrow).
//!
//! Normalizes transcript text into bounded batches, runs Ollama `graph_extract` (Slice 6.4),
//! validates, and inserts wiki-zone graph rows. Failures are audit-logged — never silently skipped.

use aether_core::{
    payload_to_graph_inserts, run_graph_extract, GraphExtractPayload, ModelRouter,
};
use aether_db::Database;
use aether_permissions::{PermissionDecision, PermissionManager};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default max estimated tokens per ingest batch (~4 chars/token heuristic).
pub const DEFAULT_MAX_TOKENS_PER_BATCH: usize = 4096;

/// Default cap on entities extracted per turn (Slice 6.4 prompt bound).
pub const DEFAULT_MAX_ENTITIES_PER_TURN: usize = 32;

/// Conservative chars-per-token estimate for batch sizing (no tokenizer dependency).
pub const ESTIMATED_CHARS_PER_TOKEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsyncFailurePolicy {
    /// Log to audit_log and continue session (default for post-turn hook).
    LogAndContinue,
    /// Propagate error to caller (fail-closed).
    FailClosed,
}

impl Default for AsyncFailurePolicy {
    fn default() -> Self {
        Self::LogAndContinue
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestConfig {
    pub max_tokens_per_batch: usize,
    pub max_entities_per_turn: usize,
    pub async_failure_policy: AsyncFailurePolicy,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            max_tokens_per_batch: DEFAULT_MAX_TOKENS_PER_BATCH,
            max_entities_per_turn: DEFAULT_MAX_ENTITIES_PER_TURN,
            async_failure_policy: AsyncFailurePolicy::LogAndContinue,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestBatch {
    pub session_id: String,
    pub turn_index: u32,
    pub normalized_text: String,
    pub estimated_tokens: usize,
    pub truncated: bool,
    pub max_entities: usize,
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum IngestError {
    #[error("Empty turn text after normalization")]
    EmptyTurn,
    #[error("Database audit error: {0}")]
    Audit(String),
    #[error("Graph insert error: {0}")]
    GraphInsert(String),
    #[error("Ingest failed: {0}")]
    Failed(String),
}

/// Post-turn hook contract for Slice 6.4 wiring (`transcript → Ollama → graph insert`).
pub trait IngestHook {
    fn on_session_turn(
        &self,
        conn: &Connection,
        session_id: &str,
        text: &str,
        turn_index: u32,
    ) -> Result<IngestBatch, IngestError>;
}

pub struct SessionIngest {
    pub config: IngestConfig,
}

impl SessionIngest {
    pub fn new(config: IngestConfig) -> Self {
        Self { config }
    }

    /// Normalize whitespace and bound text for graph_extract input.
    pub fn normalize_turn_text(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    pub fn estimate_tokens(text: &str) -> usize {
        let chars = text.chars().count();
        (chars + ESTIMATED_CHARS_PER_TOKEN - 1) / ESTIMATED_CHARS_PER_TOKEN
    }

    fn truncate_to_token_budget(text: &str, max_tokens: usize) -> (String, bool) {
        let max_chars = max_tokens.saturating_mul(ESTIMATED_CHARS_PER_TOKEN);
        if text.chars().count() <= max_chars {
            return (text.to_string(), false);
        }
        let truncated: String = text.chars().take(max_chars).collect();
        (truncated, true)
    }

    pub fn audit_ingest_failure(
        conn: &Connection,
        session_id: &str,
        turn_index: u32,
        reason: &str,
    ) -> Result<(), IngestError> {
        let args = serde_json::json!({
            "turn_index": turn_index,
            "reason": reason,
        })
        .to_string();
        PermissionManager::audit_decision(
            conn,
            session_id,
            "ingest_turn",
            &args,
            &PermissionDecision::Denied,
            Some(1),
            None,
        )
        .map_err(|e| IngestError::Audit(e.to_string()))
    }

    /// Next monotonic turn index for a session (1-based user turn count + 1).
    pub fn next_turn_index(conn: &Connection, session_id: &str) -> Result<u32, IngestError> {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM conversations WHERE session_id = ?1 AND role = 'user'",
                rusqlite::params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| IngestError::Failed(e.to_string()))?;
        Ok((count as u32).saturating_add(1))
    }

    /// Record user + assistant messages in the raw zone (`conversations`).
    pub fn record_conversation_turn(
        conn: &Connection,
        session_id: &str,
        turn_index: u32,
        user_text: &str,
        assistant_text: &str,
    ) -> Result<(), IngestError> {
        let user_id = format!("{session_id}-u-{turn_index}");
        let assistant_id = format!("{session_id}-a-{turn_index}");
        conn.execute(
            "INSERT INTO conversations (id, session_id, role, content) VALUES (?1, ?2, 'user', ?3)",
            rusqlite::params![user_id, session_id, user_text],
        )
        .map_err(|e| IngestError::Failed(e.to_string()))?;
        conn.execute(
            "INSERT INTO conversations (id, session_id, role, content) VALUES (?1, ?2, 'assistant', ?3)",
            rusqlite::params![assistant_id, session_id, assistant_text],
        )
        .map_err(|e| IngestError::Failed(e.to_string()))?;
        Ok(())
    }

    fn format_transcript(user_text: &str, assistant_text: &str) -> String {
        format!("User: {user_text}\n\nAssistant: {assistant_text}")
    }

    fn handle_failure(
        &self,
        conn: &Connection,
        session_id: &str,
        turn_index: u32,
        reason: &str,
    ) -> Result<(), IngestError> {
        let _ = Self::audit_ingest_failure(conn, session_id, turn_index, reason);
        match self.config.async_failure_policy {
            AsyncFailurePolicy::FailClosed => Err(IngestError::Failed(reason.to_string())),
            AsyncFailurePolicy::LogAndContinue => Ok(()),
        }
    }

    fn namespace_payload(
        session_id: &str,
        turn_index: u32,
        payload: GraphExtractPayload,
    ) -> GraphExtractPayload {
        let mut id_map = std::collections::HashMap::new();
        let nodes = payload
            .nodes
            .into_iter()
            .map(|mut node| {
                let namespaced = format!("{session_id}::t{turn_index}::{}", node.id);
                id_map.insert(node.id.clone(), namespaced.clone());
                node.id = namespaced;
                node
            })
            .collect();
        let edges = payload
            .edges
            .into_iter()
            .map(|mut edge| {
                if let Some(src) = id_map.get(&edge.src_node_id) {
                    edge.src_node_id = src.clone();
                }
                if let Some(dst) = id_map.get(&edge.dst_node_id) {
                    edge.dst_node_id = dst.clone();
                }
                edge
            })
            .collect();
        GraphExtractPayload { nodes, edges }
    }

    fn insert_graph_payload(
        db: &Database,
        session_id: &str,
        turn_index: u32,
        payload: GraphExtractPayload,
    ) -> Result<(usize, usize), IngestError> {
        let source_uri = format!("memory://turn/{turn_index}");
        let (nodes, edges) = payload_to_graph_inserts(&payload, &source_uri)
            .map_err(|e| IngestError::Failed(e.to_string()))?;

        for node in &nodes {
            db.insert_graph_node(node.as_new(session_id))
                .map_err(|e| IngestError::GraphInsert(e.to_string()))?;
        }
        for edge in &edges {
            db.insert_graph_edge(edge.as_new(session_id))
                .map_err(|e| IngestError::GraphInsert(e.to_string()))?;
        }

        Ok((nodes.len(), edges.len()))
    }
}

impl IngestHook for SessionIngest {
    fn on_session_turn(
        &self,
        conn: &Connection,
        session_id: &str,
        text: &str,
        turn_index: u32,
    ) -> Result<IngestBatch, IngestError> {
        let normalized = Self::normalize_turn_text(text);
        if normalized.is_empty() {
            let err = IngestError::EmptyTurn;
            let _ = Self::audit_ingest_failure(conn, session_id, turn_index, &err.to_string());
            return Err(err);
        }

        let (bounded, truncated) =
            Self::truncate_to_token_budget(&normalized, self.config.max_tokens_per_batch);
        let estimated_tokens = Self::estimate_tokens(&bounded);

        Ok(IngestBatch {
            session_id: session_id.to_string(),
            turn_index,
            normalized_text: bounded,
            estimated_tokens,
            truncated,
            max_entities: self.config.max_entities_per_turn,
        })
    }
}

/// Convenience entry for daemon post-turn batch normalization.
pub fn on_session_turn(
    conn: &Connection,
    config: &IngestConfig,
    session_id: &str,
    text: &str,
    turn_index: u32,
) -> Result<IngestBatch, IngestError> {
    SessionIngest::new(config.clone()).on_session_turn(conn, session_id, text, turn_index)
}

/// Full Slice 6.4 path: normalize → Ollama graph_extract → validate → insert graph rows.
pub async fn ingest_turn_with_graph_extract(
    db: &Database,
    router: &ModelRouter,
    config: &IngestConfig,
    session_id: &str,
    text: &str,
    turn_index: u32,
) -> Result<IngestBatch, IngestError> {
    let ingest = SessionIngest::new(config.clone());

    let batch = {
        let conn = db.conn();
        match ingest.on_session_turn(&conn, session_id, text, turn_index) {
            Ok(batch) => batch,
            Err(e) => {
                let _ = ingest.handle_failure(&conn, session_id, turn_index, &e.to_string());
                return Err(e);
            }
        }
    };

    let extract_result =
        run_graph_extract(router, &batch.normalized_text, batch.max_entities).await;

    let payload = match extract_result {
        Ok(payload) => payload,
        Err(e) => {
            let conn = db.conn();
            let _ = ingest.handle_failure(&conn, session_id, turn_index, &e.to_string());
            return Err(IngestError::Failed(e.to_string()));
        }
    };

    let namespaced = SessionIngest::namespace_payload(session_id, turn_index, payload);
    match SessionIngest::insert_graph_payload(db, session_id, turn_index, namespaced) {
        Ok((node_count, edge_count)) => {
            tracing::info!(
                session_id = %session_id,
                turn_index = turn_index,
                node_count = node_count,
                edge_count = edge_count,
                "graph_extract ingest complete"
            );
            Ok(batch)
        }
        Err(e) => {
            let conn = db.conn();
            let _ = ingest.handle_failure(&conn, session_id, turn_index, &e.to_string());
            Err(e)
        }
    }
}

/// Post-turn hook invoked by daemon after stream/loop completion.
pub async fn post_turn_graph_ingest(
    db: &Database,
    router: &ModelRouter,
    config: &IngestConfig,
    session_id: &str,
    user_text: &str,
    assistant_text: &str,
) {
    let turn_index = {
        let conn = db.conn();
        let turn_index = match SessionIngest::next_turn_index(&conn, session_id) {
            Ok(idx) => idx,
            Err(e) => {
                tracing::warn!(
                    session_id = %session_id,
                    error = %e,
                    "ingest turn index lookup failed"
                );
                return;
            }
        };

        if let Err(e) = SessionIngest::record_conversation_turn(
            &conn,
            session_id,
            turn_index,
            user_text,
            assistant_text,
        ) {
            tracing::warn!(
                session_id = %session_id,
                turn_index = turn_index,
                error = %e,
                "conversation record failed"
            );
        }
        turn_index
    };

    let transcript = SessionIngest::format_transcript(user_text, assistant_text);
    if let Err(e) = ingest_turn_with_graph_extract(
        db,
        router,
        config,
        session_id,
        &transcript,
        turn_index,
    )
    .await
    {
        tracing::warn!(
            session_id = %session_id,
            turn_index = turn_index,
            error = %e,
            "graph ingest failed (audit logged)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_core::{enforce_max_entities, validate_graph_extract};
    use aether_db::Database;

    const VALID_PAYLOAD: &str = r#"{
        "nodes": [
            {
                "id": "node-forge",
                "entity_type": "project",
                "canonical_name": "AetherForge",
                "provenance": "extracted",
                "evidence_text": "The user mentioned AetherForge."
            }
        ],
        "edges": []
    }"#;

    #[test]
    fn normalizes_and_bounds_turn_text() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('sess-ingest', 'Ingest', 'active')",
            [],
        )
        .unwrap();

        let ingest = SessionIngest::new(IngestConfig {
            max_tokens_per_batch: 10,
            ..Default::default()
        });

        let batch = ingest
            .on_session_turn(
                &conn,
                "sess-ingest",
                "  Alex   maintains   the   AetherForge   daemon.  ",
                1,
            )
            .unwrap();

        assert_eq!(
            batch.normalized_text,
            "Alex maintains the AetherForge daemon."
        );
        assert!(batch.estimated_tokens <= 10);
        assert_eq!(batch.max_entities, DEFAULT_MAX_ENTITIES_PER_TURN);
    }

    #[test]
    fn empty_turn_audits_and_fails() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('sess-empty', 'Ingest', 'active')",
            [],
        )
        .unwrap();

        let ingest = SessionIngest::default_config();
        let err = ingest
            .on_session_turn(&conn, "sess-empty", "   \n\t  ", 2)
            .unwrap_err();
        assert_eq!(err, IngestError::EmptyTurn);

        let audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE tool_name = 'ingest_turn' AND decision = 'denied'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(audit_count, 1);
    }

    #[test]
    fn truncation_flag_set_when_over_budget() {
        let long = "word ".repeat(200);
        let (text, truncated) = SessionIngest::truncate_to_token_budget(&long, 5);
        assert!(truncated);
        assert!(SessionIngest::estimate_tokens(&text) <= 5);
    }

    #[test]
    fn next_turn_index_counts_user_messages() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('sess-turns', 'T', 'active')",
            [],
        )
        .unwrap();
        assert_eq!(SessionIngest::next_turn_index(&conn, "sess-turns").unwrap(), 1);

        conn.execute(
            "INSERT INTO conversations (id, session_id, role, content) VALUES ('c1', 'sess-turns', 'user', 'hi')",
            [],
        )
        .unwrap();
        assert_eq!(SessionIngest::next_turn_index(&conn, "sess-turns").unwrap(), 2);
    }

    #[test]
    fn validated_payload_inserts_graph_rows_without_ollama() {
        let db = Database::open_in_memory().unwrap();
        {
            let conn = db.conn();
            conn.execute(
                "INSERT INTO sessions (id, title, status) VALUES ('sess-graph', 'G', 'active')",
                [],
            )
            .unwrap();
        }

        let payload = validate_graph_extract(VALID_PAYLOAD).unwrap();
        enforce_max_entities(&payload, DEFAULT_MAX_ENTITIES_PER_TURN).unwrap();
        let namespaced = SessionIngest::namespace_payload("sess-graph", 1, payload);
        let (nodes, edges) =
            SessionIngest::insert_graph_payload(&db, "sess-graph", 1, namespaced).unwrap();
        assert_eq!(nodes, 1);
        assert_eq!(edges, 0);

        let stored = db.get_graph_node("sess-graph::t1::node-forge").unwrap();
        assert!(stored.is_some());
    }

    #[test]
    fn graph_insert_failure_is_audited() {
        let db = Database::open_in_memory().unwrap();
        {
            let conn = db.conn();
            conn.execute(
                "INSERT INTO sessions (id, title, status) VALUES ('sess-audit', 'A', 'active')",
                [],
            )
            .unwrap();
        }

        let payload = validate_graph_extract(VALID_PAYLOAD).unwrap();
        let namespaced = SessionIngest::namespace_payload("sess-audit", 1, payload);
        let err =
            SessionIngest::insert_graph_payload(&db, "missing-session", 1, namespaced).unwrap_err();
        assert!(matches!(err, IngestError::GraphInsert(_)));

        let conn = db.conn();
        SessionIngest::audit_ingest_failure(&conn, "sess-audit", 1, &err.to_string()).unwrap();
        let audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE tool_name = 'ingest_turn'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(audit_count, 1);
    }
}

impl SessionIngest {
    fn default_config() -> Self {
        Self::new(IngestConfig::default())
    }
}
