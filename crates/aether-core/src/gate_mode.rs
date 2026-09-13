//! Dark launch for gates (P2-14 / GATE-04).
//!
//! Forge grows gates continuously — hook denylists, plan-admission correlation, risk scoring, the
//! post-write verify rule, skill trust. Until now the only way to measure a new gate's false-positive
//! rate was to enforce it and watch the scoreboard drop, which is how good safety work gets reverted:
//! a gate that is right 99% of the time looks identical to a broken one when the only instrument is
//! "tasks went red".
//!
//! This module gives a *blocking* gate two modes. `enforce` denies for real and is the default. `log`
//! records what the gate would have denied and lets the call through, so the gate can run against real
//! traffic — or a frozen corpus — and be judged before it is allowed to break anything.
//!
//! Two rules keep that from becoming a way to turn safety off:
//!
//! 1. **Fail-safe configuration.** An absent, empty or unparsable [`GATE_ENV_VAR`] means every gate
//!    enforces. A configuration error is never the thing that disables a gate; it is printed and
//!    ignored.
//! 2. **A closed gate list.** Only names in [`DARK_LAUNCHABLE_GATES`] can be set to `log`, and
//!    [`NEVER_DARK_LAUNCHABLE`] names the ones that must not be along with the reason. A typo fails
//!    loudly instead of silently dark-launching nothing — and output redaction can never be "logged
//!    instead of applied", because logging instead of redacting *is* the leak the rule prevents.
//!
//! Measurement is the point, so a hit is never just a counter. [`GateLedger`] pairs each observation
//! with what the input was already known to be, which is what makes [`GateSummary`] able to report
//! false positives (would-deny on a benign input) and misses (allow on an adversarial one) per gate.
//! [`GateSummary::ready_to_enforce`] is the promotion criterion: a gate goes live when its frozen
//! corpus is clean, not when someone feels confident about it.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};
use thiserror::Error;

/// Per-gate mode spec: `<gate>:log|enforce`, comma separated. Example:
/// `AETHER_GATE_MODE=inject.plan_admission:log,hook.path_denylist:enforce`.
pub const GATE_ENV_VAR: &str = "AETHER_GATE_MODE";

/// Blocking gates that may be dark-launched. A closed list on purpose — see the module docs.
pub const DARK_LAUNCHABLE_GATES: &[&str] = &[
    "hook.prompt_denylist",
    "hook.path_denylist",
    "inject.plan_admission",
];

/// Gates that must never be dark-launchable, with the reason an operator should see. Checked before
/// the allowlist, so the answer to `hook.output_redaction:log` is *why*, not "unknown gate".
pub const NEVER_DARK_LAUNCHABLE: &[(&str, &str)] = &[(
    "hook.output_redaction",
    "redaction is not a blocking gate: logging instead of redacting puts the secret straight into the \
     observation, which is the leak the rule exists to prevent",
)];

/// How a blocking gate treats a denial it would otherwise make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateMode {
    /// Deny for real. The default, and what any configuration error falls back to.
    Enforce,
    /// Record the would-be denial and let the call through.
    Log,
}

impl GateMode {
    /// Parse one `log` / `enforce` token. Case-insensitive; anything else is a configuration error.
    pub fn parse(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "log" => Some(Self::Log),
            "enforce" => Some(Self::Enforce),
            _ => None,
        }
    }

    pub fn is_log(self) -> bool {
        matches!(self, Self::Log)
    }
}

/// What an input was known to be *before* the gate saw it.
///
/// Without this label a gate's numbers are uninterpretable: "denied 40 times" says nothing until you
/// know how many of the 40 were benign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorpusLabel {
    Benign,
    Adversarial,
}

impl CorpusLabel {
    pub fn parse(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "benign" => Some(Self::Benign),
            "adversarial" => Some(Self::Adversarial),
            _ => None,
        }
    }
}

/// One gate decision, recorded rather than acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateHit {
    /// Gate name; one of [`DARK_LAUNCHABLE_GATES`] in practice, but not enforced here so a ledger can
    /// also carry measurements for a gate that is not yet wired.
    pub gate: String,
    /// Whether the gate would have denied.
    pub would_deny: bool,
    /// What the gate matched. An audit record that says only "denied" cannot be triaged, so this keeps
    /// the full internal detail — it is an audit channel, not a user-facing one (`P1-4` /
    /// `ErrorDetailLevel` governs what a *caller* is told).
    pub detail: String,
}

impl GateHit {
    /// A gate that would have denied.
    pub fn denied(gate: &str, detail: &str) -> Self {
        Self {
            gate: gate.to_string(),
            would_deny: true,
            detail: detail.to_string(),
        }
    }

    /// A gate that let the call through — the only kind recorded while measuring, since enforce mode
    /// records nothing.
    pub fn allowed(gate: &str, detail: &str) -> Self {
        Self {
            gate: gate.to_string(),
            would_deny: false,
            detail: detail.to_string(),
        }
    }
}

/// Why a [`GATE_ENV_VAR`] spec was refused. Every variant means "everything enforces".
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GateSpecError {
    #[error("gate entry {entry:?} must look like `<gate>:log` or `<gate>:enforce`")]
    MalformedEntry { entry: String },
    #[error("unknown gate {gate:?} in {var} — blocking gates that can be dark-launched: {known}")]
    UnknownGate {
        gate: String,
        var: String,
        known: String,
    },
    #[error("gate {gate:?} must never be dark-launched: {reason}")]
    NotDarkLaunchable { gate: String, reason: String },
}

/// A parsed, validated per-gate mode spec.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateSpec {
    modes: BTreeMap<String, GateMode>,
    /// Set when the environment carried a spec this module refused, so a caller can distinguish "no
    /// configuration" from "configuration rejected — enforcing everything and saying so".
    pub rejected: Option<String>,
}

impl GateSpec {
    /// Parse `<gate>:log|enforce` pairs. An empty spec is valid and means "everything enforces".
    ///
    /// Rejections are all-or-nothing: one bad entry invalidates the whole spec, because a partially
    /// applied safety configuration is worse than either extreme — the operator believes gate X is
    /// dark-launched while gate Y silently never was.
    pub fn parse(spec: &str) -> Result<Self, GateSpecError> {
        let mut modes = BTreeMap::new();
        for raw_entry in spec.split(',') {
            let entry = raw_entry.trim();
            if entry.is_empty() {
                continue;
            }
            let (gate, token) = entry.split_once(':').ok_or_else(|| {
                GateSpecError::MalformedEntry {
                    entry: entry.to_string(),
                }
            })?;
            let gate = gate.trim();
            if gate.is_empty() {
                // ":log" is a malformed entry, not an unknown gate: naming the empty string as a gate
                // would send an operator looking for a gate called "".
                return Err(GateSpecError::MalformedEntry {
                    entry: entry.to_string(),
                });
            }
            let mode = GateMode::parse(token).ok_or_else(|| GateSpecError::MalformedEntry {
                entry: entry.to_string(),
            })?;
            if let Some((_, reason)) = NEVER_DARK_LAUNCHABLE
                .iter()
                .find(|(name, _)| *name == gate)
            {
                return Err(GateSpecError::NotDarkLaunchable {
                    gate: gate.to_string(),
                    reason: (*reason).to_string(),
                });
            }
            if !DARK_LAUNCHABLE_GATES.contains(&gate) {
                return Err(GateSpecError::UnknownGate {
                    gate: gate.to_string(),
                    var: GATE_ENV_VAR.to_string(),
                    known: DARK_LAUNCHABLE_GATES.join(", "),
                });
            }
            modes.insert(gate.to_string(), mode);
        }
        Ok(Self {
            modes,
            rejected: None,
        })
    }

    /// Read [`GATE_ENV_VAR`]. Absent or empty is fine and means everything enforces. A spec this
    /// module refuses comes back as `Ok` with [`GateSpec::rejected`] set — the *behaviour* has to be
    /// fail-safe either way, and the caller gets the reason to be noisy about.
    pub fn from_env() -> Self {
        let raw = match std::env::var(GATE_ENV_VAR) {
            Err(_) => return Self::default(),
            Ok(raw) => raw,
        };
        if raw.trim().is_empty() {
            return Self::default();
        }
        match Self::parse(&raw) {
            Ok(spec) => spec,
            Err(e) => {
                let rejected = e.to_string();
                eprintln!(
                    "[gate-mode] ignoring {GATE_ENV_VAR}={raw:?}: {rejected} — every gate enforces"
                );
                Self {
                    modes: BTreeMap::new(),
                    rejected: Some(rejected),
                }
            }
        }
    }

    /// The mode for one gate. Anything not explicitly set to `log` enforces.
    pub fn mode(&self, gate: &str) -> GateMode {
        self.modes.get(gate).copied().unwrap_or(GateMode::Enforce)
    }

    pub fn is_log(&self, gate: &str) -> bool {
        self.mode(gate).is_log()
    }

    /// Every gate this spec dark-launches, in name order.
    pub fn gates_in_log(&self) -> Vec<String> {
        self.modes
            .iter()
            .filter(|(_, mode)| mode.is_log())
            .map(|(gate, _)| gate.clone())
            .collect()
    }
}

/// Process-wide dark-launch telemetry.
///
/// A gate deep inside `hooks` or `inject` has no way to hand a hit to a caller-supplied ledger
/// without changing a signature every caller depends on, so the would-be denials land here and are
/// read out by whoever is measuring. [`drain_gate_hits`] is the flush point for an audit writer:
/// `aether-permissions` owns the hash-chained `audit_log` insert, and this module deliberately does
/// not reimplement that format.
static GATE_HITS: Mutex<Vec<GateHit>> = Mutex::new(Vec::new());

fn hit_sink() -> MutexGuard<'static, Vec<GateHit>> {
    // A poisoned sink must not take the gates down with it: the hits are telemetry, and losing them
    // is better than panicking inside a deny path.
    GATE_HITS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Record a hit in the process-wide sink.
pub fn record_gate_hit(hit: GateHit) {
    hit_sink().push(hit);
}

/// Everything recorded so far, without clearing the sink.
pub fn gate_hits() -> Vec<GateHit> {
    hit_sink().clone()
}

/// Take the recorded hits, leaving the sink empty.
pub fn drain_gate_hits() -> Vec<GateHit> {
    std::mem::take(&mut *hit_sink())
}

/// The dark-launch seam, called at a gate's deny site.
///
/// `None` means *enforce*: the caller returns its denial unchanged, and nothing is recorded — so a
/// gate's allow path costs nothing and the sink is not full of every call that was fine. `Some(hit)`
/// means the gate is in `log` mode: the would-be denial has been recorded and the caller must allow.
pub fn moderate_denial(gate: &str, detail: &str) -> Option<GateHit> {
    let spec = GateSpec::from_env();
    if !spec.is_log(gate) {
        return None;
    }
    let hit = GateHit::denied(gate, detail);
    record_gate_hit(hit.clone());
    Some(hit)
}

/// Per-gate numbers over a labelled corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateSummary {
    pub gate: String,
    /// Inputs the corpus actually put through this gate.
    pub observed: usize,
    pub would_deny: usize,
    /// Would-deny on an input known to be benign: the cost of enforcing.
    pub false_positives: usize,
    /// Allowed an input known to be adversarial: the cost of *not* enforcing.
    pub misses: usize,
}

impl GateSummary {
    /// The promotion criterion. A gate may move from `log` to `enforce` when its frozen corpus is
    /// clean: nothing benign would have been denied, and nothing adversarial would have got through.
    ///
    /// `observed == 0` is **not** clean. It means the corpus never exercised the gate at all, and
    /// promoting on that evidence is exactly the guesswork dark launch exists to replace.
    pub fn ready_to_enforce(&self) -> bool {
        self.observed > 0 && self.false_positives == 0 && self.misses == 0
    }
}

/// A measurement run: gate decisions paired with what each input was known to be.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateLedger {
    entries: Vec<(GateHit, CorpusLabel)>,
}

impl GateLedger {
    pub fn record(&mut self, hit: GateHit, label: CorpusLabel) {
        self.entries.push((hit, label));
    }

    /// Record what a gate decided about one labelled input.
    pub fn record_decision(&mut self, gate: &str, denied: bool, detail: &str, label: CorpusLabel) {
        let hit = if denied {
            GateHit::denied(gate, detail)
        } else {
            GateHit::allowed(gate, detail)
        };
        self.record(hit, label);
    }

    pub fn entries(&self) -> &[(GateHit, CorpusLabel)] {
        &self.entries
    }

    pub fn hits_for(&self, gate: &str) -> Vec<&GateHit> {
        self.entries
            .iter()
            .filter(|entry| entry.0.gate == gate)
            .map(|entry| &entry.0)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Per-gate numbers, in gate-name order.
    pub fn summary(&self) -> Vec<GateSummary> {
        let mut gates: Vec<String> = Vec::new();
        for entry in &self.entries {
            if !gates.iter().any(|known| *known == entry.0.gate) {
                gates.push(entry.0.gate.clone());
            }
        }
        gates.sort();

        let mut summaries = Vec::new();
        for gate in gates {
            let mut observed = 0usize;
            let mut would_deny = 0usize;
            let mut false_positives = 0usize;
            let mut misses = 0usize;
            for (hit, label) in self.entries.iter().filter(|entry| entry.0.gate == gate) {
                observed += 1;
                if hit.would_deny {
                    would_deny += 1;
                    if *label == CorpusLabel::Benign {
                        false_positives += 1;
                    }
                } else if *label == CorpusLabel::Adversarial {
                    misses += 1;
                }
            }
            summaries.push(GateSummary {
                gate,
                observed,
                would_deny,
                false_positives,
                misses,
            });
        }
        summaries
    }

    /// The promotion verdict for one gate, or `None` if the ledger never measured it.
    pub fn ready_to_enforce(&self, gate: &str) -> Option<bool> {
        self.summary()
            .into_iter()
            .find(|summary| summary.gate == gate)
            .map(|summary| summary.ready_to_enforce())
    }

    /// The report an operator reads before promoting a gate. States its own bound, because a summary
    /// that silently drops a gate is how a false negative survives to `enforce` (P1-6).
    pub fn render_report(&self) -> String {
        let summaries = self.summary();
        let mut out = format!(
            "[gate report: {} gate(s) measured over {} labelled input(s)]\n",
            summaries.len(),
            self.entries.len()
        );
        for summary in &summaries {
            out.push_str(&format!(
                "- {}: observed {} | would_deny {} | false_positives {} | misses {} | {}\n",
                summary.gate,
                summary.observed,
                summary.would_deny,
                summary.false_positives,
                summary.misses,
                if summary.ready_to_enforce() {
                    "ready to enforce"
                } else {
                    "NOT ready to enforce"
                }
            ));
        }
        let unmeasured: Vec<&str> = DARK_LAUNCHABLE_GATES
            .iter()
            .filter(|gate| !summaries.iter().any(|s| s.gate == **gate))
            .copied()
            .collect();
        if !unmeasured.is_empty() {
            out.push_str(&format!(
                "- never measured by this corpus ({} of {} gates): {} — no promotion evidence either way\n",
                unmeasured.len(),
                DARK_LAUNCHABLE_GATES.len(),
                unmeasured.join(", ")
            ));
        }
        out
    }
}
