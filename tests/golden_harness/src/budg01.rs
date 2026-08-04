//! BUDG-01 — daemon default non-zero max_tokens; frozen plan hard-stops on BudgetExceeded + audit.

use aether_core::{
    resolve_default_max_loop_tokens, LoopConfig, LoopError, ReActLoopEngine, ToolInvocation,
    DEFAULT_MAX_LOOP_TOKENS,
};
use aether_db::Database;
use std::collections::HashMap;

const BUDG01_HARNESS_CAP: usize = 12;

pub fn budg01_fixture_ready() -> Result<(), String> {
    if DEFAULT_MAX_LOOP_TOKENS == 0 {
        return Err("DEFAULT_MAX_LOOP_TOKENS must be > 0 for BUDG-01".into());
    }
    if resolve_default_max_loop_tokens() == 0 {
        return Err(
            "resolve_default_max_loop_tokens() must be > 0 unless AETHER_MAX_LOOP_TOKENS=0".into(),
        );
    }
    Ok(())
}

pub fn test_budg01_impl() -> Result<(), String> {
    budg01_fixture_ready()?;

    let db = Database::open_in_memory().map_err(|e| e.to_string())?;
    let conn = db.conn();
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES ('sess-budg01', 'BUDG-01', 'active')",
        [],
    )
    .map_err(|e| e.to_string())?;

    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let ws = workspace.to_string_lossy().to_string();
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type)
         VALUES ('sess-budg01', ?1, 'write')",
        rusqlite::params![ws],
    )
    .map_err(|e| e.to_string())?;

    let mut config = LoopConfig {
        max_iterations: 4,
        max_tokens: BUDG01_HARNESS_CAP,
        tokens_used: 0,
        session_id: "sess-budg01".into(),
        workspace,
    };

    let plan = vec![
        ToolInvocation::FsWrite {
            path: "budget_marker.txt".into(),
            content: "BUDG-01-budget-exceeded-marker-content".repeat(8),
        },
        ToolInvocation::Done,
    ];

    let engine = ReActLoopEngine::new(4);
    let err = engine
        .run_structured(&conn, &mut config, plan, None, &HashMap::new(), |_| {})
        .unwrap_err();

    match err {
        LoopError::BudgetExceeded { used, max } => {
            if max != BUDG01_HARNESS_CAP {
                return Err(format!("expected max {}, got {}", BUDG01_HARNESS_CAP, max));
            }
            if used <= max {
                return Err(format!("expected used > max, got {}/{}", used, max));
            }
        }
        other => return Err(format!("expected BudgetExceeded, got {:?}", other)),
    }

    let audit_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log
             WHERE tool_name = 'loop_budget' AND decision = 'denied'
               AND session_id = 'sess-budg01'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if audit_count < 1 {
        return Err("Expected loop_budget denied audit entry".into());
    }

    if resolve_default_max_loop_tokens() == 0 {
        return Err("daemon default max_tokens must be > 0".into());
    }

    Ok(())
}
