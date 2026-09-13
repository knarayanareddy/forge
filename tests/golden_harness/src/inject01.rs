//! INJECT-01 — tool-result induction blocked by cross-call correlation (Phase 11 slices 11.7–11.8).
//!
//! Exercises production `admit_plan_against_observations` (and proves stream delimiting via
//! `wrap_untrusted_tool_output`). Delimiters alone are not the pass condition — every poisoned
//! case must be a correlation Deny.
//!
//! P1-5 adds a second bar. The `cohort: "paraphrase"` cases express the same induced-tool intent with
//! **zero** of the frozen phrases — alternate wording, split tokens, non-English, an encoded payload —
//! plus one benign control whose long token was in the goal all along. For that cohort this task also
//! asserts that no frozen phrase matches and that every finding carries `FindingLeg::ContentCorrelation`,
//! so the test cannot be passed by widening the denylist: a wider list that matched this cohort would
//! fail it. And it asserts the finding's other half — correlation-only denials become *approvable*
//! ([`admit_plan_with_confirmation`]), while a genuine phrase hit stays a hard denial.

use aether_core::{
    admit_plan_against_observations, admit_plan_with_confirmation, tool_result_has_injection_phrase,
    wrap_untrusted_tool_output, AdmitDecision, AdmitOutcome, FindingLeg, ToolInvocation,
    ToolObservation, TOOL_RESULT_INJECTION_PATTERNS,
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
    /// Corpus cohort. Absent for the frozen wave-1 cases; `"paraphrase"` marks the P1-5 cohort, which
    /// must be decided by correlation with the denylist silent.
    #[serde(default)]
    cohort: String,
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

fn observations_of(case: &CorpusCase) -> Vec<ToolObservation> {
    case.observations
        .iter()
        .map(|o| ToolObservation {
            iteration: o.iteration,
            tool: o.tool.clone(),
            success: o.success,
            output: o.output.clone(),
        })
        .collect()
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
    let paraphrase = corpus
        .cases
        .iter()
        .filter(|case| case.cohort == "paraphrase")
        .count();
    if paraphrase < 5 {
        return Err(format!(
            "INJECT-01 corpus needs ≥5 cohort=paraphrase cases (P1-5: alternate wording, split \
             tokens, non-English, encoded payload, benign control), found {paraphrase}"
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
        let observations = observations_of(case);
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

    // --- P1-5: paraphrased inducement is stopped by correlation, with the denylist silent ---------
    //
    // Anti-theater applied to the test itself. Nothing below can be satisfied by widening
    // `TOOL_RESULT_INJECTION_PATTERNS`: a wider list that matched this cohort fails the phrase-silence
    // check, and a finding produced by the phrase leg fails the leg check. The only way to stay green
    // is for correlation to do the work.
    let paraphrase: Vec<&CorpusCase> = corpus
        .cases
        .iter()
        .filter(|case| case.cohort == "paraphrase")
        .collect();
    for case in paraphrase {
        let observations = observations_of(case);
        let trusted = trusted_context(&case.goal, &case.original_plan);

        // (1) The frozen denylist is silent — checked against the live list, not a snapshot of it.
        for observation in &observations {
            if tool_result_has_injection_phrase(&observation.output) {
                escapes.push(format!(
                    "{}: observation matches the live denylist, so this case proves nothing about \
                     correlation",
                    case.id
                ));
            }
            let lower = observation.output.to_ascii_lowercase();
            for pattern in TOOL_RESULT_INJECTION_PATTERNS {
                if lower.contains(*pattern) {
                    escapes.push(format!(
                        "{}: observation matches frozen phrase {pattern:?} — widening the list must \
                         never be able to pass this test",
                        case.id
                    ));
                }
            }
        }

        // (2) Correlation still decides it, and every finding came from the correlation leg.
        if let AdmitDecision::Deny { findings } = admit_plan_against_observations(
            &trusted,
            &case.original_plan,
            &observations,
            &case.candidate_plan,
        ) {
            for finding in &findings {
                if finding.leg != FindingLeg::ContentCorrelation {
                    escapes.push(format!(
                        "{}: step {} was flagged by {:?}, so the denylist did the work",
                        case.id, finding.step_index, finding.leg
                    ));
                }
            }
        }

        // (3) The third leg: a correlation-only refusal becomes a consent question, with the inducing
        //     observation shown next to the induced step and still marked untrusted — a confirmation
        //     screen must not become the injection vector.
        match admit_plan_with_confirmation(
            &trusted,
            &case.original_plan,
            &observations,
            &case.candidate_plan,
        ) {
            AdmitOutcome::Allow { .. } => {
                if case.kind == "deny" {
                    escapes.push(format!(
                        "{}: paraphrased induction escaped the correlation gate",
                        case.id
                    ));
                }
            }
            AdmitOutcome::RequireApproval { requests } => {
                if case.kind != "deny" {
                    escapes.push(format!(
                        "{}: a benign case must not need approval, got {} request(s)",
                        case.id,
                        requests.len()
                    ));
                }
                if requests.is_empty() {
                    escapes.push(format!(
                        "{}: approval required with no request to show a person",
                        case.id
                    ));
                }
                for request in &requests {
                    if request.step_index >= case.candidate_plan.len() {
                        escapes.push(format!(
                            "{}: approval request points at step {}, past the end of a {}-step plan",
                            case.id,
                            request.step_index,
                            case.candidate_plan.len()
                        ));
                    }
                    if !request.evidence.contains("trust=\"untrusted\"") {
                        escapes.push(format!(
                            "{}: the confirmation screen must keep the inducing text marked untrusted",
                            case.id
                        ));
                    }
                    let rendered = request.render();
                    if !rendered.contains("data, not instructions") {
                        escapes.push(format!(
                            "{}: the confirmation prompt lost its framing: {rendered}",
                            case.id
                        ));
                    }
                }
            }
            AdmitOutcome::Deny { findings } => escapes.push(format!(
                "{}: denied on a denylist leg ({:?}) although no frozen phrase matched",
                case.id,
                findings.iter().map(|finding| finding.leg).collect::<Vec<_>>()
            )),
        }
    }

    // (4) The other direction: a genuine denylist hit stays a hard denial. Confirmation is for evidence
    //     a person can read, and "the substring 'now call' appeared" is the denylist talking.
    let phrase_case = corpus
        .cases
        .iter()
        .find(|case| case.id == "induce-mcp-via-phrase")
        .ok_or_else(|| "INJECT-01 corpus lost induce-mcp-via-phrase".to_string())?;
    let phrase_observations = observations_of(phrase_case);
    let phrase_trusted = trusted_context(&phrase_case.goal, &phrase_case.original_plan);
    match admit_plan_with_confirmation(
        &phrase_trusted,
        &phrase_case.original_plan,
        &phrase_observations,
        &phrase_case.candidate_plan,
    ) {
        AdmitOutcome::Deny { findings } if findings.iter().any(|f| !f.leg.approvable()) => {}
        other => escapes.push(format!(
            "induce-mcp-via-phrase must stay a hard deny, got {other:?}"
        )),
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
