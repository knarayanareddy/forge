use aether_db::Database;
use aether_permissions::{PermissionManager, PermissionDecision, FileMutator};
use futures::StreamExt;
use std::fs;
use tempfile::tempdir;

mod budg01;

mod cost01;
use cost01::test_cost01_impl;
use budg01::test_budg01_impl;

mod graph02;
use graph02::test_graph02_impl;

mod graph01;
use graph01::test_graph_01_impl;

mod loop01;
use loop01::test_loop_01_impl;

mod loop02;
use loop02::test_loop_02_impl;

mod plan01;
use plan01::test_plan01_impl;

mod sess01;
use sess01::test_sess01_impl;

mod undo01;
use undo01::test_undo01_impl;

mod loop04;
use loop04::test_loop04_impl;

mod hook01;
use hook01::test_hook01_impl;

mod ckpt01;
use ckpt01::test_ckpt01_impl;

mod cons01;
use cons01::test_cons01_impl;

mod perm02;
use perm02::test_perm02_impl;

mod sub01;
use sub01::test_sub01_impl;

mod sec01;
use sec01::test_sec01_impl;

mod auto01;
use auto01::test_auto01_impl;

mod check01;
use check01::test_check01_impl;

mod gate01;
use gate01::test_gate01_impl;

mod gate02;
use gate02::test_gate02_impl;

mod recovery;
use recovery::CrashRecoveryTest;

mod fs02;
use fs02::test_fs_02_impl;

mod sb01;
use sb01::test_sb01_impl;

mod mcp01;
use mcp01::test_mcp_01_impl;

mod mem02;
use mem02::test_mem02_impl;

mod skill01;
use skill01::test_skill_01_impl;

mod audit_chain;
use audit_chain::verify_audit_hash_chain;

mod red01;
use red01::test_red01_impl;
mod skill02;
use skill02::test_skill02_impl;

mod skill03;
use skill03::test_skill03_impl;

mod inject01;
use inject01::test_inject01_impl;

mod ingest01;
use ingest01::test_ingest01_impl;

mod reg01;
use reg01::test_reg01_impl;

struct TaskSpec {
    name: &'static str,
    hard_on_darwin: bool,
    fail_closed_off_darwin: bool,
}

const TASKS: [TaskSpec; 38] = [
    // ROUT-01 first: measure warm TTFT before FS-02 sandbox load and MCP/MEM embedder swap.
    TaskSpec { name: "ROUT-01", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "FS-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "FS-02", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "SB-01", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "GIT-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "CODE-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "MCP-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "MEM-01", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "MEM-02", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "GRAPH-01", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "SKILL-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "SKILL-02", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "SAFE-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "RED-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "RES-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "LOOP-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "LOOP-02", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "PLAN-01", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "LOOP-04", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "SESS-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "UNDO-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "AUTO-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "CHECK-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "GATE-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "GATE-02", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "HOOK-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "CKPT-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "CONS-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "PERM-02", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "SUB-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "SEC-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "SKILL-03", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "INJECT-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "INGEST-01", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "BUDG-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "COST-01", hard_on_darwin: true, fail_closed_off_darwin: false },
    TaskSpec { name: "GRAPH-02", hard_on_darwin: true, fail_closed_off_darwin: true },
    TaskSpec { name: "REG-01", hard_on_darwin: false, fail_closed_off_darwin: false },
];

fn is_darwin() -> bool {
    std::env::consts::OS == "macos"
}

#[tokio::main]
async fn main() {
    println!("=== AetherForge Golden Task Evaluation Harness ===");
    println!("Constitution: v1.2.4 + Phase 3 Agent Loop");
    println!("Platform: {} (Darwin canonical)\n", std::env::consts::OS);

    match red01::load_red_team_fixtures() {
        Ok(f) => println!("RED-01 fixtures: {} frozen adversarial cases loaded", f.cases.len()),
        Err(e) => eprintln!("Warning: RED-01 fixture check failed: {}", e),
    }

    match skill02::skill02_fixture_ready() {
        Ok(n) => println!("SKILL-02 fixtures: {} frozen eval questions loaded\n", n),
        Err(e) => eprintln!("Warning: SKILL-02 fixture check failed: {}\n", e),
    }

    match skill03::skill03_fixture_ready() {
        Ok(n) => println!("SKILL-03 fixtures: {} frozen poisoned-skill cases loaded", n),
        Err(e) => eprintln!("Warning: SKILL-03 fixture check failed: {}", e),
    }

    match inject01::inject01_fixture_ready() {
        Ok(n) => println!("INJECT-01 fixtures: {} frozen tool-result induction cases loaded", n),
        Err(e) => eprintln!("Warning: INJECT-01 fixture check failed: {}", e),
    }

    match ingest01::ingest01_fixture_ready() {
        Ok(n) => println!("INGEST-01 fixtures: {} expected live-extract entities loaded", n),
        Err(e) => eprintln!("Warning: INGEST-01 fixture check failed: {}", e),
    }

    match budg01::budg01_fixture_ready() {
        Ok(()) => println!("BUDG-01 fixtures: default token cap configured"),
        Err(e) => eprintln!("Warning: BUDG-01 fixture check failed: {}", e),
    }
    match cost01::cost01_fixture_ready() {
        Ok(()) => println!("COST-01 fixtures: provider token usage parser ready"),
        Err(e) => eprintln!("Warning: COST-01 fixture check failed: {}", e),
    }

    match reg01::reg01_fixture_ready() {
        Ok(()) => println!("REG-01 fixtures: models/registry.toml loaded"),
        Err(e) => eprintln!("Warning: REG-01 fixture check failed: {}", e),
    }

    match graph02::graph02_fixture_ready() {
        Ok(n) => println!("GRAPH-02 fixtures: {} gold queries loaded", n),
        Err(e) => eprintln!("Warning: GRAPH-02 fixture check failed: {}", e),
    }

    match check01::check01_fixture_ready() {
        Ok(n) => println!("CHECK-01 fixtures: {} frozen bad plans loaded", n),
        Err(e) => eprintln!("Warning: CHECK-01 fixture check failed: {}", e),
    }

    match auto01::auto01_fixture_ready() {
        Ok(()) => println!("AUTO-01 fixtures: frozen cron trigger loaded"),
        Err(e) => eprintln!("Warning: AUTO-01 fixture check failed: {}", e),
    }

    match gate01::gate01_fixture_ready() {
        Ok(()) => println!("GATE-01 fixtures: frozen Slack channel loaded"),
        Err(e) => eprintln!("Warning: GATE-01 fixture check failed: {}", e),
    }

    match gate02::gate02_fixture_ready() {
        Ok(()) => println!("GATE-02 fixtures: frozen Telegram channel loaded"),
        Err(e) => eprintln!("Warning: GATE-02 fixture check failed: {}", e),
    }

    match graph01::graph01_fixture_ready() {
        Ok(n) => println!("GRAPH-01 fixtures: {} gold queries loaded", n),
        Err(e) => eprintln!("Warning: GRAPH-01 fixture check failed: {}", e),
    }

    match plan01::plan01_fixture_ready() {
        Ok(n) => println!("PLAN-01 fixtures: {} diverse goals loaded", n),
        Err(e) => eprintln!("Warning: PLAN-01 fixture check failed: {}", e),
    }

    let db = Database::open_in_memory().expect("In-memory DB init failed");

    if is_darwin() {
        let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let chat_model =
            std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());
        match aether_core::OllamaProvider::health_check(&endpoint).await {
            Ok(()) => match aether_core::OllamaProvider::preload_chat_model(&endpoint, &chat_model).await
            {
                Ok(()) => println!(
                    "Ollama chat model pre-warmed for ROUT-01 ({} @ {})\n",
                    chat_model, endpoint
                ),
                Err(e) => eprintln!(
                    "Note: Ollama chat pre-warm failed (ROUT-01 may be cold): {}\n",
                    e
                ),
            },
            Err(e) => eprintln!(
                "Note: Ollama unreachable for pre-warm (ROUT-01 may fail): {}\n",
                e
            ),
        }
    }

    let task_filter = std::env::var("AETHER_HARNESS_TASK").ok();
    let mut passed = 0u32;
    let mut hard_pass = 0u32;
    let mut soft_pass = 0u32;
    let tasks: Vec<&TaskSpec> = TASKS
        .iter()
        .filter(|spec| {
            task_filter
                .as_deref()
                .map(|want| spec.name == want)
                .unwrap_or(true)
        })
        .collect();
    let total = tasks.len() as u32;

    for spec in tasks {
        let result = run_named_task(spec.name, &db).await;
        match result {
            Ok(hard) => {
                passed += 1;
                if hard {
                    hard_pass += 1;
                } else {
                    soft_pass += 1;
                }
            }
            Err(e) => {
            if !is_darwin() && spec.fail_closed_off_darwin {
                println!("FAIL-CLOSED ({})", e);
            } else {
                println!("FAIL ({})", e);
            }
        }
        }
    }

    println!("\n=== Evaluation Results ===");
    println!("Passed: {} / {}", passed, total);
    println!(
        "Hard green: {} / {} | Soft green: {} / {}",
        hard_pass,
        total,
        soft_pass,
        total
    );
    if is_darwin() {
        println!("Darwin scoreboard: {}/{} harness ({} hard / {} soft)", passed, total, hard_pass, soft_pass);
    } else {
        println!(
            "Non-Darwin note: FS-02, SB-01, MEM-01, ROUT-01, GRAPH-01, LOOP-02, PLAN-01, LOOP-04, INGEST-01 expected fail-closed when sandbox-exec/Ollama absent; SEC-01 is Ollama-independent"
        );
    }
}

async fn ensure_ollama_embed_ready() -> Result<(), String> {
    let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let model = std::env::var("AETHER_EMBED_MODEL").unwrap_or_else(|_| "all-minilm".to_string());
    aether_core::OllamaProvider::health_check(&endpoint)
        .await
        .map_err(|e| format!("Ollama unreachable before embed task: {}", e))?;
    aether_core::OllamaProvider::warm_embed_model(&endpoint, &model)
        .await
        .map_err(|e| format!("Ollama embedder warmup failed: {}", e))
}

async fn run_named_task(name: &str, db: &Database) -> Result<bool, String> {
    print!("[-] Running task [{}] ... ", name);
    let outcome = match name {
        "FS-01" => test_fs_01(db).await.map(|_| true),
        "FS-02" => test_fs_02().await.map(|_| true),
        "SB-01" => test_sb01_impl(db).map(|_| true),
        "GIT-01" => test_git_01(db).await.map(|_| true),
        "CODE-01" => test_code_01().await.map(|_| true),
        "MCP-01" => test_mcp_01(db).await.map(|_| true),
        "MEM-01" => {
            if is_darwin() {
                ensure_ollama_embed_ready().await?;
            }
            test_mem_01(db).await.map(|_| true)
        }
        "MEM-02" => test_mem02_impl(db).map(|_| true),
        "GRAPH-01" => {
            if is_darwin() {
                ensure_ollama_embed_ready().await?;
            }
            test_graph_01(db).await.map(|_| true)
        }
        "SKILL-01" => test_skill_01().await.map(|_| true),
        "SKILL-02" => test_skill_02().await.map(|_| true),
        "SAFE-01" => test_safe_01(db).await.map(|_| true),
        "RED-01" => test_red_01(db).await.map(|_| true),
        "ROUT-01" => test_rout_01().await.map(|_| true),
        "RES-01" => test_res_01().await.map(|_| true),
        "LOOP-01" => test_loop_01(db).await.map(|_| true),
        "LOOP-02" => test_loop_02(db).await.map(|_| true),
        "PLAN-01" => test_plan01().await.map(|_| true),
        "LOOP-04" => test_loop04_impl(db).await.map(|_| true),
        "SESS-01" => test_sess01_impl(db).map(|_| true),
        "UNDO-01" => test_undo01_impl(db).map(|_| true),
        "AUTO-01" => test_auto01(db).await.map(|_| true),
        "CHECK-01" => test_check01(db).await.map(|_| true),
        "GATE-01" => test_gate01(db).await.map(|_| true),
        "GATE-02" => test_gate02(db).await.map(|_| true),
        "HOOK-01" => test_hook01_impl(db).map(|_| true),
        "CKPT-01" => test_ckpt01_impl(db).map(|_| true),
        "CONS-01" => test_cons01_impl(db).map(|_| true),
        "PERM-02" => test_perm02_impl(db).map(|_| true),
        "SUB-01" => test_sub01_impl(db).map(|_| true),
        "SEC-01" => test_sec01_impl(db).map(|_| true),
        "SKILL-03" => test_skill03_impl().map(|_| true),
        "INJECT-01" => test_inject01_impl().map(|_| true),
        "INGEST-01" => test_ingest01_impl(db).await.map(|_| true),
        "BUDG-01" => test_budg01_impl().map(|_| true),
        "COST-01" => test_cost01_impl().await.map(|hard| hard),
        "GRAPH-02" => {
            if is_darwin() {
                ensure_ollama_embed_ready().await?;
            }
            test_graph02_impl(db).await.map(|_| true)
        }
        "REG-01" => test_reg01_impl().await.map(|_| false),
        other => Err(format!("Unknown task {}", other)),
    };

    match outcome {
        Ok(hard) => {
            println!("PASS{}", if hard { " [hard]" } else { " [soft]" });
            Ok(hard)
        }
        Err(e) => Err(e),
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

    let git_denied = aether_core::GitOps::init_commit_and_branch(
        &conn,
        session_id,
        workspace,
        "feature/git-01",
    );
    if git_denied.is_ok() {
        return Err("GitOps must fail without write grant (hard GIT-01)".into());
    }

    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace_str],
    ).map_err(|e| e.to_string())?;

    aether_core::GitOps::init_commit_and_branch(
        &conn,
        session_id,
        workspace,
        "feature/git-01",
    )
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

    let results = db.search_semantic_memory_hybrid(query_text, &query_emb, 3)
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

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path();
    let workspace_str = workspace.to_string_lossy().to_string();
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'read')",
        rusqlite::params![session_id, workspace_str],
    ).map_err(|e| e.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let escape_link = workspace.join("escape-link");
        symlink("/etc/passwd", &escape_link).map_err(|e| e.to_string())?;
        let symlink_decision = PermissionManager::check_file_access(
            &conn,
            session_id,
            &escape_link.to_string_lossy(),
            "read",
        )
        .map_err(|e| e.to_string())?;
        if symlink_decision != PermissionDecision::Denied {
            return Err("Expected symlink escape outside granted workspace to be denied".into());
        }
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

async fn rout_01_measure_ttft(
    router: &aether_core::ModelRouter,
    endpoint: &str,
    model: &str,
) -> Result<(u128, u128), String> {
    const MAX_COLD_RETRIES: usize = 3;

    for attempt in 0..MAX_COLD_RETRIES {
        let mut stream = Box::pin(
            router
                .complete_stream(aether_core::ROUT_TTFT_PROMPT, aether_core::PromptComplexity::Simple)
                .await
                .map_err(|e| format!("Timed stream failed: {}", e))?,
        );

        let mut client_ttft_ms = None;
        let mut server_ttft_ms = None;
        let mut content = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Stream chunk error: {}", e))?;
            if client_ttft_ms.is_none() {
                client_ttft_ms = chunk.ttft_ms;
            }
            if let Some(server) = chunk.server_ttft_ms {
                server_ttft_ms = Some(server);
            }
            content.push_str(&chunk.text);
            if chunk.done {
                break;
            }
        }

        if content.trim().is_empty() {
            return Err("Streamed completion returned empty content".into());
        }

        let client = client_ttft_ms
            .ok_or_else(|| "No client TTFT recorded on first streamed token".to_string())?;

        if let Some(server) = server_ttft_ms {
            return Ok((client, server));
        }

        // Server timing missing on done chunk — retry once after rewarm if model looks cold.
        if attempt + 1 < MAX_COLD_RETRIES {
            let loaded = aether_core::OllamaProvider::is_model_loaded(endpoint, model)
                .await
                .unwrap_or(false);
            if !loaded {
                eprint!("[cold-load retry {}] ", attempt + 1);
                aether_core::OllamaProvider::warm_chat_model_with_prompt(
                    endpoint,
                    model,
                    aether_core::ROUT_TTFT_PROMPT,
                    2,
                )
                .await
                .map_err(|e| format!("Cold-load rewarm failed: {}", e))?;
                continue;
            }
        }

        return Ok((client, client));
    }

    Err("ROUT-01 TTFT measurement exhausted retries".into())
}

/// Drop the lowest/highest sample then take the median — dampens GHA runner spikes.
fn rout_01_trimmed_median(mut samples: Vec<u128>) -> u128 {
    if samples.len() >= 3 {
        samples.sort_unstable();
        let inner = &samples[1..samples.len() - 1];
        inner[inner.len() / 2]
    } else {
        samples.sort_unstable();
        samples[samples.len() / 2]
    }
}

async fn rout_01_discard_warmup(endpoint: &str, model: &str, rounds: usize) -> Result<(), String> {
    aether_core::OllamaProvider::warm_chat_model_with_prompt(
        endpoint,
        model,
        aether_core::ROUT_TTFT_PROMPT,
        rounds,
    )
    .await
    .map_err(|e| format!("Discard warmup failed: {}", e))
}

async fn test_red_01(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    test_red01_impl(&conn)
}

async fn test_skill_02() -> Result<(), String> {
    test_skill02_impl()
}

async fn test_rout_01() -> Result<(), String> {
    let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let fast_model = std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());
    let slow_model = std::env::var("AETHER_CHAT_MODEL_COMPLEX").unwrap_or_else(|_| fast_model.clone());

    aether_core::OllamaProvider::health_check(&endpoint).await.map_err(|e| {
        format!("Ollama offline or unreachable: {}. (Rule: ROUT-01 must fail if Ollama is down)", e)
    })?;

    aether_core::OllamaProvider::warm_chat_model(&endpoint, &fast_model, 7)
        .await
        .map_err(|e| format!("Chat model warmup failed: {}", e))?;

    if !aether_core::OllamaProvider::is_model_loaded(&endpoint, &fast_model)
        .await
        .map_err(|e| format!("Ollama ps check failed: {}", e))?
    {
        aether_core::OllamaProvider::warm_chat_model(&endpoint, &fast_model, 5)
            .await
            .map_err(|e| format!("Model not resident after warmup: {}", e))?;
    }

    let router = aether_core::ModelRouter::new(
        aether_core::ModelBackend::OllamaMlx {
            endpoint: endpoint.clone(),
            model: fast_model.clone(),
        },
        Some(aether_core::ModelBackend::OllamaMlx {
            endpoint: endpoint.clone(),
            model: slow_model,
        }),
    );

    // Discard streams so the first counted sample is not penalized by connection/prompt priming.
    rout_01_discard_warmup(&endpoint, &fast_model, 5).await?;

    // Default 200ms on local Darwin; CI sets AETHER_ROUT_TTFT_MS for macos-15 runner overhead.
    let ttft_warm_ms: u128 = std::env::var("AETHER_ROUT_TTFT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    const TTFT_SAMPLES: usize = 7;
    const MAX_ROUNDS: usize = 3;

    let mut last_server_samples = vec![0u128; TTFT_SAMPLES];
    let mut last_median = 0u128;

    for round in 0..MAX_ROUNDS {
        let mut server_samples = Vec::with_capacity(TTFT_SAMPLES);
        let mut client_samples = Vec::with_capacity(TTFT_SAMPLES);
        for i in 0..TTFT_SAMPLES {
            let (client, server) = rout_01_measure_ttft(&router, &endpoint, &fast_model).await?;
            server_samples.push(server);
            client_samples.push(client);
            eprint!(
                "timed run {}/{}={}ms (server {}ms) ",
                i + 1,
                TTFT_SAMPLES,
                client,
                server
            );
        }

        server_samples.sort_unstable();
        let median = rout_01_trimmed_median(server_samples.clone());
        last_server_samples = server_samples.clone();
        last_median = median;

        eprintln!(
            "round {} trimmed-median warm TTFT {}ms (threshold {}ms, keep_alive 30m, server {:?}, client {:?})",
            round + 1,
            median,
            ttft_warm_ms,
            server_samples,
            client_samples
        );

        if median <= ttft_warm_ms {
            return Ok(());
        }

        if round + 1 < MAX_ROUNDS {
            aether_core::OllamaProvider::warm_chat_model(&endpoint, &fast_model, 5)
                .await
                .map_err(|e| format!("Inter-round warmup failed: {}", e))?;
            rout_01_discard_warmup(&endpoint, &fast_model, 3).await?;
        }
    }

    Err(format!(
        "Median warm server TTFT {}ms exceeds threshold {}ms after {} rounds (server samples {:?}, model {})",
        last_median, ttft_warm_ms, MAX_ROUNDS, last_server_samples, fast_model
    ))
}

async fn test_loop_02(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    test_loop_02_impl(&conn).await
}

async fn test_plan01() -> Result<(), String> {
    test_plan01_impl().await
}

async fn test_check01(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    test_check01_impl(&conn).await
}

async fn test_gate01(db: &Database) -> Result<(), String> {
    test_gate01_impl(db).await
}

async fn test_auto01(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    test_auto01_impl(&conn).await
}

async fn test_loop_01(db: &Database) -> Result<(), String> {
    let conn = db.conn();
    test_loop_01_impl(&conn).await
}

async fn test_graph_01(db: &Database) -> Result<(), String> {
    test_graph_01_impl(db).await
}

async fn test_res_01() -> Result<(), String> {
    let tmp = tempdir().map_err(|e| e.to_string())?;
    let db_path = tmp.path().join("aether_recovery.db");

    CrashRecoveryTest::simulate_sigterm_recovery(&db_path)?;
    Ok(())
}

async fn test_gate02(db: &Database) -> Result<(), String> {
    test_gate02_impl(db).await
}
