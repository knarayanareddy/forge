//! SKILL-02 book-to-skill eval rubric loader (Phase 6 slice 6.8).
//! Harness execution is stubbed until slice 6.8; this module validates frozen rubric now.

use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const SKILL02_FIXTURE_DIR: &str = "tests/golden_harness/fixtures/skills/book_skill";
pub const SKILL02_RUBRIC_FILE: &str = "skill_eval_rubric.json";
pub const SKILL02_MIN_QUESTIONS: usize = 3;
pub const SKILL02_CITATION_THRESHOLD: f64 = 0.9;

#[derive(Debug, Clone, Deserialize)]
pub struct SkillEvalQuestion {
    pub id: String,
    pub query: String,
    pub expected_chapter: String,
    pub routing_keywords: Vec<String>,
    pub citation_span: String,
    #[serde(default)]
    pub forbidden_hallucinations: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillEvalRubric {
    pub schema_version: u32,
    pub skill_id: String,
    pub description: String,
    pub citation_fidelity_threshold: f64,
    pub citation_fidelity_notes: String,
    pub source_uri: String,
    pub questions: Vec<SkillEvalQuestion>,
}

/// Load and validate the frozen SKILL-02 rubric + fixture layout.
pub fn load_skill_eval_rubric() -> Result<SkillEvalRubric, String> {
    let fixture_dir = resolve_fixture_dir()?;
    let rubric_path = fixture_dir.join(SKILL02_RUBRIC_FILE);
    let content = std::fs::read_to_string(&rubric_path)
        .map_err(|e| format!("read {}: {}", rubric_path.display(), e))?;
    let rubric: SkillEvalRubric =
        serde_json::from_str(&content).map_err(|e| format!("parse SKILL-02 rubric: {}", e))?;
    validate_rubric(&rubric, &fixture_dir)?;
    Ok(rubric)
}

fn resolve_fixture_dir() -> Result<PathBuf, String> {
    let candidates = [
        Path::new(SKILL02_FIXTURE_DIR),
        Path::new("fixtures/skills/book_skill"),
        Path::new("tests/golden_harness/fixtures/skills/book_skill"),
    ];
    for candidate in candidates {
        if candidate.join("SKILL.md").is_file() {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(format!(
        "SKILL-02 fixture dir not found (tried {:?})",
        candidates
    ))
}

fn validate_rubric(rubric: &SkillEvalRubric, fixture_dir: &Path) -> Result<(), String> {
    if rubric.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {} (expected 1)",
            rubric.schema_version
        ));
    }

    if rubric.questions.len() < SKILL02_MIN_QUESTIONS {
        return Err(format!(
            "SKILL-02 requires >= {} questions, found {}",
            SKILL02_MIN_QUESTIONS,
            rubric.questions.len()
        ));
    }

    if rubric.citation_fidelity_threshold < SKILL02_CITATION_THRESHOLD {
        return Err(format!(
            "citation_fidelity_threshold {} below required {}",
            rubric.citation_fidelity_threshold, SKILL02_CITATION_THRESHOLD
        ));
    }

    let skill_md = fixture_dir.join("SKILL.md");
    if !skill_md.is_file() {
        return Err(format!("missing SKILL.md in {}", fixture_dir.display()));
    }

    let references = fixture_dir.join("references/source.md");
    if !references.is_file() {
        return Err(format!(
            "missing skill-creator references/source.md in {}",
            fixture_dir.display()
        ));
    }

    let chapters_dir = fixture_dir.join("chapters");
    if !chapters_dir.is_dir() {
        return Err(format!(
            "missing chapters/ directory in {}",
            fixture_dir.display()
        ));
    }

    let mut ids = std::collections::HashSet::new();
    for question in &rubric.questions {
        if !ids.insert(question.id.clone()) {
            return Err(format!("duplicate SKILL-02 question id: {}", question.id));
        }
        if question.citation_span.is_empty() {
            return Err(format!("question {} missing citation_span", question.id));
        }
        let chapter_path = fixture_dir.join(&question.expected_chapter);
        if !chapter_path.is_file() {
            return Err(format!(
                "question {} references missing chapter {}",
                question.id,
                chapter_path.display()
            ));
        }
        let chapter_text = std::fs::read_to_string(&chapter_path).map_err(|e| e.to_string())?;
        if !chapter_text.contains(&question.citation_span) {
            return Err(format!(
                "question {} citation_span not found in {}",
                question.id,
                chapter_path.display()
            ));
        }
    }

    aether_skills::validate_progressive_skill_layout(fixture_dir)
        .map_err(|e| format!("progressive skill layout: {}", e))?;

    Ok(())
}

/// Stub entry for slice 6.8 harness task — validates rubric + fixtures only today.
pub fn skill02_fixture_ready() -> Result<usize, String> {
    let rubric = load_skill_eval_rubric()?;
    Ok(rubric.questions.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill02_rubric_loads_with_three_questions() {
        let rubric = load_skill_eval_rubric().expect("SKILL-02 rubric must load");
        assert!(rubric.questions.len() >= SKILL02_MIN_QUESTIONS);
        assert!(rubric.citation_fidelity_threshold >= SKILL02_CITATION_THRESHOLD);
        assert_eq!(rubric.skill_id, "book_skill");
    }

    #[test]
    fn skill02_stub_loader_reports_question_count() {
        let count = skill02_fixture_ready().expect("stub loader");
        assert!(count >= SKILL02_MIN_QUESTIONS);
    }

    #[test]
    fn skill02_citation_spans_exist_in_chapters() {
        let rubric = load_skill_eval_rubric().expect("rubric");
        for q in &rubric.questions {
            assert!(
                !q.citation_span.is_empty(),
                "question {} must define citation_span",
                q.id
            );
            assert!(
                !q.expected_chapter.is_empty(),
                "question {} must define expected_chapter",
                q.id
            );
        }
    }
}
