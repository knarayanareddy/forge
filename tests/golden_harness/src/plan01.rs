//! PLAN-01 — diverse NL planner routing with constrained tool schemas (Phase 9 slice 9.4).

use aether_core::{
    plan_tool_name, run_nl_planner, ModelBackend, ModelRouter, OllamaProvider, ToolInvocation,
};
use serde::Deserialize;

const FIXTURE: &str = include_str!("../fixtures/plan01_goals.json");

#[derive(Debug, Deserialize)]
pub struct PlanFixture {
    pub schema_version: u32,
    pub minimum_pass_rate: f64,
    pub cases: Vec<PlanCase>,
}

#[derive(Debug, Deserialize)]
pub struct PlanCase {
    pub id: String,
    pub goal: String,
    pub required_tools: Vec<String>,
    pub forbidden_tools: Vec<String>,
}

pub fn load_plan01_fixture() -> Result<PlanFixture, String> {
    let fixture: PlanFixture =
        serde_json::from_str(FIXTURE).map_err(|e| format!("PLAN-01 fixture invalid: {}", e))?;
    if fixture.schema_version != 1 {
        return Err(format!(
            "PLAN-01 unsupported schema version {}",
            fixture.schema_version
        ));
    }
    if fixture.cases.len() < 10 {
        return Err(format!(
            "PLAN-01 requires at least 10 diverse goals, found {}",
            fixture.cases.len()
        ));
    }
    if !(0.0..=1.0).contains(&fixture.minimum_pass_rate) {
        return Err("PLAN-01 minimum_pass_rate must be in [0,1]".into());
    }
    Ok(fixture)
}

pub fn plan01_fixture_ready() -> Result<usize, String> {
    load_plan01_fixture().map(|fixture| fixture.cases.len())
}

pub async fn test_plan01_impl() -> Result<(), String> {
    let fixture = load_plan01_fixture()?;
    let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let chat_model =
        std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());

    OllamaProvider::health_check(&endpoint)
        .await
        .map_err(|e| format!("PLAN-01 requires Ollama: {}", e))?;
    OllamaProvider::warm_chat_model(&endpoint, &chat_model, 1)
        .await
        .map_err(|e| format!("PLAN-01 model warmup failed: {}", e))?;

    let router = ModelRouter::new(
        ModelBackend::OllamaMlx {
            endpoint,
            model: chat_model,
        },
        None,
    );

    let mut passed = 0usize;
    let mut failures = Vec::new();
    for case in &fixture.cases {
        match run_nl_planner(&router, &case.goal, 8).await {
            Ok(plan) => {
                let tools: Vec<&str> = plan.iter().map(plan_tool_name).collect();
                let required_present = case
                    .required_tools
                    .iter()
                    .all(|required| tools.contains(&required.as_str()));
                let forbidden_present: Vec<&str> = case
                    .forbidden_tools
                    .iter()
                    .filter_map(|forbidden| {
                        tools
                            .contains(&forbidden.as_str())
                            .then_some(forbidden.as_str())
                    })
                    .collect();
                let ends_done = matches!(plan.last(), Some(ToolInvocation::Done));

                if required_present && forbidden_present.is_empty() && ends_done {
                    passed += 1;
                } else {
                    failures.push(format!(
                        "{} tools={:?}, required={:?}, forbidden_present={:?}, ends_done={}",
                        case.id,
                        tools,
                        case.required_tools,
                        forbidden_present,
                        ends_done
                    ));
                }
            }
            Err(e) => failures.push(format!("{} planner error: {}", case.id, e)),
        }
    }

    let rate = passed as f64 / fixture.cases.len() as f64;
    if rate < fixture.minimum_pass_rate {
        return Err(format!(
            "PLAN-01 semantic routing {}/{} ({:.0}%) below {:.0}%: {}",
            passed,
            fixture.cases.len(),
            rate * 100.0,
            fixture.minimum_pass_rate * 100.0,
            failures.join(" | ")
        ));
    }

    if !failures.is_empty() {
        eprintln!(
            "PLAN-01 tolerated {} case(s) within threshold: {}",
            failures.len(),
            failures.join(" | ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_has_diverse_required_tools() {
        let fixture = load_plan01_fixture().unwrap();
        let required: std::collections::HashSet<&str> = fixture
            .cases
            .iter()
            .flat_map(|case| case.required_tools.iter().map(String::as_str))
            .collect();
        for tool in [
            "fs_write",
            "fs_read",
            "python_lint",
            "git_init",
            "mcp_call",
            "skill_execute",
        ] {
            assert!(required.contains(tool), "fixture must exercise {}", tool);
        }
    }
}
