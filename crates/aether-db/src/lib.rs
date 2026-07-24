use rusqlite::{Connection, Result};
use std::path::Path;
use std::cell::RefCell;

pub struct Database {
    conn: RefCell<Connection>,
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(
                std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ())
            ));
        }

        let conn = Connection::open(path)?;

        // Enable WAL mode & foreign keys
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
        ")?;

        let db = Self { conn: RefCell::new(conn) };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(
                std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ())
            ));
        }

        let conn = Connection::open_in_memory()?;
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
        ")?;
        
        let db = Self { conn: RefCell::new(conn) };
        db.init_schema()?;
        Ok(db)
    }

    pub fn init_schema(&self) -> Result<()> {
        let conn = self.conn.borrow();
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
                embedding float[384]
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

    /// Exposes a temporary Ref to Connection for operations like conn.execute()
    pub fn conn(&self) -> std::cell::Ref<'_, Connection> {
        self.conn.borrow()
    }

    /// Inserts a semantic memory chunk and its corresponding vector embedding atomically (Spec §3 protocol)
    pub fn insert_memory_chunk(
        &self,
        chunk_id: &str,
        source_uri: &str,
        chunk_text: &str,
        embedding: &[f32],
    ) -> Result<i64> {
        assert_eq!(embedding.len(), 384, "Embedding dimension must be exactly 384");
        
        let mut conn = self.conn.borrow_mut();
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

    /// Performs semantic vector search using true mathematical Cosine Similarity in Rust over fetched vectors
    pub fn search_semantic_memory(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>> {
        assert_eq!(query_embedding.len(), 384, "Query embedding dimension must be exactly 384");

        let conn = self.conn.borrow();
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

            // Reconstruct &[f32] from blob bytes
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

        // Sort descending by cosine similarity
        scored_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored_results.truncate(limit);

        Ok(scored_results)
    }
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
    fn test_vector_insertion_and_search() {
        let db = Database::open_in_memory().unwrap();
        let embedding = vec![0.1f32; 384];
        let id = db.insert_memory_chunk("chunk-1", "file://test.txt", "Hello AetherForge", &embedding).unwrap();
        assert!(id > 0);

        let results = db.search_semantic_memory(&embedding, 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "chunk-1");
        assert!(results[0].2 > 0.85);
    }
}
