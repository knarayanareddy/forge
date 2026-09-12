//! CHECK-02 — the post-write verify shell must certify the *artifact*, not a plan-supplied string.
//!
//! CHECK-01 proved a write cannot be followed by `done` without a `verify_contains` and a lint.
//! Both of those were satisfiable without ever looking at the file that was written:
//! `python_lint` checks source supplied *in the plan*, so a plan could write broken Python, verify
//! its own bytes were on disk, lint an unrelated `def ok(): return 1` snippet, and report `done` —
//! every gate green, artifact broken. That is verification theater, and it is the failure mode the
//! review calls out in `docs/REVIEW_FABLE51_HARNESS.md` (finding P0-1).
//!
//! This task drives the production loop (`aether_daemon::task_runner::execute_structured_loop`,
//! the same entry point `run_task` uses) with plans that satisfy every pre-existing requirement and
//! still ship a broken artifact. For each one it asserts:
//!
//! 1. every step *before* `done` succeeds, and the pre-CHECK-02 gate would have cleared the run —
//!    so the case is a genuinely new catch, not something already blocked;
//! 2. the run is refused, and the refusal names the offending path (a remedy-bearing error, P1-9);
//! 3. the refused run leaves no residue: every partial write is in the undo journal and
//!    `undo_pending_writes` removes it.
//!
//! It also pins the two shapes that must keep passing — the historical `.txt` plan and a `.py`
//! plan that lints its own artifact — so tightening the gate cannot silently break LOOP-01/02/04,
//! PERM-02, CKPT-01, or GATE-01/02/03.
//!
//! Deterministic: no model, no network.

use aether_core::{
    is_lintable_artifact, LoopConfig, LoopRunResult, ToolInvocation, ToolObservation,
    DEFAULT_MAX_LOOP_TOKENS,
};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use aether_permissions::undo_pending_writes;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

/// Python that does not compile. `py_compile` rejects it at `def broken(:`.
const BROKEN_PY: &str = "def broken(:\n    return 1\n";
/// Python that compiles. Used both as a benign artifact and as the *unrelated* snippet a theatrical
/// plan lints instead of the file it wrote.
const VALID_PY: &str = "def ok():\n    return 1\n";
/// Marker text used by `verify_contains` so the byte-landing requirement is genuinely met.
const VALID_TXT: &str = "check02 seeded notes\n";

/// Minimum number of plans that must be refused, and minimum number of those that the pre-CHECK-02
/// gate would have waved through. Both are asserted at the end; the review's claim is "at least six
/// broken plans that satisfy the old gate".
const MIN_BLOCKED: usize = 6;
const MIN_NEW_CATCHES: usize = 4;

#[derive(Debug, Clone)]
enum Expect {
    /// The run must be refused, and the refusal must contain `names` so the planner (or a human
    /// reading the log) knows exactly which artifact to fix.
    Blocked { names: &'static str },
    /// The run must reach `done`. These guard against over-tightening.
    Completed,
}

struct Case {
    id: &'static str,
    /// Files present in the workspace before the loop runs.
    seeded: Vec<(&'static str, &'static str)>,
    plan: Vec<ToolInvocation>,
    expect: Expect,
    /// One line on why this case exists, surfaced in failure messages.
    rationale: &'static str,
}

struct Run {
    /// Kept alive so `workspace` stays valid for the assertions that follow.
    _tmp: TempDir,
    workspace: PathBuf,
    session_id: String,
    outcome: Result<LoopRunResult, String>,
}

/// Grant the workspace exactly the way the daemon does when a folder is selected
/// (`server::select_workspace` inserts one row per capability in `["read", "write"]`).
///
/// Both rows matter here. `fs_write` only needs `write`, but `python_lint_file` goes through
/// `PermissionManager::check_file_access(.., "read")`, which matches `permission_type` exactly — so
/// a write-only grant makes every on-disk lint fail with `Read denied for …` before the verify
/// shell is ever reached. That would test the permission layer instead of the gate, and it only
/// shows up on the cases that seed a file the plan never wrote (the write path grants nothing of
/// its own: "execution must never create its own grant").
fn seed_session_and_grant(
    db: &Database,
    session_id: &str,
    workspace: &std::path::Path,
) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'CHECK-02', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    for capability in ["read", "write"] {
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![session_id, workspace.to_string_lossy().to_string(), capability],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Run one plan against the production loop in a fresh workspace. `suffix` keeps sessions distinct
/// when the same case is run twice (once with `done`, once without).
fn run_plan(
    db: &Database,
    case_id: &str,
    suffix: &str,
    seeded: &[(&str, &str)],
    plan: Vec<ToolInvocation>,
) -> Result<Run, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let session_id = format!("sess-check02-{case_id}-{suffix}");
    seed_session_and_grant(db, &session_id, &workspace)?;

    for &(path, content) in seeded {
        let target = workspace.join(path);
        std::fs::write(&target, content)
            .map_err(|e| format!("seed {} failed: {}", path, e))?;
    }

    let mut config = LoopConfig {
        max_iterations: 12,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_id.clone(),
        workspace: workspace.clone(),
    };

    let conn = db.conn();
    let (result, _events) = execute_structured_loop(
        &conn,
        &mut config,
        plan,
        None,
        &HashMap::new(),
        None,
        "check02-case",
    );

    Ok(Run {
        _tmp: tmp,
        workspace,
        session_id,
        outcome: result.map_err(|e| e.to_string()),
    })
}

/// The gate exactly as it stood before CHECK-02: a write needs *some* successful `verify_contains`
/// and *some* successful lint, with no relationship to what was actually written.
fn legacy_gate_clears(observations: &[ToolObservation]) -> bool {
    let wrote = observations
        .iter()
        .any(|o| o.tool == "fs_write" && o.success);
    if !wrote {
        return true;
    }
    let verified = observations
        .iter()
        .any(|o| o.tool == "verify_contains" && o.success);
    let linted = observations.iter().any(|o| o.tool == "python_lint" && o.success);
    verified && linted
}

/// Every path a plan writes, in plan order — the artifacts the run is responsible for.
fn written_paths(plan: &[ToolInvocation]) -> Vec<String> {
    plan.iter()
        .filter_map(|step| match step {
            ToolInvocation::FsWrite { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect()
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            id: "unrelated-lint-source",
            seeded: vec![],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "report.py".into(),
                    content: BROKEN_PY.into(),
                },
                ToolInvocation::VerifyContains {
                    path: "report.py".into(),
                    text: "def broken".into(),
                },
                // Lints a string that was never written. This is the theater case: every old
                // requirement is met, and the artifact does not compile.
                ToolInvocation::PythonLint {
                    source: VALID_PY.into(),
                },
                ToolInvocation::Done,
            ],
            expect: Expect::Blocked { names: "report.py" },
            rationale: "python_lint on plan-supplied source cannot certify a written file",
        },
        Case {
            id: "lints-a-different-file",
            seeded: vec![("other.py", VALID_PY)],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "report.py".into(),
                    content: BROKEN_PY.into(),
                },
                ToolInvocation::VerifyContains {
                    path: "report.py".into(),
                    text: "def broken".into(),
                },
                ToolInvocation::PythonLint {
                    source: VALID_PY.into(),
                },
                // A real on-disk lint — of the wrong file.
                ToolInvocation::PythonLintFile {
                    path: "other.py".into(),
                },
                ToolInvocation::Done,
            ],
            expect: Expect::Blocked { names: "report.py" },
            rationale: "linting a different artifact must not stand in for linting this one",
        },
        Case {
            id: "two-writes-one-lint",
            seeded: vec![],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "a.py".into(),
                    content: VALID_PY.into(),
                },
                ToolInvocation::FsWrite {
                    path: "b.py".into(),
                    content: BROKEN_PY.into(),
                },
                ToolInvocation::VerifyContains {
                    path: "a.py".into(),
                    text: "def ok".into(),
                },
                ToolInvocation::VerifyContains {
                    path: "b.py".into(),
                    text: "def broken".into(),
                },
                ToolInvocation::PythonLint {
                    source: VALID_PY.into(),
                },
                ToolInvocation::PythonLintFile { path: "a.py".into() },
                ToolInvocation::Done,
            ],
            expect: Expect::Blocked { names: "b.py" },
            rationale: "the rule is per-path: one linted artifact cannot cover an unlinted sibling",
        },
        Case {
            id: "verify-on-other-path",
            seeded: vec![("notes.txt", VALID_TXT)],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "report.py".into(),
                    content: BROKEN_PY.into(),
                },
                // Verifies a file the plan did not write.
                ToolInvocation::VerifyContains {
                    path: "notes.txt".into(),
                    text: "seeded".into(),
                },
                ToolInvocation::PythonLint {
                    source: VALID_PY.into(),
                },
                ToolInvocation::Done,
            ],
            expect: Expect::Blocked { names: "report.py" },
            rationale: "an unrelated verify_contains must not clear an unverified write",
        },
        Case {
            id: "lint-file-catches-broken-artifact",
            seeded: vec![],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "report.py".into(),
                    content: BROKEN_PY.into(),
                },
                ToolInvocation::VerifyContains {
                    path: "report.py".into(),
                    text: "def broken".into(),
                },
                // The right tool, pointed at the right file — and it fails, which is the point:
                // this is the first step in the suite that can actually see the breakage.
                ToolInvocation::PythonLintFile {
                    path: "report.py".into(),
                },
                ToolInvocation::Done,
            ],
            expect: Expect::Blocked { names: "report.py" },
            rationale: "python_lint_file must fail the run when the artifact does not compile",
        },
        Case {
            id: "second-artifact-also-linted",
            seeded: vec![],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "a.py".into(),
                    content: VALID_PY.into(),
                },
                ToolInvocation::FsWrite {
                    path: "b.py".into(),
                    content: BROKEN_PY.into(),
                },
                ToolInvocation::VerifyContains {
                    path: "a.py".into(),
                    text: "def ok".into(),
                },
                ToolInvocation::VerifyContains {
                    path: "b.py".into(),
                    text: "def broken".into(),
                },
                ToolInvocation::PythonLintFile { path: "a.py".into() },
                ToolInvocation::PythonLintFile { path: "b.py".into() },
                ToolInvocation::Done,
            ],
            expect: Expect::Blocked { names: "b.py" },
            rationale: "linting every artifact must still surface the one that is broken",
        },
        Case {
            id: "no-lint-at-all",
            seeded: vec![],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "report.py".into(),
                    content: BROKEN_PY.into(),
                },
                ToolInvocation::VerifyContains {
                    path: "report.py".into(),
                    text: "def broken".into(),
                },
                ToolInvocation::Done,
            ],
            expect: Expect::Blocked { names: "python_lint" },
            rationale: "the pre-existing lint requirement still fires underneath the new one",
        },
        // --- Must keep passing: these pin backward compatibility. ---
        Case {
            id: "txt-plan-unchanged",
            seeded: vec![],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "notes.txt".into(),
                    content: VALID_TXT.into(),
                },
                ToolInvocation::VerifyContains {
                    path: "notes.txt".into(),
                    text: "seeded".into(),
                },
                ToolInvocation::PythonLint {
                    source: VALID_PY.into(),
                },
                ToolInvocation::Done,
            ],
            expect: Expect::Completed,
            rationale: "the historical LOOP-01/02/04 shape writes no .py, so it must not regress",
        },
        Case {
            id: "py-plan-lints-its-own-artifact",
            seeded: vec![],
            plan: vec![
                ToolInvocation::FsWrite {
                    path: "report.py".into(),
                    content: VALID_PY.into(),
                },
                ToolInvocation::VerifyContains {
                    path: "report.py".into(),
                    text: "def ok".into(),
                },
                ToolInvocation::PythonLintFile {
                    path: "report.py".into(),
                },
                ToolInvocation::Done,
            ],
            expect: Expect::Completed,
            rationale: "the intended new shape must complete",
        },
    ]
}

pub fn test_check02_impl(db: &Database) -> Result<(), String> {
    // The classifier the gate keys on: path-scoped and case-insensitive, never a substring match
    // (so `spy` and `report.pyi` are not lintable artifacts).
    for (path, expected) in [
        ("report.py", true),
        ("REPORT.PY", true),
        ("notes.txt", false),
        ("report.pyi", false),
        ("spy", false),
        ("no_extension", false),
        ("", false),
    ] {
        if is_lintable_artifact(path) != expected {
            return Err(format!(
                "is_lintable_artifact({:?}) should be {} — the per-path rule would mis-scope",
                path, expected
            ));
        }
    }

    let mut blocked = 0usize;
    let mut new_catches = 0usize;
    let mut completed = 0usize;

    for case in cases() {
        // Step 1: prove what the run does *without* `done`. If this fails, the case is not testing
        // the gate at all — it is testing a broken step.
        let without_done: Vec<ToolInvocation> = case
            .plan
            .iter()
            .cloned()
            .filter(|step| !matches!(step, ToolInvocation::Done))
            .collect();
        let pre = run_plan(db, case.id, "pre", &case.seeded, without_done)?;
        let legacy_clears = match &pre.outcome {
            Ok(run) => legacy_gate_clears(&run.observations),
            Err(e) => {
                // Only the two cases whose lint step is meant to fail are allowed to error here.
                if !matches!(
                    case.expect,
                    Expect::Blocked { .. }
                ) || !["lint-file-catches-broken-artifact", "second-artifact-also-linted"]
                    .contains(&case.id)
                {
                    return Err(format!(
                        "case {} ({}) failed before `done`: {} — this case cannot prove anything \
                         about the gate",
                        case.id, case.rationale, e
                    ));
                }
                false
            }
        };

        // Step 2: the real run, with `done`.
        let run = run_plan(db, case.id, "full", &case.seeded, case.plan.clone())?;
        match (&case.expect, &run.outcome) {
            (Expect::Blocked { names }, Err(message)) => {
                if !message.contains(*names) {
                    return Err(format!(
                        "case {} ({}): refusal must name {:?} so it is actionable, got: {}",
                        case.id, case.rationale, names, message
                    ));
                }
                blocked += 1;
                if legacy_clears {
                    new_catches += 1;
                }
            }
            (Expect::Blocked { .. }, Ok(run_ok)) => {
                return Err(format!(
                    "case {} ({}) must be refused but completed: done={} summary={:?}",
                    case.id, case.rationale, run_ok.done, run_ok.summary
                ));
            }
            (Expect::Completed, Ok(run_ok)) => {
                if !run_ok.done {
                    return Err(format!(
                        "case {} ({}) did not reach done",
                        case.id, case.rationale
                    ));
                }
                completed += 1;
            }
            (Expect::Completed, Err(message)) => {
                return Err(format!(
                    "case {} ({}) must still complete — the tightened gate regressed it: {}",
                    case.id, case.rationale, message
                ));
            }
        }

        // Step 3: a refused run leaves nothing behind that undo cannot reach. Every artifact the
        // plan wrote must be in the undo journal and gone from disk after `undo_pending_writes`.
        if matches!(case.expect, Expect::Blocked { .. }) {
            let written = written_paths(&case.plan);
            if written.is_empty() {
                return Err(format!("case {} asserts a block but writes nothing", case.id));
            }
            let report = {
                let conn = db.conn();
                undo_pending_writes(&conn, &run.session_id).map_err(|e| e.to_string())?
            };
            for path in &written {
                if !report.reverted.iter().any(|p| p.ends_with(path.as_str())) {
                    return Err(format!(
                        "case {}: {} was written but is not in the undo journal (reverted: {:?}, \
                         not_undone: {:?}) — a refused run left unrecoverable residue",
                        case.id, path, report.reverted, report.not_undone
                    ));
                }
                if run.workspace.join(path).exists() {
                    return Err(format!(
                        "case {}: {} still on disk after undo_pending_writes",
                        case.id, path
                    ));
                }
            }
        }
    }

    if blocked < MIN_BLOCKED {
        return Err(format!(
            "expected at least {} refused plans, got {}",
            MIN_BLOCKED, blocked
        ));
    }
    if new_catches < MIN_NEW_CATCHES {
        return Err(format!(
            "expected at least {} plans the pre-CHECK-02 gate would have cleared, got {} — the \
             suite would no longer be proving the new rule catches anything",
            MIN_NEW_CATCHES, new_catches
        ));
    }
    if completed < 2 {
        return Err(format!(
            "expected both backward-compatibility controls to complete, got {}",
            completed
        ));
    }

    Ok(())
}
