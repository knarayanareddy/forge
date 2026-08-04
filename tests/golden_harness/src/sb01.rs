//! SB-01 — production tool sandbox boundary (Phase 8.0c).

use aether_core::{LoopConfig, ReActLoopEngine, ToolInvocation};
use aether_db::Database;
use aether_sandbox::ProductionSandbox;
use std::collections::HashMap;

pub fn test_sb01_impl(db: &Database) -> Result<(), String> {
    if std::env::consts::OS != "macos" {
        return Err("SB-01 requires Darwin Seatbelt (fail-closed off Darwin)".into());
    }

    let workspace = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace_path = workspace.path().to_path_buf();
    let session_id = "sess-sb-01";
    {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES (?1, 'SB-01', 'active')",
            rusqlite::params![session_id],
        )
        .map_err(|e| e.to_string())?;
        let path = workspace_path.to_string_lossy().to_string();
        for capability in ["read", "write"] {
            conn.execute(
                "INSERT INTO capability_grants (session_id, resource_path, permission_type)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![session_id, path, capability],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    // Exercise the real ToolRegistry path: file write/read/verify, lint child, and git children.
    let plan = vec![
        ToolInvocation::FsWrite {
            path: "sb01.py".into(),
            content: "def sandboxed():\n    return 'SB-01'\n".into(),
        },
        ToolInvocation::FsRead {
            path: "sb01.py".into(),
        },
        ToolInvocation::VerifyContains {
            path: "sb01.py".into(),
            text: "SB-01".into(),
        },
        ToolInvocation::PythonLint {
            source: "def sandboxed():\n    return 'SB-01'\n".into(),
        },
        ToolInvocation::GitInit {
            branch: "sb01-main".into(),
        },
        ToolInvocation::Done,
    ];
    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: 16_384,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_id.into(),
        workspace: workspace_path.clone(),
    };
    let result = {
        let conn = db.conn();
        ReActLoopEngine::new(8)
            .run_structured(
                &conn,
                &mut config,
                plan,
                None,
                &HashMap::new(),
                |_| {},
            )
            .map_err(|e| e.to_string())?
    };
    if !result.done || result.observations.iter().any(|observation| !observation.success) {
        return Err(format!(
            "production sandbox loop did not complete cleanly: {:?}",
            result.observations
        ));
    }

    // Parent secrets must never enter a tool child.
    std::env::set_var("AETHER_SB01_SECRET", "must-not-reach-child");
    let env_result = ProductionSandbox::command(
            "/usr/bin/env",
            std::iter::empty::<&str>(),
            &workspace_path,
        )
        .map_err(|e| e.to_string())
        .and_then(|mut command| command.output().map_err(|e| e.to_string()));
    std::env::remove_var("AETHER_SB01_SECRET");
    let env_output = env_result?;
    let child_env = String::from_utf8_lossy(&env_output.stdout);
    if child_env.contains("must-not-reach-child") || child_env.contains("AETHER_SB01_SECRET") {
        return Err("sandbox child inherited parent secret environment".into());
    }

    // Seatbelt profile denies all network. A connection failure is the required outcome.
    let network = ProductionSandbox::command(
        "/usr/bin/curl",
        ["--connect-timeout", "2", "https://example.com"],
        &workspace_path,
    )
    .map_err(|e| e.to_string())?
    .output()
    .map_err(|e| e.to_string())?;
    if network.status.success() {
        return Err("sandboxed tool escaped network deny policy".into());
    }

    if ProductionSandbox::read_to_string(&workspace_path, std::path::Path::new("/etc/passwd"))
        .is_ok()
    {
        return Err("sandboxed production read escaped workspace".into());
    }

    Ok(())
}
