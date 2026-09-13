//! GATE-04 — dark launch: measure a gate before enforcing it (P2-14).
//!
//! Forge's harness grows gates continuously, and until now the only instrument for a new gate was the
//! scoreboard: enforce it, watch tasks go red, and guess whether the red is the gate being wrong or the
//! gate being right about something the task never intended. That is how good safety work gets reverted.
//! `AETHER_GATE_MODE=<gate>:log` lets a blocking gate record what it *would* have denied and let the call
//! through, so it can be judged on real traffic or a frozen corpus before it is allowed to break a run.
//!
//! What this pins, in order:
//!
//! * **The spec parser is closed and fail-safe.** Only registered blocking gates can be dark-launched,
//!   a typo is refused with the list of names that do exist, one bad entry invalidates the whole spec,
//!   and output redaction is refused *with the reason* — logging instead of redacting is the leak.
//! * **Enforce is the default and stays the default under a configuration error.** With no variable,
//!   with an empty one, and with an unparsable one, every frozen adversarial input is still refused.
//! * **Log mode records instead of blocking.** Every adversarial input across both corpora produces
//!   exactly one would-deny hit carrying triageable detail; no benign input produces one; and enforce
//!   mode records nothing at all, so a gate's allow path costs nothing.
//! * **The promotion criterion is a measurement, not a mood.** Over the frozen corpora — the labelled
//!   hook cases plus all ten INJECT-01 plan cases — every gate comes out with zero false positives and
//!   zero misses, and only then reports `ready to enforce`. The same ledger, given one over-broad
//!   decision or one miss, reports **not** ready: the instrument has to be able to fail.
//! * **Nothing leaks.** The environment is restored even when an assertion returns early, because a
//!   leaked `AETHER_GATE_MODE` would silently un-enforce gates for every task that runs afterwards —
//!   the one failure mode this feature must never have.
//!
//! Soft, per the review: it mutates process environment and reads a process-wide telemetry sink, so it
//! is a whole-process property rather than a pure function. The harness runs tasks sequentially, which
//! is what makes that safe here.

use aether_core::{
    admit_plan_against_observations, drain_gate_hits, enforce_user_prompt_submit, gate_hits,
    pre_tool_use_path_check, AdmitDecision, CorpusLabel, GateLedger, GateMode, GateSpec,
    GateSpecError, HookDecision, ToolInvocation, ToolObservation, DARK_LAUNCHABLE_GATES,
    GATE_ENV_VAR,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

const PROMPT_GATE: &str = "hook.prompt_denylist";
const PATH_GATE: &str = "hook.path_denylist";
const ADMISSION_GATE: &str = "inject.plan_admission";

/// Restores the environment even when an assertion returns early. Same shape as the `EnvGuard` in
/// `sec01`/`ckpt01`/`sess01`: a leaked spec here would silently un-enforce gates for every later task.
struct EnvGuard {
    key: String,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var(&self.key, v),
            None => std::env::remove_var(&self.key),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GateCorpus {
    schema_version: u32,
    cases: Vec<GateCase>,
}

#[derive(Debug, Deserialize)]
struct GateCase {
    id: String,
    gate: String,
    label: String,
    kind: String,
    input: String,
}

/// The plan-admission half of the corpus is INJECT-01's, read from the same file rather than
/// duplicated: the promotion criterion should be measured on the cases the gate is already frozen
/// against, not on a second set that can drift from them.
#[derive(Debug, Deserialize)]
struct PlanCorpus {
    schema_version: u32,
    cases: Vec<PlanCase>,
}

#[derive(Debug, Deserialize)]
struct PlanCase {
    id: String,
    kind: String,
    goal: String,
    original_plan: Vec<ToolInvocation>,
    observations: Vec<ObservationDto>,
    candidate_plan: Vec<ToolInvocation>,
    #[serde(default)]
    expected_reason_contains: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ObservationDto {
    iteration: usize,
    tool: String,
    success: bool,
    output: String,
}

fn fixture_path(name: &str) -> Result<PathBuf, String> {
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name),
        Path::new("tests/golden_harness/fixtures").join(name),
        Path::new("fixtures").join(name),
    ];
    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| format!("{name} not found"))
}

fn load_gate_corpus() -> Result<GateCorpus, String> {
    let path = fixture_path("gate04_corpus.json")?;
    let corpus: GateCorpus =
        serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("parse {path:?}: {e}"))?;
    if corpus.schema_version != 1 {
        return Err(format!(
            "unsupported GATE-04 corpus schema_version {}",
            corpus.schema_version
        ));
    }
    Ok(corpus)
}

fn load_plan_corpus() -> Result<PlanCorpus, String> {
    let path = fixture_path("inject01_corpus.json")?;
    let corpus: PlanCorpus =
        serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("parse {path:?}: {e}"))?;
    if corpus.schema_version != 1 {
        return Err(format!(
            "unsupported INJECT-01 corpus schema_version {}",
            corpus.schema_version
        ));
    }
    Ok(corpus)
}

pub fn gate04_fixture_ready() -> Result<usize, String> {
    let gate_corpus = load_gate_corpus()?;
    let plan_corpus = load_plan_corpus()?;
    for gate in [PROMPT_GATE, PATH_GATE] {
        let cases: Vec<&GateCase> = gate_corpus
            .cases
            .iter()
            .filter(|case| case.gate == gate)
            .collect();
        for label in ["benign", "adversarial"] {
            let n = cases.iter().filter(|case| case.label == label).count();
            if n < 2 {
                return Err(format!(
                    "GATE-04 corpus needs ≥2 {label} cases for {gate}, found {n}"
                ));
            }
        }
    }
    let deny = plan_corpus
        .cases
        .iter()
        .filter(|case| case.kind == "deny")
        .count();
    if deny < 8 {
        return Err(format!(
            "GATE-04 needs ≥8 adversarial plan cases from INJECT-01, found {deny}"
        ));
    }
    Ok(gate_corpus.cases.len() + plan_corpus.cases.len())
}

fn label(token: &str) -> Result<CorpusLabel, String> {
    CorpusLabel::parse(token).ok_or_else(|| {
        format!("unknown corpus label {token:?} — a gate measurement needs benign|adversarial")
    })
}

/// Run one labelled hook case through the real gate and report `(denied, detail)`.
fn run_hook_case(case: &GateCase) -> Result<(bool, String), String> {
    match case.kind.as_str() {
        "prompt" => match enforce_user_prompt_submit(&case.input) {
            Ok(()) => Ok((false, "prompt admitted".to_string())),
            Err(reason) => Ok((true, reason)),
        },
        "path" => match pre_tool_use_path_check(Path::new(&case.input)) {
            HookDecision::Allow => Ok((false, "path admitted".to_string())),
            HookDecision::Deny(reason) => Ok((true, reason)),
        },
        other => Err(format!("case {}: unknown hook case kind {other:?}", case.id)),
    }
}

/// Run one INJECT-01 plan case through the real admission gate.
fn run_plan_case(case: &PlanCase) -> Result<(bool, String), String> {
    let mut trusted = case.goal.clone();
    trusted.push('\n');
    for step in &case.original_plan {
        trusted.push_str(&format!("{step:?}\n"));
    }
    let observations: Vec<ToolObservation> = case
        .observations
        .iter()
        .map(|dto| ToolObservation {
            iteration: dto.iteration,
            tool: dto.tool.clone(),
            success: dto.success,
            output: dto.output.clone(),
        })
        .collect();
    match admit_plan_against_observations(
        &trusted,
        &case.original_plan,
        &observations,
        &case.candidate_plan,
    ) {
        AdmitDecision::Allow { .. } => Ok((false, "plan admitted".to_string())),
        AdmitDecision::Deny { findings } => Ok((
            true,
            findings
                .iter()
                .map(|finding| finding.reason.clone())
                .collect::<Vec<_>>()
                .join("; "),
        )),
    }
}

fn plan_label(case: &PlanCase) -> Result<CorpusLabel, String> {
    match case.kind.as_str() {
        "deny" => Ok(CorpusLabel::Adversarial),
        "allow" => Ok(CorpusLabel::Benign),
        other => Err(format!(
            "case {}: unknown INJECT-01 kind {other:?} (deny|allow)",
            case.id
        )),
    }
}

fn summary_for(ledger: &GateLedger, gate: &str) -> Result<aether_core::GateSummary, String> {
    ledger
        .summary()
        .into_iter()
        .find(|summary| summary.gate == gate)
        .ok_or_else(|| format!("{gate} was never measured by this ledger"))
}

pub fn test_gate04_impl() -> Result<(), String> {
    let gate_corpus = load_gate_corpus()?;
    let plan_corpus = load_plan_corpus()?;
    let adversarial_inputs = gate_corpus
        .cases
        .iter()
        .filter(|case| case.label == "adversarial")
        .count()
        + plan_corpus
            .cases
            .iter()
            .filter(|case| case.kind == "deny")
            .count();

    // --- A. the spec is closed, and every rejection means "enforce everything" ----------------

    let spec = GateSpec::parse(&format!("{PATH_GATE}:log,{ADMISSION_GATE}:enforce"))
        .map_err(|e| e.to_string())?;
    if spec.mode(PATH_GATE) != GateMode::Log {
        return Err("an explicit `log` entry must put that gate in log mode".into());
    }
    if spec.mode(ADMISSION_GATE) != GateMode::Enforce {
        return Err("an explicit `enforce` entry must enforce".into());
    }
    if spec.mode(PROMPT_GATE) != GateMode::Enforce {
        return Err("a gate the spec never mentions must enforce".into());
    }
    if spec.gates_in_log() != vec![PATH_GATE.to_string()] {
        return Err(format!(
            "gates_in_log must name exactly the dark-launched gates, got {:?}",
            spec.gates_in_log()
        ));
    }
    if spec.rejected.is_some() {
        return Err("a valid spec must not report itself rejected".into());
    }
    for empty in ["", "   ", ",,"] {
        let spec = GateSpec::parse(empty).map_err(|e| format!("spec {empty:?}: {e}"))?;
        if !spec.gates_in_log().is_empty() {
            return Err(format!("an empty spec must dark-launch nothing, got {empty:?}"));
        }
    }
    match GateSpec::parse("hook.path_denyliste:log") {
        Err(GateSpecError::UnknownGate { gate, known, .. }) => {
            if gate != "hook.path_denyliste" {
                return Err(format!("the error must name the offending gate, got {gate:?}"));
            }
            if !known.contains(PATH_GATE) {
                return Err(format!(
                    "a typo is only fixable if the error lists the real gates, got {known:?}"
                ));
            }
        }
        other => return Err(format!("an unregistered gate name must be refused, got {other:?}")),
    }
    for bad in [
        PATH_GATE,
        "hook.path_denylist:monitor",
        "hook.path_denylist:",
        ":log",
        "hook.path_denylist:log:enforce",
    ] {
        match GateSpec::parse(bad) {
            Err(GateSpecError::MalformedEntry { .. }) => {}
            other => {
                return Err(format!(
                    "entry {bad:?} is malformed and must be refused as such, got {other:?}"
                ))
            }
        }
    }
    // One bad entry invalidates the whole spec: a partially applied safety configuration is worse
    // than either extreme, because the operator believes gate X is dark-launched while gate Y is not.
    match GateSpec::parse(&format!("{PATH_GATE}:log,hook.path_denyliste:log")) {
        Err(GateSpecError::UnknownGate { .. }) => {}
        other => return Err(format!("a mixed spec must be refused whole, got {other:?}")),
    }
    // Redaction is not a blocking gate, and "log instead of redact" is the leak itself.
    for mode in ["log", "enforce"] {
        match GateSpec::parse(&format!("hook.output_redaction:{mode}")) {
            Err(GateSpecError::NotDarkLaunchable { gate, reason }) => {
                if gate != "hook.output_redaction" {
                    return Err(format!("wrong gate named in the refusal: {gate:?}"));
                }
                if !reason.contains("leak") {
                    return Err(format!(
                        "the refusal must say why, not just no: {reason:?}"
                    ));
                }
            }
            other => {
                return Err(format!(
                    "hook.output_redaction must never be configurable, got {other:?}"
                ))
            }
        }
    }

    // --- B. enforce is the default: measure every gate on the frozen corpora ------------------

    std::env::remove_var(GATE_ENV_VAR);
    let spec = GateSpec::from_env();
    if spec.rejected.is_some() || !spec.gates_in_log().is_empty() {
        return Err(format!(
            "with no {GATE_ENV_VAR} every gate must enforce, got {spec:?}"
        ));
    }
    let _ = drain_gate_hits();

    let mut ledger = GateLedger::default();
    for case in &gate_corpus.cases {
        let (denied, detail) = run_hook_case(case)?;
        let expected = case.label == "adversarial";
        if denied != expected {
            return Err(format!(
                "case {} ({}) must {} in enforce mode, detail: {detail}",
                case.id,
                case.label,
                if expected { "deny" } else { "allow" }
            ));
        }
        ledger.record_decision(&case.gate, denied, &detail, label(&case.label)?);
    }
    for case in &plan_corpus.cases {
        let (denied, detail) = run_plan_case(case)?;
        let expected = case.kind == "deny";
        if denied != expected {
            return Err(format!(
                "plan case {} ({}) must {} in enforce mode, detail: {detail}",
                case.id,
                case.kind,
                if expected { "deny" } else { "allow" }
            ));
        }
        if let (true, Some(needle)) = (denied, case.expected_reason_contains.as_deref()) {
            if !detail.contains(needle) {
                return Err(format!(
                    "plan case {} denied for the wrong reason: {needle:?} not in {detail:?}",
                    case.id
                ));
            }
        }
        ledger.record_decision(ADMISSION_GATE, denied, &detail, plan_label(case)?);
    }

    // Enforce mode records nothing: a gate's allow path must cost nothing, and the sink must not fill
    // with every call that was fine.
    if !gate_hits().is_empty() {
        return Err(format!(
            "enforce mode must not write dark-launch telemetry, got {:?}",
            gate_hits()
        ));
    }

    // --- C. the promotion criterion, measured rather than asserted ----------------------------

    for (gate, observed, would_deny) in [
        (PROMPT_GATE, 4usize, 2usize),
        (PATH_GATE, 4, 2),
        (ADMISSION_GATE, plan_corpus.cases.len(), 8),
    ] {
        let summary = summary_for(&ledger, gate)?;
        if summary.observed != observed || summary.would_deny != would_deny {
            return Err(format!(
                "{gate}: expected {observed} observed / {would_deny} would-deny, got {summary:?} — \
                 a corpus that quietly stops exercising a gate has to fail loudly"
            ));
        }
        if summary.false_positives != 0 || summary.misses != 0 {
            return Err(format!("{gate} is not clean on the frozen corpus: {summary:?}"));
        }
        if !summary.ready_to_enforce() || ledger.ready_to_enforce(gate) != Some(true) {
            return Err(format!(
                "{gate} measured clean and must report ready to enforce: {summary:?}"
            ));
        }
    }
    let report = ledger.render_report();
    if !report.starts_with("[gate report: 3 gate(s) measured over ") {
        return Err(format!("the report must state its own bound:\n{report}"));
    }
    for gate in DARK_LAUNCHABLE_GATES {
        if !report.contains(&format!("- {gate}: observed")) {
            return Err(format!("the report is missing {gate}:\n{report}"));
        }
    }

    // --- D. log mode records instead of blocking ----------------------------------------------

    let all_log = DARK_LAUNCHABLE_GATES
        .iter()
        .map(|gate| format!("{gate}:log"))
        .collect::<Vec<_>>()
        .join(",");
    let spec = GateSpec::parse(&all_log).map_err(|e| e.to_string())?;
    if spec.gates_in_log().len() != DARK_LAUNCHABLE_GATES.len() {
        return Err(format!(
            "every blocking gate must be dark-launchable, got {:?}",
            spec.gates_in_log()
        ));
    }

    {
        let _guard = EnvGuard::set(GATE_ENV_VAR, &all_log);
        let _ = drain_gate_hits();
        for case in &gate_corpus.cases {
            let (denied, detail) = run_hook_case(case)?;
            if denied {
                return Err(format!(
                    "case {}: in log mode a gate records, it does not block ({detail})",
                    case.id
                ));
            }
        }
        for case in &plan_corpus.cases {
            let (denied, detail) = run_plan_case(case)?;
            if denied {
                return Err(format!(
                    "plan case {}: the admission gate must not refuse a plan while dark-launched ({detail})",
                    case.id
                ));
            }
        }

        let hits = drain_gate_hits();
        if hits.len() != adversarial_inputs {
            return Err(format!(
                "expected exactly one recorded would-deny per adversarial input \
                 ({adversarial_inputs}), got {}: {hits:?}",
                hits.len()
            ));
        }
        if hits.iter().any(|hit| !hit.would_deny) {
            return Err("log mode records would-be denials, not every call".into());
        }
        for gate in DARK_LAUNCHABLE_GATES {
            let mine: Vec<_> = hits.iter().filter(|hit| hit.gate == *gate).collect();
            if mine.is_empty() {
                return Err(format!("{gate} recorded nothing while dark-launched"));
            }
            if mine.iter().any(|hit| hit.detail.trim().len() < 20) {
                return Err(format!(
                    "{gate} recorded a detail too short to triage: {:?}",
                    mine.iter().map(|hit| hit.detail.clone()).collect::<Vec<_>>()
                ));
            }
        }
        let admission_hits = hits.iter().filter(|hit| hit.gate == ADMISSION_GATE).count();
        if admission_hits != 8 {
            return Err(format!(
                "all eight adversarial plan cases must be recorded by the admission gate, got {admission_hits}"
            ));
        }
    }

    // --- E. nothing leaks ---------------------------------------------------------------------

    if std::env::var(GATE_ENV_VAR).is_ok() {
        return Err(format!(
            "the guard must clear {GATE_ENV_VAR}: a leaked spec would silently un-enforce gates for \
             every task that runs afterwards"
        ));
    }
    let adversarial_case = gate_corpus
        .cases
        .iter()
        .find(|case| case.label == "adversarial" && case.kind == "path")
        .ok_or("corpus has no adversarial path case")?;
    let (denied, detail) = run_hook_case(adversarial_case)?;
    if !denied {
        return Err(format!(
            "case {}: the gate must enforce again once the dark launch is over ({detail})",
            adversarial_case.id
        ));
    }
    let _ = drain_gate_hits();

    // A configuration error must never be the thing that turns a gate off.
    {
        let _bad = EnvGuard::set(GATE_ENV_VAR, "hook.path_denylist:monitor");
        let spec = GateSpec::from_env();
        if spec.rejected.is_none() {
            return Err("a rejected spec must be reported, not silently ignored".into());
        }
        if !spec.gates_in_log().is_empty() {
            return Err(format!(
                "a rejected spec must dark-launch nothing, got {:?}",
                spec.gates_in_log()
            ));
        }
        let (denied, detail) = run_hook_case(adversarial_case)?;
        if !denied {
            return Err(format!(
                "an unparsable {GATE_ENV_VAR} must leave the gate enforcing ({detail})"
            ));
        }
    }
    if std::env::var(GATE_ENV_VAR).is_ok() {
        return Err("the bad-spec guard must also clear the environment".into());
    }

    // --- F. the instrument has to be able to fail ---------------------------------------------

    // An over-broad gate: one benign input denied. This is the false positive that gets safety work
    // reverted, and the ledger is what makes it visible before promotion instead of after.
    let mut over_broad = GateLedger::default();
    for entry in ledger.entries() {
        over_broad.record(entry.0.clone(), entry.1);
    }
    over_broad.record_decision(
        PATH_GATE,
        true,
        "over-broad rule matched a workspace-relative read",
        CorpusLabel::Benign,
    );
    let summary = summary_for(&over_broad, PATH_GATE)?;
    if summary.false_positives != 1 {
        return Err(format!(
            "a benign input denied must count as a false positive, got {summary:?}"
        ));
    }
    if summary.ready_to_enforce() || over_broad.ready_to_enforce(PATH_GATE) != Some(false) {
        return Err(format!(
            "a gate with a false positive must not report ready to enforce: {summary:?}"
        ));
    }
    if !over_broad.render_report().contains("NOT ready to enforce") {
        return Err(format!(
            "the report must say so in words:\n{}",
            over_broad.render_report()
        ));
    }

    // A too-narrow gate: an adversarial input waved through.
    let mut too_narrow = GateLedger::default();
    too_narrow.record_decision(
        PROMPT_GATE,
        false,
        "rule missed a paraphrase of the override phrase",
        CorpusLabel::Adversarial,
    );
    let summary = summary_for(&too_narrow, PROMPT_GATE)?;
    if summary.misses != 1 || summary.ready_to_enforce() {
        return Err(format!(
            "a missed adversarial input must block promotion, got {summary:?}"
        ));
    }

    // And a gate the corpus never touched has no evidence either way: `observed == 0` is not clean.
    let unmeasured = GateLedger::default();
    if unmeasured.ready_to_enforce(PATH_GATE) != None {
        return Err("an unmeasured gate must not report a verdict".into());
    }
    let mut one_case = GateLedger::default();
    one_case.record_decision(PATH_GATE, false, "one benign input", CorpusLabel::Benign);
    if one_case.ready_to_enforce(PATH_GATE) != Some(true) {
        return Err(format!(
            "the criterion is 'no counter-evidence', so one clean case yields a verdict (a thin \
             corpus is the corpus author's problem, not the ledger's) — got {:?}",
            one_case.ready_to_enforce(PATH_GATE)
        ));
    }
    let report = one_case.render_report();
    if !report.contains("never measured by this corpus") {
        return Err(format!(
            "the report must name the gates it has no evidence for:\n{report}"
        ));
    }
    for gate in [PROMPT_GATE, ADMISSION_GATE] {
        if !report.contains(gate) {
            return Err(format!("the report omits an unmeasured gate ({gate}):\n{report}"));
        }
    }

    Ok(())
}

// No `#[cfg(test)]` wrapper on purpose, and the same is true of every other env-mutating task in this
// harness (`sec01`, `ckpt01`, `fork01`, `loop04`, `sess01`): `cargo test` runs unit tests on parallel
// threads inside one process, and `AETHER_GATE_MODE` is process-wide. This task belongs to the harness
// binary, which runs tasks sequentially and is the only place its guard is actually safe.
