//! SKILL-03 — frozen poisoned-skill corpus, 0 escapes (Phase 11 slices 11.1–11.4).
//!
//! Exercises production `aether-skills` APIs (`install_skill`, `admit_skill`, `SkillExecutor`)
//! against a frozen corpus — not harness-only mocks. Every poisoned case must fail closed;
//! the benign control skill must still install and execute.

use aether_db::Database;
use aether_skills::{
    admit_skill, install_skill, SkillExecutor, SkillLoader, SkillPinStore,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Corpus {
    schema_version: u32,
    cases: Vec<CorpusCase>,
}

#[derive(Debug, Deserialize)]
struct CorpusCase {
    id: String,
    kind: String,
    skill_dir: String,
    #[serde(default)]
    mutated_skill_dir: Option<String>,
    #[serde(default)]
    expected_reject_contains: Option<String>,
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/skills/poisoned")
}

fn load_corpus() -> Result<Corpus, String> {
    let path = corpus_root().join("corpus.json");
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let corpus: Corpus = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if corpus.schema_version != 1 {
        return Err(format!(
            "unsupported SKILL-03 corpus schema_version {}",
            corpus.schema_version
        ));
    }
    Ok(corpus)
}

fn load_skill_dir(dir_name: &str) -> Result<aether_skills::SkillDefinition, String> {
    let path = corpus_root().join(dir_name).join("SKILL.md");
    if !path.is_file() {
        return Err(format!("missing skill fixture: {}", path.display()));
    }
    SkillLoader::load_from_file(&path).map_err(|e| e.to_string())
}

fn assert_rejects(err: &str, needle: &str, case_id: &str) -> Result<(), String> {
    if !err.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()) {
        return Err(format!(
            "SKILL-03 case {case_id}: expected reject containing {needle:?}, got: {err}"
        ));
    }
    Ok(())
}

fn seed_session_with_full_grants(
    db: &Database,
    session_id: &str,
    workspace: &Path,
) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'SKILL-03', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    let ws = workspace.to_string_lossy().to_string();
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type)
         VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, ws],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type)
         VALUES (?1, ?2, 'read')",
        rusqlite::params![session_id, ws],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn skill03_fixture_ready() -> Result<usize, String> {
    let corpus = load_corpus()?;
    let poisoned = corpus
        .cases
        .iter()
        .filter(|c| c.kind != "install_and_execute")
        .count();
    if poisoned < 8 {
        return Err(format!(
            "SKILL-03 corpus needs ≥8 poisoned cases, found {poisoned}"
        ));
    }
    Ok(poisoned)
}

pub fn test_skill03_impl() -> Result<(), String> {
    let corpus = load_corpus()?;
    let poisoned = skill03_fixture_ready()?;

    let mut escapes = Vec::new();

    for case in &corpus.cases {
        match case.kind.as_str() {
            "install_reject" => {
                let skill = load_skill_dir(&case.skill_dir)?;
                let mut pins = SkillPinStore::new();
                let needle = case
                    .expected_reject_contains
                    .as_deref()
                    .ok_or_else(|| format!("{} missing expected_reject_contains", case.id))?;

                match install_skill(&mut pins, &skill) {
                    Ok(_) => escapes.push(format!(
                        "{}: install_skill succeeded (escape)",
                        case.id
                    )),
                    Err(e) => {
                        if let Err(check) = assert_rejects(&e.to_string(), needle, &case.id) {
                            escapes.push(check);
                        }
                    }
                }

                // Even with a full workspace grant, execute must not succeed for a poisoned skill.
                let db = Database::open_in_memory().map_err(|e| e.to_string())?;
                let session_id = format!("sess-skill03-{}", case.id);
                let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
                seed_session_with_full_grants(&db, &session_id, tmp.path())?;
                let conn = db.conn();
                if SkillExecutor::execute(
                    &conn,
                    &session_id,
                    &skill,
                    tmp.path(),
                    &HashMap::new(),
                )
                .is_ok()
                {
                    escapes.push(format!(
                        "{}: SkillExecutor::execute succeeded on poisoned skill (escape)",
                        case.id
                    ));
                }
            }
            "rug_pull" => {
                let base = load_skill_dir(&case.skill_dir)?;
                let mutated_dir = case
                    .mutated_skill_dir
                    .as_deref()
                    .ok_or_else(|| format!("{} missing mutated_skill_dir", case.id))?;
                // Parse the mutated body against the *base* path so the skill id (directory
                // name) stays stable — a real rug-pull updates SKILL.md in place, it does not
                // rename the skill directory.
                let mutated_path = corpus_root().join(mutated_dir).join("SKILL.md");
                let mutated_text =
                    fs::read_to_string(&mutated_path).map_err(|e| e.to_string())?;
                let base_path = corpus_root().join(&case.skill_dir).join("SKILL.md");
                let mutated = SkillLoader::parse(&mutated_text, &base_path)
                    .map_err(|e| e.to_string())?;
                if mutated.id != base.id {
                    return Err(format!(
                        "{}: rug-pull fixture id mismatch: base={}, mutated={}",
                        case.id, base.id, mutated.id
                    ));
                }
                let mut pins = SkillPinStore::new();
                install_skill(&mut pins, &base).map_err(|e| {
                    format!("{}: installing rug-pull base failed: {e}", case.id)
                })?;
                match admit_skill(&pins, &mutated) {
                    Ok(()) => escapes.push(format!(
                        "{}: admit_skill allowed rug-pulled body (escape)",
                        case.id
                    )),
                    Err(e) => {
                        let needle = case
                            .expected_reject_contains
                            .as_deref()
                            .unwrap_or("pin mismatch");
                        if let Err(check) = assert_rejects(&e.to_string(), needle, &case.id) {
                            escapes.push(check);
                        }
                    }
                }
            }
            "install_and_execute" => {
                let skill = load_skill_dir(&case.skill_dir)?;
                let mut pins = SkillPinStore::new();
                install_skill(&mut pins, &skill).map_err(|e| {
                    format!("{}: benign control failed install: {e}", case.id)
                })?;
                admit_skill(&pins, &skill).map_err(|e| {
                    format!("{}: benign control failed admit: {e}", case.id)
                })?;

                let db = Database::open_in_memory().map_err(|e| e.to_string())?;
                let session_id = "sess-skill03-benign";
                let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
                let workspace = tmp.path();
                fs::write(workspace.join("notes.txt"), "").map_err(|e| e.to_string())?;
                seed_session_with_full_grants(&db, session_id, workspace)?;
                let conn = db.conn();
                SkillExecutor::execute(&conn, session_id, &skill, workspace, &HashMap::new())
                    .map_err(|e| format!("{}: benign execute failed: {e}", case.id))?;
                let content = fs::read_to_string(workspace.join("notes.txt"))
                    .map_err(|e| e.to_string())?;
                if !content.contains("SKILL-03-benign") {
                    return Err(format!(
                        "{}: benign execute did not write expected marker, got {content:?}",
                        case.id
                    ));
                }
            }
            other => return Err(format!("unknown SKILL-03 case kind: {other}")),
        }
    }

    if !escapes.is_empty() {
        return Err(format!(
            "SKILL-03: {poisoned} poisoned cases checked; {} escape(s):\n  - {}",
            escapes.len(),
            escapes.join("\n  - ")
        ));
    }

    // Repo changelog skill must still install under the trusted path (manifest added for 11.1).
    let changelog_path = Path::new("skills/changelog/SKILL.md");
    if changelog_path.is_file() {
        let changelog = SkillLoader::load_from_file(changelog_path).map_err(|e| e.to_string())?;
        let mut pins = SkillPinStore::new();
        install_skill(&mut pins, &changelog).map_err(|e| {
            format!("repo changelog skill must remain installable after SKILL-03: {e}")
        })?;
    }

    Ok(())
}
