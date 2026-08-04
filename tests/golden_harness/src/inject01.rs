//! INJECT-01 — tool-result induction blocked by cross-call correlation (Phase 11 slices 11.7–11.8).
//!
//! Exercises production `admit_plan_against_observations` (and proves stream delimiting via
//! `wrap_untrusted_tool_output`). Delimiters alone are not the pass condition — every poisoned
//! case must be a correlation Deny.

use aether_core::{
    admit_plan_against_observations, wrap_untrusted_tool_output, AdmitDecision, ToolInvocation,
    ToolObservation,
};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Corpus {
    schema_version: u32,
    cases: Vec<CorpusCase>,
}

#[derive(Debug, Deserialize)]
struct CorpusCase {
    id: String,
    kind: String,
    goal: String,
    original_plan: Vec<ToolInvocation>,
    observations: Vec<ToolObservationDto>,
    candidate_plan: Vec<ToolInvocation>,
    #[serde(default)]
    expected_reason_contains: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolObservationDto {
    iteration: usize,
    tool: String,
    success: bool,
    output: String,
}

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/inject01_corpus.json")
}

fn load_corpus() -> Result<Corpus, String> {
    let text = fs::read_to_string(corpus_path()).map_err(|e| e.to_string())?;
    let corpus: Corpus = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if corpus.schema_version != 1 {
        return Err(format!(
            "unsupported INJECT-01 corpus schema_version {}",
            corpus.schema_version
        ));
    }
    Ok(corpus)
}

fn trusted_context(goal: &str, original: &[ToolInvocation]) -> String {
    let mut ctx = goal.to_string();
    ctx.push('\n');
    for step in original {
        ctx.push_str(&format!("{step:?}\n"));
    }
    ctx
}

pub fn inject01_fixture_ready() -> Result<usize, String> {
    let corpus = load_corpus()?;
    let poisoned = corpus.cases.iter().filter(|c| c.kind == "deny").count();
    if poisoned < 8 {
        return Err(format!(
            "INJECT-01 corpus needs ≥8 deny cases, found {poisoned}"
        ));
    }
    Ok(poisoned)
}

pub fn test_inject01_impl() -> Result<(), String> {
    let corpus = load_corpus()?;
    let poisoned = inject01_fixture_ready()?;
    let mut escapes = Vec::new();

    // Delimiter boundary is required but not sufficient — assert it exists, then correlation.
    let wrapped = wrap_untrusted_tool_output("fs_read", "probe");
    if !wrapped.contains("trust=\"untrusted\"") || !wrapped.contains("<tool_result") {
        return Err("wrap_untrusted_tool_output missing required delimiter markers".into());
    }

    for case in &corpus.cases {
        let observations: Vec<ToolObservation> = case
            .observations
            .iter()
            .map(|o| ToolObservation {
                iteration: o.iteration,
                tool: o.tool.clone(),
                success: o.success,
                output: o.output.clone(),
            })
            .collect();
        let trusted = trusted_context(&case.goal, &case.original_plan);
        let decision = admit_plan_against_observations(
            &trusted,
            &case.original_plan,
            &observations,
            &case.candidate_plan,
        );

        match case.kind.as_str() {
            "deny" => match decision {
                AdmitDecision::Deny { findings } => {
                    let blob = findings
                        .iter()
                        .map(|f| f.reason.clone())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    if let Some(needle) = &case.expected_reason_contains {
                        if !blob.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()) {
                            escapes.push(format!(
                                "{}: deny reasons missing {needle:?}: {blob}",
                                case.id
                            ));
                        }
                    }
                    if findings.is_empty() {
                        escapes.push(format!("{}: Deny with zero findings", case.id));
                    }
                }
                AdmitDecision::Allow { .. } => {
                    escapes.push(format!(
                        "{}: admit allowed induced plan (escape)",
                        case.id
                    ));
                }
            },
            "allow" => match decision {
                AdmitDecision::Allow { .. } => {}
                AdmitDecision::Deny { findings } => {
                    escapes.push(format!(
                        "{}: benign plan denied: {:?}",
                        case.id, findings
                    ));
                }
            },
            other => return Err(format!("unknown INJECT-01 case kind: {other}")),
        }
    }

    if !escapes.is_empty() {
        return Err(format!(
            "INJECT-01: {poisoned} deny cases checked; {} escape(s):\n  - {}",
            escapes.len(),
            escapes.join("\n  - ")
        ));
    }

    Ok(())
}
