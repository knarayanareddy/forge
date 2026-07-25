//! RED-01 adversarial fixture loader (Phase 6 slice 6.7).
//! Harness execution is stubbed until slice 6.7; this module validates frozen cases now.

use serde::Deserialize;
use std::path::Path;

pub const RED_TEAM_FIXTURE_PATH: &str = "tests/golden_harness/fixtures/red_team_prompts.json";
pub const RED_TEAM_MIN_CASES: usize = 12;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RedTeamSurface {
    PermissionManager,
    LoopEngine,
    McpAllowlist,
    AuditChain,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedTeamCase {
    pub id: String,
    pub category: String,
    pub atr_tag: String,
    pub surface: RedTeamSurface,
    pub summary: String,
    pub payload: serde_json::Value,
    pub expected_decision: String,
    pub forbidden_success: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedTeamFixture {
    pub schema_version: u32,
    pub description: String,
    pub min_cases: usize,
    pub cases: Vec<RedTeamCase>,
}

/// Load and validate the frozen RED-01 fixture file.
pub fn load_red_team_fixtures() -> Result<RedTeamFixture, String> {
    let path = resolve_fixture_path()?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let fixture: RedTeamFixture =
        serde_json::from_str(&content).map_err(|e| format!("parse RED-01 fixture: {}", e))?;
    validate_fixture(&fixture)?;
    Ok(fixture)
}

fn resolve_fixture_path() -> Result<std::path::PathBuf, String> {
    let candidates = [
        Path::new(RED_TEAM_FIXTURE_PATH),
        Path::new("fixtures/red_team_prompts.json"),
        Path::new("tests/golden_harness/fixtures/red_team_prompts.json"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(format!(
        "RED-01 fixture not found (tried {:?})",
        candidates
    ))
}

fn validate_fixture(fixture: &RedTeamFixture) -> Result<(), String> {
    if fixture.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {} (expected 1)",
            fixture.schema_version
        ));
    }

    let min = fixture.min_cases.max(RED_TEAM_MIN_CASES);
    if fixture.cases.len() < min {
        return Err(format!(
            "RED-01 requires >= {} cases, found {}",
            min,
            fixture.cases.len()
        ));
    }

    let mut ids = std::collections::HashSet::new();
    for case in &fixture.cases {
        if !ids.insert(case.id.clone()) {
            return Err(format!("duplicate RED-01 case id: {}", case.id));
        }
        if case.atr_tag.is_empty() || case.category.is_empty() {
            return Err(format!("case {} missing category or atr_tag", case.id));
        }
        if case.forbidden_success.is_empty() {
            return Err(format!("case {} missing forbidden_success", case.id));
        }
    }

    Ok(())
}

/// Stub entry for slice 6.7 harness task — loads fixtures only today.
pub fn red01_fixture_ready() -> Result<usize, String> {
    let fixture = load_red_team_fixtures()?;
    Ok(fixture.cases.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_team_fixture_loads_and_meets_minimum() {
        let fixture = load_red_team_fixtures().expect("RED-01 fixture must load");
        assert!(fixture.cases.len() >= RED_TEAM_MIN_CASES);
        assert!(fixture.cases.iter().any(|c| c.category == "path_traversal"));
        assert!(fixture.cases.iter().any(|c| c.category == "json_plan_inject"));
        assert!(fixture.cases.iter().any(|c| c.category == "symlink_escape"));
        assert!(fixture.cases.iter().any(|c| c.category == "fake_done"));
        assert!(fixture.cases.iter().any(|c| c.category == "prompt_injection"));
    }

    #[test]
    fn red01_stub_loader_reports_case_count() {
        let count = red01_fixture_ready().expect("stub loader");
        assert!(count >= RED_TEAM_MIN_CASES);
    }
}
