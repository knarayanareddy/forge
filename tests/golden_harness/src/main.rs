use aether_db::Database;
use aether_permissions::{PermissionManager, PermissionDecision, FileMutator};
use std::fs;
use tempfile::tempdir;

mod recovery;
use recovery::CrashRecoveryTest;

mod fs02;
use fs02::test_fs_02_impl;

mod mcp01;
use mcp01::test_mcp_01_impl;

mod skill01;
use skill01::test_skill_01_impl;

mod audit_chain;
use audit_chain::verify_audit_hash_chain;

#[tokio::main]
async fn main() {
    println!("=== AetherForge Golden Task Evaluation Harness ===");
    println!("Constitution: v1.2.4 (Spec-Anchored, Eval-Driven)");
    println!("Active Vertical Slices: SAFE-01, FS-01 + undo_journal, RES-01, FS-02 + Seatbelt, MEM-01, MCP-01, SKILL-01, ROUT-01, GIT-01, CODE-01\n");
    
    let db = Database::open_in_memory().expect("In-memory DB init failed");
    
    let mut passed = 0;
    let total = 10;

    passed += run_task("FS-01", || test_fs_01(&db)).await;
    passed += run_task("FS-02", || test_fs_02()).await;
    passed += run_task("GIT-01", || test_git_01(&db)).await;
    passed += run_task("CODE-01", || test_code_01()).await;
    passed += run_task("MCP-01", || test_mcp_01(&db)).await;
    passed += run_task("MEM-01", || test_mem_01(&db)).await;
    passed += run_task("SKILL-01", || test_skill_01()).await;
    passed += run_task("SAFE-01", || test_safe_01(&db)).await;
    passed += run_task("ROUT-01", || test_rout_01()).await;
    passed += run_task("RES-01", || test_res_01()).await;

    println!("\n=== Evaluation Results ===");
    println!("Passed: {} / {}", passed, total);
}

async fn run_task<F, Fut>(name: &str, test_fn: F) -> i32
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    print!("[-] Running task [{}] ... ", name);
    match test_fn().await {
        Ok(()) => {
            println!("PASS");
            1
        }
        Err(e) => {
            println!("FAIL ({})", e);
            0
        }
    }
}

async fn test_fs_01(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    let session_id = "sess-fs-01";

    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'FS-01 Session', 'active')",
        rusqlite::params![session_id],
    ).map_err(|e| e.to_string())?;

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let dir_path = tmp.path();
    let dir_str = dir_path.to_string_lossy().to_string();

    let ungranted_res = FileMutator::bulk_rename_with_undo(&conn, session_id, dir_path);
    if ungranted_res.is_ok() {
        return Err("Expected ungranted directory rename to be denied".into());
    }

    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, dir_str],
    ).map_err(|e| e.to_string())?;

    let file1 = dir_path.join("test_alpha.txt");
    let file2 = dir_path.join("test_beta.txt");
    let file3 = dir_path.join("other.txt");

    fs::write(&file1, "alpha content").map_err(|e| e.to_string())?;
    fs::write(&file2, "beta content").map_err(|e| e.to_string())?;
    fs::write(&file3, "other content").map_err(|e| e.to_string())?;

    let renames = FileMutator::bulk_rename_with_undo(&conn, session_id, dir_path)?;
    if renames.len() != 2 {
        return Err(format!("Expected 2 renames, got {}", renames.len()));
    }

    assert!(!dir_path.join("test_alpha.txt").exists());
    assert!(dir_path.join("archive_alpha.txt").exists());
    assert!(!dir_path.join("test_beta.txt").exists());
    assert!(dir_path.join("archive_beta.txt").exists());
    assert!(dir_path.join("other.txt").exists());

    FileMutator::rollback(&conn, session_id)?;

    assert!(dir_path.join("test_alpha.txt").exists());
    assert!(!dir_path.join("archive_alpha.txt").exists());
    assert!(dir_path.join("test_beta.txt").exists());
    assert!(!dir_path.join("archive_beta.txt").exists());

    Ok(())
}

async fn test_fs_02() -> Result<(), String> {
    test_fs_02_impl().await
}

async fn test_git_01(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    let session_id = "sess-git-01";

    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'GIT-01 Session', 'active')",
        rusqlite::params![session_id],
    ).map_err(|e| e.to_string())?;

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path();
    let workspace_str = workspace.to_string_lossy().to_string();

    let denied = PermissionManager::check_file_access(&conn, session_id, &workspace_str, "write")
        .map_err(|e| e.to_string())?;
    if denied != PermissionDecision::Denied {
        return Err("Expected ungranted workspace write to be denied".into());
    }

    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace_str],
    ).map_err(|e| e.to_string())?;

    aether_core::GitOps::init_commit_and_branch(workspace, "feature/git-01")
        .map_err(|e| e.to_string())?;

    if !workspace.join(".git").exists() {
        return Err("git init did not create .git directory".into());
    }

    Ok(())
}

async fn test_code_01() -> Result<(), String> {
    let bad_snippet = "def broken(\n    x = 1\n    return x\n";

    let issues = aether_core::PythonLinter::check_syntax(bad_snippet)
        .map_err(|e| e.to_string())?;
    if issues.is_empty() {
        return Err("Expected syntax errors from malformed Python snippet".into());
    }

    let has_actionable_line = issues.iter().any(|i| i.line >= 1 && !i.message.is_empty());
    if !has_actionable_line {
        return Err(format!("Expected line-numbered lint issues, got {:?}", issues));
    }

    Ok(())
}

async fn test_mcp_01(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    test_mcp_01_impl(&conn).await
}

async fn test_mem_01(db: &Database) -> Result<(), String> {
    let endpoint = "http://localhost:11434";
    let model = "all-minilm";

    let fact_a = "AetherForge is a secure local-first Mac agent runtime powered by MLX";
    let emb_a = match aether_core::fetch_ollama_embedding(endpoint, model, fact_a).await {
        Ok(e) => e,
        Err(e) => return Err(format!("Ollama embedder offline or failed: {}. (Rule: MEM-01 must fail if Ollama is down)", e)),
    };

    let fact_b = "Python is a popular programming language used for data science and web backend development";
    let emb_b = aether_core::fetch_ollama_embedding(endpoint, model, fact_b).await
        .map_err(|e| e.to_string())?;

    db.insert_memory_chunk("chk-fact-a", "memory://fact-a", fact_a, &emb_a)
        .map_err(|e| e.to_string())?;
    db.insert_memory_chunk("chk-fact-b", "memory://fact-b", fact_b, &emb_b)
        .map_err(|e| e.to_string())?;

    let query_text = "Tell me about AetherForge local Mac agent execution platform";
    let query_emb = aether_core::fetch_ollama_embedding(endpoint, model, query_text).await
        .map_err(|e| e.to_string())?;

    let results = db.search_semantic_memory(&query_emb, 3)
        .map_err(|e| e.to_string())?;

    if results.is_empty() {
        return Err("No semantic memory results returned".into());
    }

    let (top_chunk_id, _text, top_similarity) = &results[0];
    if top_chunk_id != "chk-fact-a" {
        return Err(format!("Negative control / precision failure: expected top-1 to be 'chk-fact-a', got '{}'", top_chunk_id));
    }

    if *top_similarity < 0.60 {
        return Err(format!("Similarity score too low for semantic paraphrase match: {}", top_similarity));
    }

    Ok(())
}

async fn test_skill_01() -> Result<(), String> {
    test_skill_01_impl().await
}

async fn test_safe_01(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    let session_id = "sess-safe-01";

    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'SAFE-01 Session', 'active')",
        rusqlite::params![session_id],
    ).map_err(|e| e.to_string())?;

    let decision_passwd = PermissionManager::check_file_access(&conn, session_id, "/etc/passwd", "read")
        .map_err(|e| e.to_string())?;
    if decision_passwd != PermissionDecision::Denied {
        return Err("Expected /etc/passwd denied without grant".into());
    }
    PermissionManager::audit_decision(&conn, session_id, "file_read", r#"{"path": "/etc/passwd"}"#, &decision_passwd, Some(1), Some(5))
        .map_err(|e| e.to_string())?;

    let decision_tmp = PermissionManager::check_file_access(&conn, session_id, "/tmp/secret.txt", "read")
        .map_err(|e| e.to_string())?;
    if decision_tmp != PermissionDecision::Denied {
        return Err("Expected /tmp/secret.txt denied without grant".into());
    }
    PermissionManager::audit_decision(&conn, session_id, "file_read", r#"{"path": "/tmp/secret.txt"}"#, &decision_tmp, Some(1), Some(3))
        .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'read')",
        rusqlite::params![session_id, "/tmp/secret.txt"],
    ).map_err(|e| e.to_string())?;

    let decision_granted = PermissionManager::check_file_access(&conn, session_id, "/tmp/secret.txt", "read")
        .map_err(|e| e.to_string())?;
    if decision_granted != PermissionDecision::Approved {
        return Err("Expected /tmp/secret.txt approved with grant".into());
    }
    PermissionManager::audit_decision(&conn, session_id, "file_read", r#"{"path": "/tmp/secret.txt"}"#, &decision_granted, Some(0), Some(4))
        .map_err(|e| e.to_string())?;

    let traversal = PermissionManager::check_file_access(&conn, session_id, "/tmp/../etc/passwd", "read")
        .map_err(|e| e.to_string())?;
    if traversal != PermissionDecision::Denied {
        return Err("Expected path traversal via .. to be denied".into());
    }

    let denied_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM audit_log WHERE decision = 'denied';",
        [],
        |row| row.get(0)
    ).map_err(|e| e.to_string())?;

    if denied_count < 2 {
        return Err("Expected at least 2 denied audit entries".into());
    }

    verify_audit_hash_chain(&conn)?;

    Ok(())
}

async fn test_rout_01() -> Result<(), String> {
    let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let fast_model = std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());
    let slow_model = std::env::var("AETHER_CHAT_MODEL_COMPLEX").unwrap_or_else(|_| fast_model.clone());

    aether_core::OllamaProvider::health_check(&endpoint).await.map_err(|e| {
        format!("Ollama offline or unreachable: {}. (Rule: ROUT-01 must fail if Ollama is down)", e)
    })?;

    let router = aether_core::ModelRouter::new(
        aether_core::ModelBackend::OllamaMlx {
            endpoint: endpoint.clone(),
            model: fast_model.clone(),
        },
        Some(aether_core::ModelBackend::OllamaMlx {
            endpoint,
            model: slow_model,
        }),
    );

    let warmup = router
        .complete("Reply with exactly: ok", aether_core::PromptComplexity::Simple)
        .await
        .map_err(|e| format!("Warmup completion failed: {}", e))?;

    if warmup.content.is_empty() {
        return Err("Warmup returned empty content".into());
    }

    let timed = router
        .complete("Reply with one short word: forge", aether_core::PromptComplexity::Simple)
        .await
        .map_err(|e| format!("Timed completion failed: {}", e))?;

    if timed.content.is_empty() {
        return Err("Completion returned empty content".into());
    }

    const TTFT_WARM_MS: u128 = 2000;
    if timed.ttft_ms > TTFT_WARM_MS {
        return Err(format!(
            "TTFT {}ms exceeds warm threshold {}ms (cold model load may require pulling {})",
            timed.ttft_ms, TTFT_WARM_MS, fast_model
        ));
    }

    Ok(())
}

async fn test_res_01() -> Result<(), String> {
    let tmp = tempdir().map_err(|e| e.to_string())?;
    let db_path = tmp.path().join("aether_recovery.db");

    CrashRecoveryTest::simulate_sigterm_recovery(&db_path)?;
    Ok(())
}
