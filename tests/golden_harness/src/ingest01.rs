//! INGEST-01 — live Ollama graph_extract on a fresh transcript (Phase 8.2–8.3).
//!
//! Anti-theater: the fixture must not contain `extract_json`. Entities must come from
//! production `ingest_turn_with_graph_extract` (schema-constrained Ollama), then recall@1
//! on the ingested session memory must surface the distinctive transcript fact.

use aether_core::{ModelBackend, ModelRouter, OllamaProvider};
use aether_daemon::ingest::{ingest_turn_with_graph_extract, AsyncFailurePolicy, IngestConfig};
use aether_db::Database;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const INGEST01_FIXTURE_PATH: &str = "tests/golden_harness/fixtures/ingest01_transcript.json";
pub const INGEST01_MAX_ATTEMPTS: usize = 3;
pub const INGEST01_RECALL_K: usize = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct Ingest01Fixture {
    pub schema_version: u32,
    pub session_id: String,
    pub user_text: String,
    pub assistant_text: String,
    pub expected_entity_names: Vec<String>,
    pub gold_query: String,
    pub recall_must_contain: String,
}

pub fn ingest01_fixture_ready() -> Result<usize, String> {
    let fixture = load_ingest01_fixture()?;
    Ok(fixture.expected_entity_names.len())
}

pub fn load_ingest01_fixture() -> Result<Ingest01Fixture, String> {
    let path = resolve_fixture_path()?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;

    // Anti-theater: refuse any fixture that embeds a frozen extract seed.
    let raw: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("parse INGEST-01 fixture: {e}"))?;
    if raw.get("extract_json").is_some() {
        return Err(
            "INGEST-01 fixture must not contain extract_json (live Ollama extract only)".into(),
        );
    }

    let fixture: Ingest01Fixture =
        serde_json::from_value(raw).map_err(|e| format!("parse INGEST-01 fixture: {e}"))?;
    validate_fixture(&fixture)?;
    Ok(fixture)
}

fn resolve_fixture_path() -> Result<PathBuf, String> {
    let candidates = [
        Path::new(INGEST01_FIXTURE_PATH),
        Path::new("fixtures/ingest01_transcript.json"),
        Path::new("tests/golden_harness/fixtures/ingest01_transcript.json"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(format!(
        "INGEST-01 fixture not found (tried {:?})",
        candidates
    ))
}

fn validate_fixture(fixture: &Ingest01Fixture) -> Result<(), String> {
    if fixture.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {} (expected 1)",
            fixture.schema_version
        ));
    }
    if fixture.user_text.trim().is_empty() || fixture.assistant_text.trim().is_empty() {
        return Err("INGEST-01 fixture requires non-empty user_text and assistant_text".into());
    }
    if fixture.expected_entity_names.len() < 2 {
        return Err("INGEST-01 requires ≥2 expected_entity_names".into());
    }
    if fixture.gold_query.trim().is_empty() || fixture.recall_must_contain.trim().is_empty() {
        return Err("INGEST-01 requires gold_query and recall_must_contain".into());
    }
    Ok(())
}

fn alnum_lower(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn entity_mentioned(blob_alnum: &str, expected: &str) -> bool {
    let needle = alnum_lower(expected);
    !needle.is_empty() && blob_alnum.contains(&needle)
}

fn clear_session_graph_and_memory(db: &Database, session_id: &str) -> Result<(), String> {
    let conn = db.conn();
    let chunk_prefix = format!("{session_id}::%");
    conn.execute(
        "DELETE FROM graph_chunk_links WHERE chunk_id LIKE ?1",
        rusqlite::params![chunk_prefix],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM semantic_memory WHERE chunk_id LIKE ?1",
        rusqlite::params![chunk_prefix],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM graph_edges WHERE session_id = ?1",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM graph_nodes WHERE session_id = ?1",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn ensure_ollama_ready() -> Result<(String, String, String), String> {
    let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let chat_model =
        std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());
    let embed_model =
        std::env::var("AETHER_EMBED_MODEL").unwrap_or_else(|_| "all-minilm".to_string());

    OllamaProvider::health_check(&endpoint)
        .await
        .map_err(|e| format!("Ollama offline or unreachable: {e}"))?;
    OllamaProvider::warm_chat_model(&endpoint, &chat_model, 2)
        .await
        .map_err(|e| format!("Chat model warmup failed: {e}"))?;
    OllamaProvider::warm_embed_model(&endpoint, &embed_model)
        .await
        .map_err(|e| format!("Embed model warmup failed: {e}"))?;

    Ok((endpoint, chat_model, embed_model))
}

pub async fn test_ingest01_impl(db: &Database) -> Result<(), String> {
    let fixture = load_ingest01_fixture()?;
    let (endpoint, chat_model, embed_model) = ensure_ollama_ready().await?;

    {
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'INGEST-01', 'active')",
            rusqlite::params![fixture.session_id],
        )
        .map_err(|e| e.to_string())?;
    }

    let router = ModelRouter::new(
        ModelBackend::OllamaMlx {
            endpoint: endpoint.clone(),
            model: chat_model,
        },
        None,
    );
    let config = IngestConfig {
        async_failure_policy: AsyncFailurePolicy::FailClosed,
        max_entities_per_turn: 12,
        ..Default::default()
    };

    // Same transcript shape as `SessionIngest::format_transcript` / post_turn_graph_ingest.
    let transcript = format!(
        "User: {}\n\nAssistant: {}",
        fixture.user_text.trim(),
        fixture.assistant_text.trim()
    );

    let mut last_err = String::new();
    let mut node_blob = String::new();
    let mut node_count = 0i64;

    for attempt in 1..=INGEST01_MAX_ATTEMPTS {
        clear_session_graph_and_memory(db, &fixture.session_id)?;
        match ingest_turn_with_graph_extract(
            db,
            &router,
            &config,
            &fixture.session_id,
            &transcript,
            1,
        )
        .await
        {
            Ok(_) => {}
            Err(e) => {
                last_err = format!("attempt {attempt}: ingest error: {e}");
                continue;
            }
        }

        let (count, blob) = {
            let conn = db.conn();
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM graph_nodes WHERE session_id = ?1",
                    rusqlite::params![fixture.session_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT id, canonical_name, COALESCE(properties_json,''), COALESCE(source_uri,'') \
                     FROM graph_nodes WHERE session_id = ?1",
                )
                .map_err(|e| e.to_string())?;
            let mut blob = String::new();
            let rows = stmt
                .query_map(rusqlite::params![fixture.session_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (id, name, props, uri) = row.map_err(|e| e.to_string())?;
                blob.push_str(&id);
                blob.push(' ');
                blob.push_str(&name);
                blob.push(' ');
                blob.push_str(&props);
                blob.push(' ');
                blob.push_str(&uri);
                blob.push('\n');
            }
            let mem_text: String = conn
                .query_row(
                    "SELECT COALESCE(group_concat(chunk_text, ' '), '') FROM semantic_memory WHERE chunk_id LIKE ?1",
                    rusqlite::params![format!("{}::%", fixture.session_id)],
                    |row| row.get(0),
                )
                .unwrap_or_default();
            blob.push_str(&mem_text);
            (count, blob)
        };

        if count < 1 {
            last_err = format!("attempt {attempt}: live extract inserted zero graph_nodes");
            continue;
        }

        let blob_alnum = alnum_lower(&blob);
        let missing: Vec<String> = fixture
            .expected_entity_names
            .iter()
            .filter(|expected| !entity_mentioned(&blob_alnum, expected))
            .cloned()
            .collect();
        if !missing.is_empty() {
            last_err = format!(
                "attempt {attempt}: missing entities {:?}; blob sample: {}",
                missing,
                blob.chars().take(400).collect::<String>()
            );
            continue;
        }

        let namespaced: i64 = {
            let conn = db.conn();
            conn.query_row(
                "SELECT COUNT(*) FROM graph_nodes WHERE session_id = ?1 AND id LIKE ?2",
                rusqlite::params![fixture.session_id, format!("{}::t1::%", fixture.session_id)],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?
        };
        if namespaced < 1 {
            last_err = format!(
                "attempt {attempt}: expected session-namespaced node ids from production ingest"
            );
            continue;
        }

        node_count = count;
        node_blob = blob;
        last_err.clear();
        break;
    }

    if !last_err.is_empty() {
        return Err(format!(
            "INGEST-01 live extract failed after {INGEST01_MAX_ATTEMPTS} attempts: {last_err}"
        ));
    }
    let _ = (node_count, &node_blob);

    let query_emb = aether_core::fetch_ollama_embedding(&endpoint, &embed_model, &fixture.gold_query)
        .await
        .map_err(|e| format!("INGEST-01 query embed failed: {e}"))?;

    let hits = db
        .search_hybrid_with_graph(
            &fixture.session_id,
            &fixture.gold_query,
            &query_emb,
            INGEST01_RECALL_K,
        )
        .map_err(|e| format!("INGEST-01 hybrid search failed: {e}"))?;

    if hits.is_empty() {
        return Err("INGEST-01 recall@1 returned no chunks".into());
    }

    let (_chunk_id, top_text, _score) = &hits[0];
    if !top_text.contains(&fixture.recall_must_contain) {
        return Err(format!(
            "INGEST-01 recall@1 missed '{}' in top chunk (got: {})",
            fixture.recall_must_contain,
            top_text.chars().take(240).collect::<String>()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_loads_without_extract_json() {
        let fixture = load_ingest01_fixture().expect("fixture");
        assert!(fixture.expected_entity_names.len() >= 2);
        assert!(fixture.user_text.contains("NebulaLedger"));
    }

    #[test]
    fn entity_match_tolerates_slugified_names() {
        let blob = alnum_lower("node-mira-chen-acme | MiraChen | nebulaledger-daemon orchid9");
        assert!(entity_mentioned(&blob, "Mira Chen"));
        assert!(entity_mentioned(&blob, "NebulaLedger"));
        assert!(entity_mentioned(&blob, "Orchid-9"));
    }
}
