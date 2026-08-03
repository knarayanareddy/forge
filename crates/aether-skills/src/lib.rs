mod disclosure;
mod trust;

pub use disclosure::{
    citation_fidelity, compose_citation_answer, route_chapter_for_query, DisclosureEntry,
    DisclosureIndex, DisclosureKind, RoutedChapter,
};

pub use trust::{
    admit_skill, install_skill, scan_credential_paths, scan_skill_injection, skill_content_hash,
    validate_manifest_not_overbroad, validate_steps_within_manifest, SkillCapabilityManifest,
    SkillPinStore, CREDENTIAL_PATH_PATTERNS, INJECTION_PATTERNS,
};

use aether_permissions::{PermissionDecision, PermissionManager};
use aether_sandbox::ProductionSandbox;
use rusqlite::Connection;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SkillError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillStep {
    AppendFile {
        path: String,
        template: String,
    },
    ReadFile {
        path: String,
    },
}

#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub markdown_body: String,
    pub steps: Vec<SkillStep>,
    /// Present when the skill declares `filesystem` / `network` / `tools` in frontmatter
    /// (Phase 11 / SKILL-03). Required for [`install_skill`] / [`admit_skill`].
    pub capabilities: Option<SkillCapabilityManifest>,
}

pub struct SkillLoader;

impl SkillLoader {
    /// Load a SKILL.md file from disk (agentskills.io-style frontmatter + ## Steps section).
    pub fn load_from_file(path: &Path) -> Result<SkillDefinition, SkillError> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content, path)
    }

    /// Load all skills from subdirectories of `skills_dir` that contain SKILL.md.
    pub fn load_directory(skills_dir: &Path) -> Result<Vec<SkillDefinition>, SkillError> {
        let mut skills = Vec::new();
        if !skills_dir.is_dir() {
            return Ok(skills);
        }

        for entry in fs::read_dir(skills_dir)? {
            let entry = entry?;
            let skill_path = entry.path().join("SKILL.md");
            if skill_path.is_file() {
                skills.push(Self::load_from_file(&skill_path)?);
            }
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(skills)
    }

    pub fn parse(content: &str, source_path: &Path) -> Result<SkillDefinition, SkillError> {
        let (frontmatter, body) = split_frontmatter(content)?;
        let meta = parse_frontmatter(&frontmatter)?;

        let name = meta
            .get("name")
            .cloned()
            .ok_or_else(|| SkillError::Parse("Missing 'name' in SKILL.md frontmatter".into()))?;
        let description = meta
            .get("description")
            .cloned()
            .ok_or_else(|| SkillError::Parse("Missing 'description' in SKILL.md frontmatter".into()))?;

        let id = source_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(&name)
            .to_string();

        let steps = parse_steps(body)?;
        let capabilities = parse_capabilities(&meta)?;

        Ok(SkillDefinition {
            id,
            name,
            description,
            markdown_body: content.to_string(),
            steps,
            capabilities,
        })
    }
}

pub struct SkillExecutor;

impl SkillExecutor {
    /// Execute procedural skill steps against a workspace with grant-based permission checks.
    ///
    /// When the skill declares a capability manifest, step actions/paths must stay inside it
    /// (Phase 11 / SKILL-03). Credential-shaped paths are always rejected, manifest or not.
    pub fn execute(
        conn: &Connection,
        session_id: &str,
        skill: &SkillDefinition,
        workspace: &Path,
        variables: &HashMap<String, String>,
    ) -> Result<(), SkillError> {
        // Re-run install-time trust checks at execute so skipping `install_skill` is not an
        // escape hatch for a poisoned body that somehow entered the skill map.
        scan_skill_injection(skill)?;
        scan_credential_paths(skill)?;
        if let Some(caps) = &skill.capabilities {
            validate_manifest_not_overbroad(&skill.id, caps)?;
            validate_steps_within_manifest(skill, caps)?;
        }

        for step in &skill.steps {
            match step {
                SkillStep::ReadFile { path } => {
                    let full_path = ProductionSandbox::validate_target(workspace, Path::new(path))
                        .map_err(|e| SkillError::Execution(e.to_string()))?;
                    let full_str = full_path.to_string_lossy().to_string();
                    let decision = PermissionManager::check_file_access(conn, session_id, &full_str, "read")
                        .map_err(|e| SkillError::Db(e))?;
                    if decision != PermissionDecision::Approved {
                        return Err(SkillError::PermissionDenied(format!(
                            "Read denied for {} without grant",
                            full_str
                        )));
                    }
                    ProductionSandbox::read_to_string(workspace, &full_path)
                        .map_err(|e| SkillError::Execution(e.to_string()))?;
                    PermissionManager::audit_decision(
                        conn,
                        session_id,
                        "skill_read_file",
                        &format!(r#"{{"skill": "{}", "path": "{}"}}"#, skill.name, full_str),
                        &decision,
                        Some(0),
                        None,
                    )
                    .map_err(SkillError::Db)?;
                }
                SkillStep::AppendFile { path, template } => {
                    let full_path = ProductionSandbox::validate_target(workspace, Path::new(path))
                        .map_err(|e| SkillError::Execution(e.to_string()))?;
                    let full_str = full_path.to_string_lossy().to_string();
                    let decision = PermissionManager::check_file_access(conn, session_id, &full_str, "write")
                        .map_err(SkillError::Db)?;
                    if decision != PermissionDecision::Approved {
                        return Err(SkillError::PermissionDenied(format!(
                            "Write denied for {} without grant",
                            full_str
                        )));
                    }

                    let rendered = render_template(template, variables);
                    ProductionSandbox::append_file(workspace, &full_path, rendered.as_bytes())
                        .map_err(|e| SkillError::Execution(e.to_string()))?;

                    PermissionManager::audit_decision(
                        conn,
                        session_id,
                        "skill_append_file",
                        &format!(r#"{{"skill": "{}", "path": "{}"}}"#, skill.name, full_str),
                        &decision,
                        Some(0),
                        None,
                    )
                    .map_err(SkillError::Db)?;
                }
            }
        }

        Ok(())
    }
}

/// Upsert a skill definition into procedural_skills.
pub fn register_skill(conn: &Connection, skill: &SkillDefinition) -> Result<(), SkillError> {
    conn.execute(
        "INSERT INTO procedural_skills (id, name, description, markdown_body)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           description = excluded.description,
           markdown_body = excluded.markdown_body,
           updated_at = CURRENT_TIMESTAMP",
        rusqlite::params![skill.id, skill.name, skill.description, skill.markdown_body],
    )?;
    Ok(())
}

pub fn record_skill_success(conn: &Connection, skill_id: &str) -> Result<(), SkillError> {
    conn.execute(
        "UPDATE procedural_skills SET success_count = success_count + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        rusqlite::params![skill_id],
    )?;
    Ok(())
}

pub fn record_skill_failure(conn: &Connection, skill_id: &str) -> Result<(), SkillError> {
    conn.execute(
        "UPDATE procedural_skills SET failure_count = failure_count + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        rusqlite::params![skill_id],
    )?;
    Ok(())
}

fn split_frontmatter(content: &str) -> Result<(String, &str), SkillError> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(SkillError::Parse("SKILL.md must begin with YAML frontmatter (---)".into()));
    }
    let rest = &trimmed[3..];
    let end = rest
        .find("\n---")
        .ok_or_else(|| SkillError::Parse("Unclosed YAML frontmatter".into()))?;
    let frontmatter = rest[..end].trim().to_string();
    let body = &rest[end + 4..];
    Ok((frontmatter, body))
}

fn parse_frontmatter(yaml: &str) -> Result<HashMap<String, String>, SkillError> {
    let mut map = HashMap::new();
    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        map.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(map)
}

/// Parse optional capability manifest keys from flat frontmatter:
/// `filesystem: a.txt,b.txt`, `network: false`, `tools: read_file,append_file`.
/// All three must be present together; partial declarations are a parse error.
fn parse_capabilities(
    meta: &HashMap<String, String>,
) -> Result<Option<SkillCapabilityManifest>, SkillError> {
    let has_fs = meta.contains_key("filesystem");
    let has_net = meta.contains_key("network");
    let has_tools = meta.contains_key("tools");
    if !has_fs && !has_net && !has_tools {
        return Ok(None);
    }
    if !(has_fs && has_net && has_tools) {
        return Err(SkillError::Parse(
            "Capability manifest requires filesystem, network, and tools together".into(),
        ));
    }

    let filesystem = meta["filesystem"]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let network = match meta["network"].to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => true,
        "false" | "no" | "0" | "none" => false,
        other => {
            return Err(SkillError::Parse(format!(
                "Invalid network capability value: {other}"
            )));
        }
    };
    let tools = meta["tools"]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    Ok(Some(SkillCapabilityManifest {
        filesystem,
        network,
        tools,
    }))
}

fn parse_steps(body: &str) -> Result<Vec<SkillStep>, SkillError> {
    let Some(steps_section) = extract_section(body, "Steps") else {
        return Err(SkillError::Parse("Missing '## Steps' section in SKILL.md".into()));
    };

    let mut steps = Vec::new();
    let mut current_action: Option<String> = None;
    let mut current_path: Option<String> = None;
    let mut current_template: Option<String> = None;

    for line in steps_section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("- action:") {
            if let Some(step) = flush_step(&current_action, &current_path, &current_template)? {
                steps.push(step);
            }
            current_action = Some(trimmed["- action:".len()..].trim().to_string());
            current_path = None;
            current_template = None;
        } else if trimmed.starts_with("path:") {
            current_path = Some(trimmed["path:".len()..].trim().to_string());
        } else if trimmed.starts_with("template:") {
            current_template = Some(trimmed["template:".len()..].trim().to_string());
        }
    }

    if let Some(step) = flush_step(&current_action, &current_path, &current_template)? {
        steps.push(step);
    }

    if steps.is_empty() {
        return Err(SkillError::Parse("No executable steps found under ## Steps".into()));
    }

    Ok(steps)
}

fn flush_step(
    action: &Option<String>,
    path: &Option<String>,
    template: &Option<String>,
) -> Result<Option<SkillStep>, SkillError> {
    let Some(action) = action else {
        return Ok(None);
    };
    let path = path
        .clone()
        .ok_or_else(|| SkillError::Parse(format!("Step '{}' missing path", action)))?;

    let step = match action.as_str() {
        "read_file" => SkillStep::ReadFile { path },
        "append_file" => {
            let template = template
                .clone()
                .ok_or_else(|| SkillError::Parse(format!("Step '{}' missing template", action)))?;
            SkillStep::AppendFile { path, template }
        }
        other => {
            return Err(SkillError::Parse(format!("Unknown skill action: {}", other)));
        }
    };

    Ok(Some(step))
}

fn extract_section<'a>(body: &'a str, title: &str) -> Option<&'a str> {
    let marker = format!("## {}", title);
    let start = body.find(&marker)?;
    let rest = &body[start + marker.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(&rest[..end])
}

fn render_template(template: &str, variables: &HashMap<String, String>) -> String {
    let mut rendered = template.to_string();
    for (key, value) in variables {
        rendered = rendered.replace(&format!("{{{{{}}}}}", key), value);
    }
    rendered
}

/// Minimal layout check for progressive-disclosure skills (SKILL-02 prep).
/// Procedural skills use `SkillLoader`; multi-chapter routing skills use this validator.
pub fn validate_progressive_skill_layout(skill_dir: &Path) -> Result<(), SkillError> {
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.is_file() {
        return Err(SkillError::Parse(format!(
            "Missing SKILL.md in {}",
            skill_dir.display()
        )));
    }

    let content = fs::read_to_string(&skill_md)?;
    let (frontmatter, body) = split_frontmatter(&content)?;
    let meta = parse_frontmatter(&frontmatter)?;

    for key in ["name", "description"] {
        if !meta.contains_key(key) {
            return Err(SkillError::Parse(format!(
                "Progressive skill missing '{}' in frontmatter",
                key
            )));
        }
    }

    if extract_section(body, "Chapters").is_none() {
        return Err(SkillError::Parse(
            "Progressive skill missing '## Chapters' section".into(),
        ));
    }

    let chapters_dir = skill_dir.join("chapters");
    if !chapters_dir.is_dir() {
        return Err(SkillError::Parse(format!(
            "Missing chapters/ directory in {}",
            skill_dir.display()
        )));
    }

    let chapter_count = fs::read_dir(&chapters_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .count();

    if chapter_count == 0 {
        return Err(SkillError::Parse(
            "chapters/ must contain at least one .md file".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_changelog_skill() {
        let content = r#"---
name: changelog
description: Append a dated entry to CHANGELOG.md
---

# Changelog

## Steps

- action: read_file
  path: CHANGELOG.md
- action: append_file
  path: CHANGELOG.md
  template: "- {{date}}: {{entry}}\n"
"#;
        let skill = SkillLoader::parse(content, Path::new("skills/changelog/SKILL.md")).unwrap();
        assert_eq!(skill.name, "changelog");
        assert_eq!(skill.steps.len(), 2);
    }

    #[test]
    fn test_progressive_skill_fixture_layout() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        for fixture_name in ["rust-cookbook", "forge-api-guide", "book_skill"] {
            let fixture = Path::new(manifest).join(format!(
                "../../tests/golden_harness/fixtures/skills/{fixture_name}"
            ));
            validate_progressive_skill_layout(&fixture)
                .unwrap_or_else(|e| panic!("{fixture_name} fixture layout: {e}"));
        }
    }
}
