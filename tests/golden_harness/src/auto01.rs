use aether_core::ReActLoopEngine;
use aether_daemon::automation::{AutomationScheduler, AutomationTrigger, TriggerConfig, TriggerType};
use aether_daemon::task_runner::run_automation_trigger;
use aether_permissions::AutomationGrant;
use std::fs;
use tempfile::tempdir;

#[derive(serde::Deserialize)]
struct Auto01Fixture {
    trigger_id: String,
    session_id: String,
    trigger_type: String,
    config: TriggerConfig,
    loop_plan: Vec<serde_json::Value>,
}

pub fn auto01_fixture_ready() -> Result<(), String> {
    let raw = include_str!("../fixtures/auto01_trigger.json");
    let fixture: Auto01Fixture = serde_json::from_str(raw)
        .map_err(|e| format!("AUTO-01 fixture parse failed: {}", e))?;
    if fixture.trigger_id.is_empty() || fixture.loop_plan.is_empty() {
        return Err("AUTO-01 fixture missing trigger_id or loop_plan".into());
    }
    Ok(())
}

pub async fn test_auto01_impl(conn: &rusqlite::Connection) -> Result<(), String> {
    let raw = include_str!("../fixtures/auto01_trigger.json");
    let fixture: Auto01Fixture = serde_json::from_str(raw)
        .map_err(|e| format!("fixture parse: {}", e))?;

    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'AUTO-01', 'active')",
        rusqlite::params![fixture.session_id],
    )
    .map_err(|e| e.to_string())?;

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let workspace_str = workspace.to_string_lossy().to_string();
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type)
         VALUES (?1, ?2, 'write')",
        rusqlite::params![fixture.session_id, &workspace_str],
    )
    .map_err(|e| e.to_string())?;

    let task_prompt = serde_json::json!({ "loop": fixture.loop_plan }).to_string();

    let trigger_type = match fixture.trigger_type.as_str() {
        "cron" => TriggerType::Cron,
        "file_watch" => TriggerType::FileWatch,
        "pr_webhook" => TriggerType::PrWebhook,
        other => return Err(format!("unknown trigger_type in fixture: {}", other)),
    };

    let trigger = AutomationTrigger {
        trigger_id: fixture.trigger_id.clone(),
        trigger_type,
        session_id: fixture.session_id.clone(),
        config: fixture.config,
        task_prompt: task_prompt.clone(),
        workspace_path: Some(workspace_str.clone()),
        enabled: true,
        last_fired_at: None,
    };

    AutomationScheduler::register_trigger(conn, &trigger).map_err(|e| e.to_string())?;

    // Forbidden: trigger without AutomationGrant must not enqueue/run.
    let mut scheduler = AutomationScheduler::new();
    let denied = scheduler
        .trigger_automation_run(conn, &trigger, "cron")
        .map_err(|e| e.to_string())?;
    if !matches!(denied, aether_daemon::automation::AutomationOutcome::Denied { .. }) {
        return Err("Expected automation fire without grant to be denied".into());
    }

    let denied_audit: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log
             WHERE tool_name = 'automation_run' AND decision = 'denied'
               AND arguments_json LIKE ?1",
            rusqlite::params![format!("%{}%", fixture.trigger_id)],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if denied_audit < 1 {
        return Err("Expected denied automation_run audit entry".into());
    }

    AutomationGrant::grant(conn, &fixture.trigger_id, &fixture.session_id)
        .map_err(|e| e.to_string())?;

    let enqueued = scheduler
        .trigger_automation_run(conn, &trigger, "cron")
        .map_err(|e| e.to_string())?;
    if !matches!(
        enqueued,
        aether_daemon::automation::AutomationOutcome::Enqueued { .. }
    ) {
        return Err("Expected granted automation to enqueue".into());
    }

    let results = AutomationScheduler::run_pending(conn, 1, |t| {
        run_automation_trigger(conn, t)
    })
    .map_err(|e| e.to_string())?;

    if results.len() != 1 || results[0].1.is_err() {
        return Err(format!(
            "automation run_task failed: {:?}",
            results.first().map(|(_, r)| r)
        ));
    }

    let marker = workspace.join("auto_marker.txt");
    if !marker.exists() {
        return Err("AUTO-01 marker file not created by loop plan".into());
    }
    let content = fs::read_to_string(&marker).map_err(|e| e.to_string())?;
    if !content.contains("AUTO-01-verified") {
        return Err(format!("Unexpected marker content: {}", content));
    }

    let audit_with_trigger: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log
             WHERE tool_name = 'automation_run' AND decision = 'approved'
               AND arguments_json LIKE ?1",
            rusqlite::params![format!("%{}%", fixture.trigger_id)],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if audit_with_trigger < 2 {
        return Err(format!(
            "Expected audit_log entries with trigger_id {}; found {}",
            fixture.trigger_id, audit_with_trigger
        ));
    }

    let completed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM automation_queue
             WHERE trigger_id = ?1 AND status = 'completed'",
            rusqlite::params![fixture.trigger_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if completed != 1 {
        return Err(format!(
            "Expected 1 completed queue entry, got {}",
            completed
        ));
    }

    // Sanity: structured plan parser accepts frozen fixture shape.
    if ReActLoopEngine::parse_plan_from_prompt(&task_prompt).is_none() {
        return Err("Frozen AUTO-01 plan must parse via ReActLoopEngine".into());
    }

    Ok(())
}
