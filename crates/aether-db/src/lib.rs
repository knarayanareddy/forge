mod recovery;

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
}
