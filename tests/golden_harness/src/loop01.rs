use aether_core::{LoopConfig, ReActLoopEngine, ToolInvocation};
use std::collections::HashMap;
use std::fs;
use tempfile::tempdir;

pub async fn test_loop_01_impl(conn: &rusqlite::Connection) -> Result<(), String> {
    let session_id = "sess-loop-01";
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'LOOP-01 Session', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let workspace_str = workspace.to_string_lossy().to_string();

    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace_str],
    )
    .map_err(|e| e.to_string())?;

    let plan = vec![
        ToolInvocation::FsWrite {
            path: "loop_marker.txt".into(),
            content: "LOOP-01-verified".into(),
        },
        ToolInvocation::VerifyContains {
            path: "loop_marker.txt".into(),
            text: "LOOP-01-verified".into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::Done,
    ];

    let mut config = LoopConfig {
        max_iterations: 6,
        max_tokens: 0,
        tokens_used: 0,
        session_id: session_id.into(),
        workspace: workspace.clone(),
    };

    let engine = ReActLoopEngine::new(6);
    let mut plan_events = 0usize;
    let mut tool_events = 0usize;
    let mut verify_events = 0usize;

    let result = engine
        .run_structured(
            conn,
            &mut config,
            plan,
            None,
            &HashMap::new(),
            |event| match event {
                aether_core::LoopStreamEvent::Plan { .. } => plan_events += 1,
                aether_core::LoopStreamEvent::Tool { .. } => tool_events += 1,
                aether_core::LoopStreamEvent::Verify { .. } => verify_events += 1,
                _ => {}
            },
        )
        .map_err(|e| e.to_string())?;

    if !result.done {
        return Err("Expected loop to finish with done=true".into());
    }

    if result.iterations < 3 {
        return Err(format!(
            "Expected at least 3 iterations, got {}",
            result.iterations
        ));
    }

    if plan_events < 3 || tool_events < 3 {
        return Err(format!(
            "Expected plan/tool events; plan={} tool={}",
            plan_events, tool_events
        ));
    }

    if verify_events < 2 {
        return Err(format!(
            "Expected verify events for contains+lint, got {}",
            verify_events
        ));
    }

    let marker = workspace.join("loop_marker.txt");
    if !marker.exists() {
        return Err("loop_marker.txt not created".into());
    }

    let content = fs::read_to_string(&marker).map_err(|e| e.to_string())?;
    if !content.contains("LOOP-01-verified") {
        return Err(format!("Unexpected marker content: {}", content));
    }

    Ok(())
}
