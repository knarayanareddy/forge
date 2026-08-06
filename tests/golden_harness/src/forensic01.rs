//! FORENSIC-01 — failure trajectory classification + regression export (Phase 12).
use aether_core::LoopConfig;
use aether_daemon::forensics::{classification_accuracy, export_regression_case, load_forensic_trajectories, FailureClass, FORENSIC01_MIN_ACCURACY};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;
use std::path::Path;
use tempfile::tempdir;
pub fn forensic01_fixture_ready() -> Result<usize, String> {
    let path = [Path::new("tests/golden_harness/fixtures/forensic01_trajectories.json"), Path::new("fixtures/forensic01_trajectories.json")].into_iter().find(|p| p.exists()).ok_or("fixture missing")?;
    Ok(load_forensic_trajectories(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?.len())
}
pub fn test_forensic01_impl(db: &Database) -> Result<(), String> {
    let path = Path::new("tests/golden_harness/fixtures/forensic01_trajectories.json");
    let trajectories = load_forensic_trajectories(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?;
    if classification_accuracy(&trajectories) < FORENSIC01_MIN_ACCURACY { return Err("accuracy below threshold".into()); }
    let t = trajectories.iter().find(|x| x.id == "fg-missing-grant-01").ok_or("missing case")?;
    let regression = export_regression_case(t);
    if regression.failure_class != FailureClass::MissingGrant { return Err("wrong class".into()); }
    let tmp = tempdir().map_err(|e| e.to_string())?; let workspace = tmp.path(); let session_id = "sess-forensic-01-replay";
    let conn = db.conn();
    conn.execute("INSERT INTO sessions (id, title, status) VALUES (?1, 'FORENSIC-01', 'active')", rusqlite::params![session_id]).map_err(|e| e.to_string())?;
    conn.execute("INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')", rusqlite::params![session_id, workspace.to_string_lossy().to_string()]).map_err(|e| e.to_string())?;
    let mut config = LoopConfig::new(4, session_id.to_string(), workspace.to_path_buf());
    let (result, _) = execute_structured_loop(&conn, &mut config, regression.plan, None, &HashMap::new(), None, &regression.goal);
    result.map_err(|e| format!("replay failed: {e}"))?; Ok(())
}
