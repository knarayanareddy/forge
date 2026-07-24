use aether_db::Database;
use aether_skills::{register_skill, record_skill_success, SkillExecutor, SkillLoader};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

pub async fn test_skill_01_impl() -> Result<(), String> {
    let skills_root = Path::new("skills");
    if !skills_root.is_dir() {
        return Err("skills/ directory missing at repo root".into());
    }

    let skills = SkillLoader::load_directory(skills_root).map_err(|e| e.to_string())?;
    let changelog = skills
        .iter()
        .find(|s| s.name == "changelog")
        .ok_or_else(|| "Expected changelog skill in skills/".to_string())?;

    if changelog.steps.len() < 2 {
        return Err(format!(
            "Expected at least 2 procedural steps, got {}",
            changelog.steps.len()
        ));
    }

    let db = Database::open_in_memory().map_err(|e| e.to_string())?;
    let conn = db.conn();

    register_skill(&conn, changelog).map_err(|e| e.to_string())?;

    let session_id = "sess-skill-01";
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'SKILL-01 Session', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path();
    let workspace_str = workspace.to_string_lossy().to_string();
    let changelog_path = workspace.join("CHANGELOG.md");
    fs::write(&changelog_path, "# Changelog\n\n").map_err(|e| e.to_string())?;
    let changelog_str = changelog_path.to_string_lossy().to_string();

    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace_str],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'read')",
        rusqlite::params![session_id, changelog_str],
    )
    .map_err(|e| e.to_string())?;

    let mut variables = HashMap::new();
    variables.insert("date".to_string(), "2026-07-24".to_string());
    variables.insert(
        "entry".to_string(),
        "SKILL-01 harness verification".to_string(),
    );

    SkillExecutor::execute(&conn, session_id, changelog, workspace, &variables)
        .map_err(|e| e.to_string())?;

    record_skill_success(&conn, &changelog.id).map_err(|e| e.to_string())?;

    let content = fs::read_to_string(&changelog_path).map_err(|e| e.to_string())?;
    if !content.contains("SKILL-01 harness verification") {
        return Err(format!(
            "CHANGELOG.md missing expected entry; got: {}",
            content
        ));
    }

    let success_count: i64 = conn
        .query_row(
            "SELECT success_count FROM procedural_skills WHERE id = ?1",
            rusqlite::params![changelog.id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    if success_count != 1 {
        return Err(format!(
            "Expected success_count=1 after execution, got {}",
            success_count
        ));
    }

    Ok(())
}
