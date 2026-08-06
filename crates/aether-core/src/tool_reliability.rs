//! BFCL-style frozen tool reliability scoring (Phase 12 / RELY-01).
//!
//! Scores are computed from frozen tool-call transcripts — no live model inference.

use crate::{load_discovered_registry, ModelRegistry, RegistryError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_RELIABILITY_FIXTURE: &str =
    "tests/golden_harness/fixtures/rely01_corpus.json";

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ToolCallCase {
    pub id: String,
    pub tool_name: String,
    pub expect_success: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FrozenToolResponse {
    pub tool_name: String,
    pub success: bool,
    pub arguments_match: bool,
    #[serde(default)]
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ToolCaseSpec {
    pub id: String,
    pub tool_name: String,
    pub expect_success: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ToolCaseOutcome {
    pub tool_name: String,
    pub success: bool,
    pub arguments_match: bool,
    #[serde(default)]
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProfileReliability {
    pub profile_id: String,
    pub score: f64,
    pub cases_passed: usize,
    pub cases_total: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProfileScore {
    pub profile_id: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ToolReliabilityCorpus {
    pub schema_version: u32,
    pub cases: Vec<ToolCallCase>,
    pub profiles: HashMap<String, HashMap<String, FrozenToolResponse>>,
}

#[derive(Debug, Error)]
pub enum ReliabilityError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("corpus error: {0}")]
    Corpus(String),
    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),
}

pub fn score_tool_response(case: &ToolCallCase, response: &FrozenToolResponse) -> f64 {
    let mut score = 0.0;
    if response.tool_name == case.tool_name {
        score += 0.4;
    }
    if response.success == case.expect_success {
        score += 0.35;
    }
    if response.arguments_match {
        score += 0.25;
    }
    score
}

pub fn evaluate_profile_reliability(
    cases: &[ToolCallCase],
    responses: &HashMap<String, FrozenToolResponse>,
) -> f64 {
    if cases.is_empty() {
        return 0.0;
    }
    cases
        .iter()
        .map(|c| {
            responses
                .get(&c.id)
                .map(|r| score_tool_response(c, r))
                .unwrap_or(0.0)
        })
        .sum::<f64>()
        / cases.len() as f64
}

pub fn score_profile_from_outcomes(
    cases: &[ToolCallCase],
    outcomes: &HashMap<String, ToolCaseOutcome>,
) -> f64 {
    let responses: HashMap<String, FrozenToolResponse> = outcomes
        .iter()
        .map(|(id, o)| {
            (
                id.clone(),
                FrozenToolResponse {
                    tool_name: o.tool_name.clone(),
                    success: o.success,
                    arguments_match: o.arguments_match,
                    output: o.output.clone(),
                },
            )
        })
        .collect();
    evaluate_profile_reliability(cases, &responses)
}

pub fn rank_profiles_by_reliability(
    profiles: &[(String, HashMap<String, FrozenToolResponse>)],
    cases: &[ToolCallCase],
) -> Vec<(String, f64)> {
    let mut ranked: Vec<(String, f64)> = profiles
        .iter()
        .map(|(id, responses)| (id.clone(), evaluate_profile_reliability(cases, responses)))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    ranked
}

pub fn discover_reliability_fixture() -> PathBuf {
    if let Ok(path) = std::env::var("AETHER_RELY01_FIXTURE") {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(DEFAULT_RELIABILITY_FIXTURE)
}

pub fn load_reliability_corpus(path: &Path) -> Result<ToolReliabilityCorpus, ReliabilityError> {
    let raw = std::fs::read_to_string(path)?;
    let corpus: ToolReliabilityCorpus = serde_json::from_str(&raw)?;
    if corpus.schema_version != 1 {
        return Err(ReliabilityError::Corpus(format!(
            "unsupported schema_version {}",
            corpus.schema_version
        )));
    }
    if corpus.cases.len() != 6 {
        return Err(ReliabilityError::Corpus(format!(
            "expected 6 cases, got {}",
            corpus.cases.len()
        )));
    }
    Ok(corpus)
}

pub fn profiles_with_reliability(
    corpus: &ToolReliabilityCorpus,
) -> Vec<ProfileReliability> {
    corpus
        .profiles
        .iter()
        .map(|(profile_id, responses)| {
            let score = evaluate_profile_reliability(&corpus.cases, responses);
            let cases_passed = corpus
                .cases
                .iter()
                .filter(|c| {
                    responses
                        .get(&c.id)
                        .map(|r| score_tool_response(c, r) >= 1.0)
                        .unwrap_or(false)
                })
                .count();
            ProfileReliability {
                profile_id: profile_id.clone(),
                score,
                cases_passed,
                cases_total: corpus.cases.len(),
            }
        })
        .collect()
}

pub fn verify_ranking(corpus: &ToolReliabilityCorpus) -> Result<(), ReliabilityError> {
    let profiles: Vec<(String, HashMap<String, FrozenToolResponse>)> = corpus
        .profiles
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let ranked = rank_profiles_by_reliability(&profiles, &corpus.cases);
    let local = ranked
        .iter()
        .find(|(id, _)| id == "ollama-local")
        .map(|(_, s)| *s)
        .ok_or_else(|| ReliabilityError::Corpus("missing ollama-local".into()))?;
    let complex = ranked
        .iter()
        .find(|(id, _)| id == "ollama-complex")
        .map(|(_, s)| *s)
        .ok_or_else(|| ReliabilityError::Corpus("missing ollama-complex".into()))?;
    if complex <= local {
        return Err(ReliabilityError::Corpus(format!(
            "ollama-complex ({complex}) must outrank ollama-local ({local})"
        )));
    }
    if (local - 0.667).abs() > 0.01 {
        return Err(ReliabilityError::Corpus(format!(
            "ollama-local score {local} != 0.667"
        )));
    }
    if (complex - 1.0).abs() > 0.01 {
        return Err(ReliabilityError::Corpus(format!(
            "ollama-complex score {complex} != 1.0"
        )));
    }
    Ok(())
}

pub fn registry_reliability_matches_corpus(
    registry: &ModelRegistry,
    corpus: &ToolReliabilityCorpus,
) -> Result<(), ReliabilityError> {
    for profile_id in ["ollama-local", "ollama-complex"] {
        let profile = registry.profile(profile_id)?;
        let expected = corpus
            .profiles
            .get(profile_id)
            .map(|responses| evaluate_profile_reliability(&corpus.cases, responses))
            .ok_or_else(|| ReliabilityError::Corpus(format!("missing profile {profile_id}")))?;
        let recorded = profile
            .tool_reliability
            .ok_or_else(|| ReliabilityError::Corpus(format!("{profile_id} missing tool_reliability")))?;
        if (recorded - expected).abs() > 0.01 {
            return Err(ReliabilityError::Corpus(format!(
                "{profile_id} registry tool_reliability {recorded} != corpus {expected}"
            )));
        }
    }
    Ok(())
}

pub fn load_registry_reliability_check() -> Result<(), ReliabilityError> {
    let corpus = load_reliability_corpus(&discover_reliability_fixture())?;
    verify_ranking(&corpus)?;
    let registry = load_discovered_registry()?;
    registry_reliability_matches_corpus(&registry, &corpus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complex_outranks_local_fixture() {
        let corpus = load_reliability_corpus(&discover_reliability_fixture()).expect("fixture");
        verify_ranking(&corpus).expect("ranking");
    }
}
