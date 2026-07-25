mod consolidate;
mod consolidate_review;
mod graph;
mod recovery;

pub use consolidate::ConsolidationRunRecord;
pub use consolidate_review::{
    format_consolidate_review, ConsolidateEdgeDiff, ConsolidateNodeDiff, ConsolidatePreview,
    EdgeAction, NodeAction,
};
pub use graph::{
    EntityType, GraphEdge, GraphNeighbor, GraphNode, NewGraphEdge, NewGraphNode, QueryPolicy,
    RelationType,
};
pub use recovery::{RecoveryManager, RecoveryReport};

use rusqlite::{Connection, Result};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        register_sqlite_vec();

        let conn = Connection::open(path)?;

        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
        ")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        RecoveryManager::recover_on_startup(&db.conn.lock().unwrap())?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        register_sqlite_vec();

        let conn = Connection::open_in_memory()?;
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
        ")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        RecoveryManager::recover_on_startup(&db.conn.lock().unwrap())?;
        Ok(db)
    }

    pub fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('active', 'paused', 'completed', 'failed')),
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS capability_grants (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT,
                resource_path TEXT NOT NULL,
                bookmark_data BLOB,
                is_stale BOOLEAN DEFAULT 0,
                permission_type TEXT NOT NULL CHECK(permission_type IN ('read', 'write', 'execute', 'mcp_call')),
                granted_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL CHECK(role IN ('system', 'user', 'assistant', 'tool')),
                content TEXT NOT NULL,
                tokens_used INTEGER DEFAULT 0,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS semantic_memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chunk_id TEXT UNIQUE NOT NULL,
                source_uri TEXT NOT NULL,
                model_id TEXT NOT NULL DEFAULT 'all-MiniLM-L6-v2',
                dimension INTEGER NOT NULL DEFAULT 384 CHECK(dimension = 384),
                chunk_text TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS semantic_memory_fts USING fts5(
                chunk_text,
                content='semantic_memory',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS semantic_memory_ai AFTER INSERT ON semantic_memory BEGIN
                INSERT INTO semantic_memory_fts(rowid, chunk_text) VALUES (new.id, new.chunk_text);
            END;

            CREATE TRIGGER IF NOT EXISTS semantic_memory_ad AFTER DELETE ON semantic_memory BEGIN
                INSERT INTO semantic_memory_fts(semantic_memory_fts, rowid, chunk_text) VALUES('delete', old.id, old.chunk_text);
            END;

            CREATE TRIGGER IF NOT EXISTS semantic_memory_au AFTER UPDATE ON semantic_memory BEGIN
                INSERT INTO semantic_memory_fts(semantic_memory_fts, rowid, chunk_text) VALUES('delete', old.id, old.chunk_text);
                INSERT INTO semantic_memory_fts(rowid, chunk_text) VALUES (new.id, new.chunk_text);
            END;

            CREATE VIRTUAL TABLE IF NOT EXISTS semantic_memory_vec USING vec0(
                embedding float[384] distance_metric=cosine
            );

            CREATE TABLE IF NOT EXISTS procedural_skills (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                description TEXT NOT NULL,
                markdown_body TEXT NOT NULL,
                success_count INTEGER DEFAULT 0,
                failure_count INTEGER DEFAULT 0,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS undo_journal (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                op_type TEXT NOT NULL CHECK(op_type IN ('file_write', 'file_delete', 'file_rename')),
                target_path TEXT NOT NULL,
                inverse_patch TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('pending', 'applied', 'reverted')) DEFAULT 'pending',
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                arguments_json TEXT NOT NULL,
                decision TEXT NOT NULL CHECK(decision IN ('approved', 'denied', 'auto_allowed')),
                exit_code INTEGER,
                execution_duration_ms INTEGER,
                prev_hash TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_conv_session ON conversations(session_id);
            CREATE INDEX IF NOT EXISTS idx_audit_session ON audit_log(session_id);
            CREATE INDEX IF NOT EXISTS idx_grants_session ON capability_grants(session_id);
            CREATE INDEX IF NOT EXISTS idx_undo_session ON undo_journal(session_id);

            CREATE TABLE IF NOT EXISTS graph_nodes (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                entity_type TEXT NOT NULL CHECK(entity_type IN (
                    'person', 'project', 'concept', 'file', 'tool', 'event', 'other'
                )),
                canonical_name TEXT NOT NULL,
                aliases_json TEXT NOT NULL DEFAULT '[]',
                properties_json TEXT NOT NULL DEFAULT '{}',
                source_uri TEXT NOT NULL,
                valid_from TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                valid_to TIMESTAMP,
                recorded_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                superseded_by TEXT,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
                FOREIGN KEY(superseded_by) REFERENCES graph_nodes(id) ON DELETE SET NULL
            );

            CREATE INDEX IF NOT EXISTS idx_graph_nodes_session ON graph_nodes(session_id);
            CREATE INDEX IF NOT EXISTS idx_graph_nodes_name ON graph_nodes(canonical_name);
            CREATE INDEX IF NOT EXISTS idx_graph_nodes_valid ON graph_nodes(valid_from, valid_to);

            CREATE TABLE IF NOT EXISTS graph_edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                src_node_id TEXT NOT NULL,
                dst_node_id TEXT NOT NULL,
                relation_type TEXT NOT NULL CHECK(relation_type IN (
                    'related_to', 'part_of', 'authored_by', 'depends_on',
                    'located_in', 'implements', 'contradicts', 'other'
                )),
                weight REAL NOT NULL DEFAULT 1.0,
                evidence_text TEXT NOT NULL,
                source_uri TEXT NOT NULL,
                valid_from TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                valid_to TIMESTAMP,
                recorded_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
                FOREIGN KEY(src_node_id) REFERENCES graph_nodes(id) ON DELETE CASCADE,
                FOREIGN KEY(dst_node_id) REFERENCES graph_nodes(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_graph_edges_src ON graph_edges(src_node_id);
            CREATE INDEX IF NOT EXISTS idx_graph_edges_dst ON graph_edges(dst_node_id);
            CREATE INDEX IF NOT EXISTS idx_graph_edges_session ON graph_edges(session_id);
            CREATE INDEX IF NOT EXISTS idx_graph_edges_valid ON graph_edges(valid_from, valid_to);

            CREATE TABLE IF NOT EXISTS graph_chunk_links (
                chunk_id TEXT NOT NULL,
                node_id TEXT NOT NULL,
                link_confidence REAL NOT NULL DEFAULT 1.0,
                PRIMARY KEY (chunk_id, node_id),
                FOREIGN KEY(node_id) REFERENCES graph_nodes(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS consolidation_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                finished_at TIMESTAMP,
                status TEXT NOT NULL CHECK(status IN ('running', 'review_pending', 'applied', 'rejected')),
                input_node_count INTEGER NOT NULL,
                output_node_count INTEGER,
                contradiction_count INTEGER DEFAULT 0,
                dedupe_count INTEGER DEFAULT 0,
                review_artifact_path TEXT,
                applied_at TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS query_policy (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                policy_name TEXT UNIQUE NOT NULL,
                rrf_k REAL NOT NULL DEFAULT 60.0,
                graph_hop_depth INTEGER NOT NULL DEFAULT 1 CHECK(graph_hop_depth BETWEEN 0 AND 1),
                fts_weight REAL NOT NULL DEFAULT 1.0,
                vec_weight REAL NOT NULL DEFAULT 1.0,
                graph_weight REAL NOT NULL DEFAULT 1.0,
                max_graph_expansion INTEGER NOT NULL DEFAULT 32,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            INSERT OR IGNORE INTO query_policy (policy_name, graph_hop_depth)
            VALUES ('default', 1);
        ")
    }

    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    pub fn insert_memory_chunk(
        &self,
        chunk_id: &str,
        source_uri: &str,
        chunk_text: &str,
        embedding: &[f32],
    ) -> Result<i64> {
        assert_eq!(embedding.len(), 384, "Embedding dimension must be exactly 384");

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO semantic_memory (chunk_id, source_uri, model_id, dimension, chunk_text) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![chunk_id, source_uri, "all-MiniLM-L6-v2", 384, chunk_text],
        )?;
        let surrogate_id = tx.last_insert_rowid();

        let embedding_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                embedding.as_ptr() as *const u8,
                embedding.len() * std::mem::size_of::<f32>()
            )
        };

        tx.execute(
            "INSERT INTO semantic_memory_vec(rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![surrogate_id, embedding_bytes],
        )?;

        tx.commit()?;
        Ok(surrogate_id)
    }

    /// Hybrid retrieval: Reciprocal Rank Fusion of FTS5 BM25 keyword rank + sqlite-vec KNN.
    pub fn search_semantic_memory_hybrid(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>> {
        assert_eq!(query_embedding.len(), 384, "Query embedding dimension must be exactly 384");

        const RRF_K: f64 = 60.0;
        let fetch = limit.saturating_mul(4).max(limit);

        let fts_ranks = self.search_fts_ranked(query_text, fetch)?;
        let vec_ranks = self.search_semantic_memory_knn(query_embedding, fetch)
            .or_else(|_| self.search_semantic_memory_linear(query_embedding, fetch))?;

        let mut scores: std::collections::HashMap<String, (String, f64)> = std::collections::HashMap::new();
        let mut vec_similarity: std::collections::HashMap<String, f32> = std::collections::HashMap::new();

        for (rank, (chunk_id, chunk_text, _)) in fts_ranks.into_iter().enumerate() {
            let rrf = 1.0 / (RRF_K + (rank as f64) + 1.0);
            scores
                .entry(chunk_id)
                .and_modify(|(_, s)| *s += rrf)
                .or_insert((chunk_text, rrf));
        }

        for (rank, (chunk_id, chunk_text, similarity)) in vec_ranks.into_iter().enumerate() {
            vec_similarity.insert(chunk_id.clone(), similarity);
            let rrf = 1.0 / (RRF_K + (rank as f64) + 1.0);
            scores
                .entry(chunk_id.clone())
                .and_modify(|(_, s)| *s += rrf)
                .or_insert((chunk_text, rrf));
        }

        let mut fused: Vec<(String, String, f64, f32)> = scores
            .into_iter()
            .map(|(id, (text, rrf_score))| {
                let sim = vec_similarity.get(&id).copied().unwrap_or(0.0);
                (id, text, rrf_score, sim)
            })
            .collect();
        fused.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        fused.truncate(limit);
        Ok(fused
            .into_iter()
            .map(|(id, text, _, sim)| (id, text, sim))
            .collect())
    }

    /// Hybrid retrieval with optional 1-hop graph RRF (Phase 6).
    ///
    /// When `query_policy.graph_hop_depth = 0`, returns the same ranking as
    /// [`search_semantic_memory_hybrid`](Self::search_semantic_memory_hybrid).
    pub fn search_hybrid_with_graph(
        &self,
        session_id: &str,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>> {
        self.search_hybrid_with_graph_policy(session_id, query_text, query_embedding, limit, "default")
    }

    pub fn search_hybrid_with_graph_policy(
        &self,
        session_id: &str,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
        policy_name: &str,
    ) -> Result<Vec<(String, String, f32)>> {
        assert_eq!(query_embedding.len(), 384, "Query embedding dimension must be exactly 384");

        let policy = self.get_query_policy(policy_name)?;
        if policy.graph_hop_depth == 0 {
            return self.search_semantic_memory_hybrid(query_text, query_embedding, limit);
        }

        let fetch = limit.saturating_mul(4).max(limit);

        let fts_ranks = self.search_fts_ranked(query_text, fetch)?;
        let vec_ranks = self.search_semantic_memory_knn(query_embedding, fetch)
            .or_else(|_| self.search_semantic_memory_linear(query_embedding, fetch))?;

        let mut seed_chunk_ids: Vec<String> = Vec::new();
        for (chunk_id, _, _) in fts_ranks.iter().chain(vec_ranks.iter()) {
            if !seed_chunk_ids.iter().any(|id| id == chunk_id) {
                seed_chunk_ids.push(chunk_id.clone());
            }
        }

        let seed_refs: Vec<&str> = seed_chunk_ids.iter().map(|s| s.as_str()).collect();
        let seed_node_ids = self.get_node_ids_for_chunks(&seed_refs)?;

        let seed_node_refs: Vec<&str> = seed_node_ids.iter().map(|s| s.as_str()).collect();
        let neighbors = self.get_one_hop_neighbors(session_id, &seed_node_refs, None)?;

        let seed_score_map: std::collections::HashMap<&str, f64> = seed_node_ids
            .iter()
            .map(|id| (id.as_str(), 1.0))
            .collect();

        // Graph RRF ranks chunks linked to 1-hop neighbors only — seed chunks already
        // contribute via FTS/vec; re-ranking them here would double-count.
        let mut node_scores: Vec<(String, f64)> = Vec::new();
        for neighbor in neighbors {
            let src_score = seed_score_map
                .get(neighbor.edge.src_node_id.as_str())
                .copied()
                .unwrap_or(1.0);
            let expanded_score = neighbor.edge.weight * src_score;
            let entry = node_scores
                .iter()
                .position(|(id, _)| id == &neighbor.edge.dst_node_id);
            match entry {
                Some(idx) => {
                    if expanded_score > node_scores[idx].1 {
                        node_scores[idx].1 = expanded_score;
                    }
                }
                None => node_scores.push((neighbor.edge.dst_node_id.clone(), expanded_score)),
            }
        }

        let graph_ranks = if node_scores.is_empty() {
            Vec::new()
        } else {
            self.get_graph_ranked_chunks(
                &node_scores,
                policy.max_graph_expansion as usize,
            )?
        };

        let mut scores: std::collections::HashMap<String, (String, f64)> =
            std::collections::HashMap::new();
        let mut vec_similarity: std::collections::HashMap<String, f32> =
            std::collections::HashMap::new();

        for (rank, (chunk_id, chunk_text, _)) in fts_ranks.into_iter().enumerate() {
            let rrf = policy.fts_weight / (policy.rrf_k + (rank as f64) + 1.0);
            scores
                .entry(chunk_id)
                .and_modify(|(_, s)| *s += rrf)
                .or_insert((chunk_text, rrf));
        }

        for (rank, (chunk_id, chunk_text, similarity)) in vec_ranks.into_iter().enumerate() {
            vec_similarity.insert(chunk_id.clone(), similarity);
            let rrf = policy.vec_weight / (policy.rrf_k + (rank as f64) + 1.0);
            scores
                .entry(chunk_id.clone())
                .and_modify(|(_, s)| *s += rrf)
                .or_insert((chunk_text, rrf));
        }

        for (rank, (chunk_id, chunk_text, _)) in graph_ranks.into_iter().enumerate() {
            let rrf = policy.graph_weight / (policy.rrf_k + (rank as f64) + 1.0);
            scores
                .entry(chunk_id)
                .and_modify(|(_, s)| *s += rrf)
                .or_insert((chunk_text, rrf));
        }

        let mut fused: Vec<(String, String, f64, f32)> = scores
            .into_iter()
            .map(|(id, (text, rrf_score))| {
                let sim = vec_similarity.get(&id).copied().unwrap_or(0.0);
                (id, text, rrf_score, sim)
            })
            .collect();
        fused.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        fused.truncate(limit);
        Ok(fused
            .into_iter()
            .map(|(id, text, _, sim)| (id, text, sim))
            .collect())
    }

    fn search_fts_ranked(
        &self,
        query_text: &str,
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>> {
        let conn = self.conn.lock().unwrap();
        let fts_query = fts5_query_from_text(query_text);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = conn.prepare(
            "SELECT sm.chunk_id, sm.chunk_text, bm25(semantic_memory_fts) AS rank
             FROM semantic_memory_fts
             JOIN semantic_memory sm ON sm.id = semantic_memory_fts.rowid
             WHERE semantic_memory_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let mut rows = stmt.query(rusqlite::params![fts_query, limit as i64])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let chunk_id: String = row.get(0)?;
            let chunk_text: String = row.get(1)?;
            let rank: f64 = row.get(2)?;
            let score = (-rank).max(0.0) as f32;
            results.push((chunk_id, chunk_text, score));
        }
        Ok(results)
    }

    /// Semantic vector search via sqlite-vec KNN `MATCH ... k=N`, with linear cosine fallback.
    pub fn search_semantic_memory(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>> {
        assert_eq!(query_embedding.len(), 384, "Query embedding dimension must be exactly 384");

        match self.search_semantic_memory_knn(query_embedding, limit) {
            Ok(results) if !results.is_empty() => Ok(results),
            _ => self.search_semantic_memory_linear(query_embedding, limit),
        }
    }

    fn search_semantic_memory_knn(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>> {
        let conn = self.conn.lock().unwrap();
        let embedding_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                query_embedding.as_ptr() as *const u8,
                query_embedding.len() * std::mem::size_of::<f32>()
            )
        };

        let mut stmt = conn.prepare(
            "SELECT sm.chunk_id, sm.chunk_text, v.distance
             FROM semantic_memory_vec v
             JOIN semantic_memory sm ON sm.id = v.rowid
             WHERE v.embedding MATCH ?1 AND k = ?2
             ORDER BY v.distance"
        )?;

        let mut rows = stmt.query(rusqlite::params![embedding_bytes, limit as i64])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let chunk_id: String = row.get(0)?;
            let chunk_text: String = row.get(1)?;
            let distance: f64 = row.get(2)?;
            let similarity = (1.0 - distance as f32).clamp(0.0, 1.0);
            results.push((chunk_id, chunk_text, similarity));
        }

        Ok(results)
    }

    fn search_semantic_memory_linear(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT sm.chunk_id, sm.chunk_text, v.embedding
             FROM semantic_memory_vec v
             JOIN semantic_memory sm ON sm.id = v.rowid"
        )?;

        let mut scored_results = Vec::new();
        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let chunk_id: String = row.get(0)?;
            let chunk_text: String = row.get(1)?;
            let embedding_blob: Vec<u8> = row.get(2)?;

            if embedding_blob.len() == 384 * std::mem::size_of::<f32>() {
                let db_vec: &[f32] = unsafe {
                    std::slice::from_raw_parts(
                        embedding_blob.as_ptr() as *const f32,
                        384
                    )
                };

                let similarity = cosine_similarity(query_embedding, db_vec);
                scored_results.push((chunk_id, chunk_text, similarity));
            }
        }

        scored_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored_results.truncate(limit);

        Ok(scored_results)
    }
}

fn register_sqlite_vec() {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(
            std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ())
        ));
    }
}

fn fts5_query_from_text(text: &str) -> String {
    text.split_whitespace()
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()))
        .map(|w| {
            let escaped: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
            if escaped.is_empty() {
                String::new()
            } else {
                format!("\"{}\"", escaped)
            }
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_init() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table';");
        assert!(stmt.is_ok());
    }

    #[test]
    fn test_recovery_marks_pending_reverted() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('sess-rec', 'Recovery', 'active')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
             VALUES ('sess-rec', 'file_rename', '/tmp/x', '{}', 'pending')",
            [],
        )
        .unwrap();

        let report = RecoveryManager::recover_on_startup(&conn).unwrap();
        assert_eq!(report.pending_reverted, 1);

        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM undo_journal WHERE status = 'pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0);

        let reverted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM undo_journal WHERE status = 'reverted'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reverted, 1);
    }

    #[test]
    fn test_hybrid_rrf_prefers_semantic_match() {
        let db = Database::open_in_memory().unwrap();
        let emb_a = vec![1.0f32; 384];
        let mut emb_b = vec![0.0f32; 384];
        emb_b[0] = 1.0;

        db.insert_memory_chunk(
            "chk-fact-a",
            "memory://fact-a",
            "AetherForge secure local Mac agent runtime",
            &emb_a,
        )
        .unwrap();
        db.insert_memory_chunk(
            "chk-fact-b",
            "memory://fact-b",
            "Python data science web backend programming",
            &emb_b,
        )
        .unwrap();

        let query_emb = emb_a.clone();
        let results = db
            .search_semantic_memory_hybrid("AetherForge Mac agent platform", &query_emb, 2)
            .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "chk-fact-a");
    }

    #[test]
    fn test_vector_insertion_and_search() {
        let db = Database::open_in_memory().unwrap();
        let embedding = vec![0.1f32; 384];
        let id = db
            .insert_memory_chunk("chunk-1", "file://test.txt", "Hello AetherForge", &embedding)
            .unwrap();
        assert!(id > 0);

        let results = db.search_semantic_memory(&embedding, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "chunk-1");
        assert!(results[0].2 > 0.85);
    }

    #[test]
    fn test_hybrid_rrf_prefers_keyword_overlap() {
        let db = Database::open_in_memory().unwrap();
        let emb_a = vec![1.0f32; 384];
        let mut emb_b = vec![0.0f32; 384];
        emb_b[0] = 1.0;

        db.insert_memory_chunk(
            "chk-aether",
            "memory://a",
            "AetherForge secure local Mac agent runtime MLX",
            &emb_a,
        )
        .unwrap();
        db.insert_memory_chunk(
            "chk-python",
            "memory://b",
            "Python programming language data science web backend",
            &emb_b,
        )
        .unwrap();

        let query_emb = emb_b.clone();
        let vec_only = db.search_semantic_memory(&query_emb, 2).unwrap();
        assert_eq!(vec_only[0].0, "chk-python");

        let hybrid = db
            .search_semantic_memory_hybrid("AetherForge Mac agent MLX", &query_emb, 2)
            .unwrap();
        assert_eq!(hybrid[0].0, "chk-aether");
        assert!(hybrid[0].2 > 0.0);
    }

    fn seed_session(db: &Database, session_id: &str) {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES (?1, 'Graph hybrid test', 'active')",
            rusqlite::params![session_id],
        )
        .unwrap();
    }

    fn set_graph_hop_depth(db: &Database, depth: i32) {
        let conn = db.conn();
        conn.execute(
            "UPDATE query_policy SET graph_hop_depth = ?1 WHERE policy_name = 'default'",
            rusqlite::params![depth],
        )
        .unwrap();
    }

    fn set_graph_weight(db: &Database, weight: f64) {
        let conn = db.conn();
        conn.execute(
            "UPDATE query_policy SET graph_weight = ?1 WHERE policy_name = 'default'",
            rusqlite::params![weight],
        )
        .unwrap();
    }

    #[test]
    fn test_hybrid_with_graph_hop_zero_parity_with_phase5_hybrid() {
        let db = Database::open_in_memory().unwrap();
        set_graph_hop_depth(&db, 0);

        let emb_a = vec![1.0f32; 384];
        let mut emb_b = vec![0.0f32; 384];
        emb_b[0] = 1.0;

        db.insert_memory_chunk(
            "chk-fact-a",
            "memory://fact-a",
            "AetherForge secure local Mac agent runtime",
            &emb_a,
        )
        .unwrap();
        db.insert_memory_chunk(
            "chk-fact-b",
            "memory://fact-b",
            "Python data science web backend programming",
            &emb_b,
        )
        .unwrap();

        let query_emb = emb_a.clone();
        let query = "AetherForge Mac agent platform";

        let baseline = db
            .search_semantic_memory_hybrid(query, &query_emb, 2)
            .unwrap();
        let with_graph = db
            .search_hybrid_with_graph("unused-session", query, &query_emb, 2)
            .unwrap();

        assert_eq!(with_graph.len(), baseline.len());
        for (graph_hit, base_hit) in with_graph.iter().zip(baseline.iter()) {
            assert_eq!(graph_hit.0, base_hit.0);
            assert_eq!(graph_hit.1, base_hit.1);
            assert!((graph_hit.2 - base_hit.2).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn test_hybrid_with_graph_boosts_graph_linked_chunk() {
        use crate::{EntityType, NewGraphEdge, NewGraphNode, RelationType};

        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-hybrid-graph");
        set_graph_hop_depth(&db, 1);
        set_graph_weight(&db, 3.0);

        let mut emb_noise = vec![0.0f32; 384];
        emb_noise[0] = 1.0;
        let emb_maintainer = vec![0.0f32; 384];

        db.insert_memory_chunk(
            "chk-noise",
            "memory://noise",
            "Generic platform runtime overview",
            &emb_noise,
        )
        .unwrap();
        db.insert_memory_chunk(
            "chk-maintainer",
            "memory://maintainer",
            "Alex keeps the forge service healthy",
            &emb_maintainer,
        )
        .unwrap();

        db.insert_graph_node(NewGraphNode {
            id: "node-forge",
            session_id: "sess-hybrid-graph",
            entity_type: EntityType::Project,
            canonical_name: "AetherForge",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://seed",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();
        db.insert_graph_node(NewGraphNode {
            id: "node-maintainer",
            session_id: "sess-hybrid-graph",
            entity_type: EntityType::Person,
            canonical_name: "Alex Maintainer",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://seed",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        db.insert_graph_edge(NewGraphEdge {
            session_id: "sess-hybrid-graph",
            src_node_id: "node-forge",
            dst_node_id: "node-maintainer",
            relation_type: RelationType::RelatedTo,
            weight: 2.0,
            evidence_text: "Alex maintains AetherForge.",
            source_uri: "memory://seed",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        db.link_graph_chunk("chk-noise", "node-forge", 0.9).unwrap();
        db.link_graph_chunk("chk-maintainer", "node-maintainer", 0.95)
            .unwrap();

        let query_emb = emb_noise.clone();
        let query = "platform runtime overview";

        let baseline = db
            .search_semantic_memory_hybrid(query, &query_emb, 2)
            .unwrap();
        assert_eq!(baseline[0].0, "chk-noise");

        let graph_augmented = db
            .search_hybrid_with_graph("sess-hybrid-graph", query, &query_emb, 2)
            .unwrap();
        assert_eq!(graph_augmented[0].0, "chk-maintainer");
        assert_ne!(graph_augmented[0].0, baseline[0].0);
    }
}
