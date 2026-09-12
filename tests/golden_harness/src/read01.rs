//! READ-01 — every bounded read surface must report its bound, and a cut must be recoverable.
//!
//! Three surfaces in forge cut content without saying so (finding P1-6):
//!
//! * `fs_read` returned `content.chars().take(500)` — no marker, and no way to ask for more, so a
//!   goal whose answer sits at byte 10 000 of a 12 000-char file reads as "the answer is not in
//!   this file";
//! * `enrich_prompt_with_memory` cut a memory line mid-string and `break`ed when its 6 000-char
//!   budget ran out, so later hits vanished without a trace;
//! * the subagent preview kept 200 chars per file and presented them as the file.
//!
//! A model that cannot distinguish *absent* from *truncated* confidently reports absence — which is
//! why the reference prompt makes every bounded surface state its bound and offer the next page.
//!
//! This task pins the fixed contract on all three, plus the two properties that make a bound
//! survivable rather than merely honest: truncation is from the **middle** (the tail of a source
//! file carries `return`/`main`), and an explicit `offset`/`limit` window **reconstructs the file
//! exactly** when paged. Deterministic: no model, no network.

use aether_core::{
    classify_denial, run_subagent_read_task, render_read_window, LoopConfig, ToolError,
    ToolInvocation, DEFAULT_MAX_LOOP_TOKENS, FS_READ_MAX_CHARS,
};
use aether_core::DenialCategory;
use aether_daemon::task_runner::{
    enrich_prompt_with_memory, execute_structured_loop, RetrievedMemory, MAX_MEMORY_CONTEXT_CHARS,
};
use aether_db::Database;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

/// The answer the goal is looking for. Placed past the default read window on purpose: if it were
/// inside the first 500 chars, every assertion below would pass even with the old head-only cut.
const NEEDLE: &str = "READ-01-NEEDLE answer=42";
const HEAD_MARKER: &str = "READ-01-HEAD-MARKER";
const TAIL_MARKER: &str = "READ-01-TAIL-MARKER";
const BIG_FILE: &str = "big.txt";
/// Page size used by the reconstruction walk. Deliberately the default window so the walk exercises
/// the same budget the loop would apply on its own.
const PAGE: usize = 500;

/// ~12 000 ASCII characters with [`NEEDLE`] at ~10 000. ASCII on purpose: byte offsets and
/// character offsets coincide, so the fixture's arithmetic is checkable by eye.
fn big_file() -> String {
    let mut out = format!("{HEAD_MARKER} first line of the file\n");
    let mut i = 0usize;
    while out.chars().count() < 10_000 {
        out.push_str(&format!("line {i}: filler filler filler filler filler\n"));
        i += 1;
    }
    out.push_str(&format!("{NEEDLE}\n"));
    while out.chars().count() < 12_000 {
        out.push_str(&format!("tail {i}: filler filler filler filler\n"));
        i += 1;
    }
    out.push_str(&format!("{TAIL_MARKER} last line of the file\n"));
    out
}

struct Run {
    /// Kept alive so `workspace` stays valid for the assertions that follow.
    _tmp: TempDir,
    #[allow(dead_code)]
    workspace: PathBuf,
    outcome: Result<aether_core::LoopRunResult, String>,
}

/// Run a plan through the production loop in a fresh workspace. `grant_read` false reproduces the
/// write-only session that made CHECK-02's original fixture test the permission layer by accident.
fn run_plan(
    db: &Database,
    case_id: &str,
    grant_read: bool,
    plan: Vec<ToolInvocation>,
) -> Result<Run, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let session_id = format!("sess-read01-{case_id}");
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'READ-01', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    let mut capabilities = vec!["write"];
    if grant_read {
        capabilities.push("read");
    }
    for capability in capabilities {
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![session_id, workspace.to_string_lossy().to_string(), capability],
        )
        .map_err(|e| e.to_string())?;
    }

    std::fs::write(workspace.join(BIG_FILE), big_file())
        .map_err(|e| format!("seeding {BIG_FILE} failed: {}", e))?;

    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id,
        workspace: workspace.clone(),
    };
    let (result, _events) = execute_structured_loop(
        &conn,
        &mut config,
        plan,
        None,
        &HashMap::new(),
        None,
        "read01-case",
    );
    Ok(Run {
        _tmp: tmp,
        workspace,
        outcome: result.map_err(|e| e.to_string()),
    })
}

fn read_observation(run: &Run) -> Result<String, String> {
    let result = run
        .outcome
        .as_ref()
        .map_err(|e| format!("expected the read plan to run, got: {e}"))?;
    result
        .observations
        .iter()
        .find(|o| o.tool == "fs_read")
        .map(|o| o.output.clone())
        .ok_or_else(|| "no fs_read observation was recorded".to_string())
}

pub fn test_read01_impl(db: &Database) -> Result<(), String> {
    let content = big_file();
    let total = content.chars().count();
    let needle_at = content
        .find(NEEDLE)
        .ok_or_else(|| "fixture must contain the needle".to_string())?;

    // Fixture sanity: the whole task is vacuous unless the needle sits past the default window and
    // the file is big enough to be cut at all.
    if needle_at < FS_READ_MAX_CHARS {
        return Err(format!(
            "fixture is wrong: the needle is at {needle_at}, inside the {FS_READ_MAX_CHARS}-char \
             default window, so a head-only cut would still find it"
        ));
    }
    if total <= FS_READ_MAX_CHARS {
        return Err(format!(
            "fixture is wrong: {total} chars never exceeds the {FS_READ_MAX_CHARS}-char window"
        ));
    }

    // --- A. the renderer contract -----------------------------------------------------------

    // A1. A file that fits is returned verbatim: no marker, no decoration. Marker spam on small
    // reads would train the model to ignore markers.
    let small = "short file\n";
    let (text, ok) = render_read_window("small.txt", small, None, None);
    if !ok || text != small {
        return Err(format!("a read that fits must be verbatim, got ok={ok} {:?}", text));
    }
    if text.contains("[truncated") {
        return Err("a read that fits must not carry a truncation marker".into());
    }

    // A2. The default read of a large file cuts from the middle, keeps head *and* tail, says it was
    // cut, and names the true size.
    let (text, ok) = render_read_window(BIG_FILE, &content, None, None);
    if !ok {
        return Err("the default window read must succeed".into());
    }
    for required in [
        "[truncated from the middle",
        &format!("of {total} chars"),
        HEAD_MARKER,
        TAIL_MARKER,
        "\"action\":\"fs_read\"",
    ] {
        if !text.contains(required) {
            return Err(format!(
                "default window must report its bound and keep both ends; missing {required:?} in:\n\
                 {text}"
            ));
        }
    }
    if text.contains(NEEDLE) {
        return Err(
            "the default window must not reach the needle — otherwise the paging assertions below \
             prove nothing"
                .into(),
        );
    }
    if text.chars().count() > FS_READ_MAX_CHARS + 256 {
        return Err(format!(
            "the default window must stay near its budget, got {} chars for a {FS_READ_MAX_CHARS} \
             budget",
            text.chars().count()
        ));
    }

    // A3. An explicit window reaches the needle, and reports what is left plus the literal next
    // step — the bound is recoverable, not terminal.
    let window_start = needle_at - 100;
    let (text, ok) = render_read_window(BIG_FILE, &content, Some(window_start), Some(400));
    if !ok {
        return Err("a windowed read must succeed".into());
    }
    if !text.contains(NEEDLE) {
        return Err(format!(
            "the answer at char {needle_at} must be reachable with offset={window_start} \
             limit=400, got:\n{text}"
        ));
    }
    for required in [
        &format!("[truncated: showing 400 of {total} chars from offset {window_start}"),
        "chars remain",
        "next page",
        &format!("\"offset\":{}", window_start + 400),
    ] {
        if !text.contains(required) {
            return Err(format!(
                "a windowed read must report the remaining work; missing {required:?} in:\n{text}"
            ));
        }
    }

    // A4. An offset past the end is a *failed* observation naming the real size — never an empty
    // string, which is indistinguishable from "the file is empty".
    let (text, ok) = render_read_window(BIG_FILE, &content, Some(total + 10), Some(100));
    if ok {
        return Err("an offset past the end must not report success".into());
    }
    if !text.contains("past the end") || !text.contains(&format!("{total} chars")) {
        return Err(format!(
            "an out-of-range offset must name the real size, got: {text}"
        ));
    }
    if text.trim().is_empty() {
        return Err("an out-of-range offset must not render as an empty read".into());
    }

    // A5. A window that ends exactly at EOF is verbatim — the last page carries no marker.
    let (text, ok) = render_read_window(BIG_FILE, &content, Some(total - 100), Some(100));
    if !ok || text != content[total - 100..] {
        return Err(format!(
            "the final page must be verbatim, got ok={ok} {:?}",
            text.chars().take(60).collect::<String>()
        ));
    }

    // A6. Paging with offset/limit reconstructs the file exactly. This is the assertion that turns
    // "we told you it was cut" into "you can still get all of it".
    let mut rebuilt = String::new();
    let mut offset = 0usize;
    let mut pages = 0usize;
    loop {
        let (text, ok) = render_read_window(BIG_FILE, &content, Some(offset), Some(PAGE));
        if !ok {
            return Err(format!("page at offset {offset} failed: {text}"));
        }
        let body = text.split("\n[truncated").next().unwrap_or_else(|| text.as_str());
        rebuilt.push_str(body);
        offset += PAGE;
        pages += 1;
        if offset >= total {
            break;
        }
        if pages > 4 * (total / PAGE + 2) {
            return Err("paging did not terminate".into());
        }
    }
    if rebuilt != content {
        return Err(format!(
            "paged reads must reconstruct the file exactly: {} chars rebuilt vs {total} original",
            rebuilt.chars().count()
        ));
    }

    // --- B. the same contract through the production loop -----------------------------------

    // B1. `fs_read` with no window: the observation the model actually sees carries the marker and
    // the true size, and does not contain the answer.
    let run = run_plan(
        db,
        "default-window",
        true,
        vec![
            ToolInvocation::FsRead {
                path: BIG_FILE.into(),
                offset: None,
                limit: None,
            },
            ToolInvocation::Done,
        ],
    )?;
    let observed = read_observation(&run)?;
    if !observed.contains("[truncated") || !observed.contains(&format!("of {total} chars")) {
        return Err(format!(
            "the loop's fs_read observation must report the cut and the true size, got:\n{}",
            observed.chars().take(300).collect::<String>()
        ));
    }
    if observed.contains(NEEDLE) {
        return Err("the loop's default read must not reach the needle".into());
    }
    if !run.outcome.as_ref().map(|r| r.done).unwrap_or(false) {
        return Err("a read-only plan must still complete".into());
    }

    // B2. The same goal, paged: the answer is in the observation and the run completes. Read
    // against B1, this is the pair that proves the needle was beyond the window and not absent.
    let run = run_plan(
        db,
        "windowed",
        true,
        vec![
            ToolInvocation::FsRead {
                path: BIG_FILE.into(),
                offset: Some(needle_at - 100),
                limit: Some(400),
            },
            ToolInvocation::Done,
        ],
    )?;
    let observed = read_observation(&run)?;
    if !observed.contains(NEEDLE) {
        return Err(format!(
            "a windowed fs_read must reach the answer at char {needle_at}, got:\n{}",
            observed.chars().take(300).collect::<String>()
        ));
    }
    if !run.outcome.as_ref().map(|r| r.done).unwrap_or(false) {
        return Err("the paged read plan must complete".into());
    }

    // B3. A write-only session: the refusal must carry the remedy and be classified terminal, so a
    // replanning caller stops instead of spending its budget (P1-9 meets P1-6 — an unreadable file
    // is not a "not present" file).
    let run = run_plan(
        db,
        "no-read-grant",
        false,
        vec![ToolInvocation::FsRead {
            path: BIG_FILE.into(),
            offset: None,
            limit: None,
        }],
    )?;
    let denied = match &run.outcome {
        Err(message) => message.clone(),
        Ok(_) => {
            return Err(
                "a read without a read grant must be refused, not silently allowed".into()
            )
        }
    };
    for required in ["Read denied for", "read grant", "retryable: no"] {
        if !denied.contains(required) {
            return Err(format!(
                "the read refusal must be remedy-bearing; missing {required:?} in: {denied}"
            ));
        }
    }
    let classified = ToolError::classify(&denied);
    if classified.retryable {
        return Err(format!(
            "a missing grant is not repairable by a new plan, got retryable: {denied}"
        ));
    }
    if classify_denial(&denied) != DenialCategory::Permission {
        return Err(format!(
            "the remedy-bearing refusal must still classify as a permission denial, got {:?}",
            classify_denial(&denied)
        ));
    }

    // --- C. the other two bounded surfaces --------------------------------------------------

    // C1. Memory injection reports how much it dropped.
    let hits: Vec<RetrievedMemory> = (0..40)
        .map(|i| RetrievedMemory {
            chunk_id: format!("sess-read01::t{i}::turn"),
            text: format!("memory hit {i}: ") + &"detail ".repeat(60),
            similarity: 0.9,
        })
        .collect();
    let prompt = enrich_prompt_with_memory("What did we decide about the read budget?", &hits);
    if !prompt.contains("[memory truncated:") {
        return Err(
            "an exhausted memory budget must say so, instead of dropping hits silently".into()
        );
    }
    if !prompt.contains(&format!("of {} hits shown", hits.len())) {
        return Err(format!(
            "the memory truncation marker must name the true hit count, got: {}",
            prompt.chars().take(200).collect::<String>()
        ));
    }
    for required in [
        "<retrieved_memory trust=\"untrusted\">",
        "</retrieved_memory>",
        "narrow the query",
    ] {
        if !prompt.contains(required) {
            return Err(format!(
                "the memory injection must stay untrusted-marked and actionable; missing \
                 {required:?}"
            ));
        }
    }
    if !prompt.ends_with("Current user request:\nWhat did we decide about the read budget?") {
        return Err("the current request must still come last, verbatim".into());
    }
    if prompt.chars().count() > MAX_MEMORY_CONTEXT_CHARS + 256 {
        return Err(format!(
            "the memory injection exceeded its own budget: {} chars",
            prompt.chars().count()
        ));
    }
    // And the marker must not appear when nothing was dropped.
    let few: Vec<RetrievedMemory> = hits.iter().take(2).cloned().collect();
    let small_prompt = enrich_prompt_with_memory("short question", &few);
    if small_prompt.contains("[memory truncated:") {
        return Err("a memory injection that fit must not claim it was truncated".into());
    }

    // C2. The subagent preview reports its bound too.
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let long = "subagent line\n".repeat(220);
    std::fs::write(workspace.join("sub.txt"), &long).map_err(|e| e.to_string())?;
    std::fs::write(workspace.join("tiny.txt"), "tiny\n").map_err(|e| e.to_string())?;
    let result = run_subagent_read_task(
        &workspace,
        &["sub.txt".to_string(), "tiny.txt".to_string()],
    )
    .map_err(|e| format!("subagent read failed: {e}"))?;
    let previewed = result
        .files
        .iter()
        .find(|f| f.path == "sub.txt")
        .ok_or_else(|| "sub.txt missing from the subagent summary".to_string())?;
    if !previewed.preview.contains("[preview:") {
        return Err(format!(
            "a 200-char preview of a {}-char file must say it is a preview, got: {:?}",
            long.chars().count(),
            previewed.preview
        ));
    }
    if !previewed
        .preview
        .contains(&format!("of {} chars", long.chars().count()))
    {
        return Err(format!(
            "the preview marker must name the true size, got: {:?}",
            previewed.preview
        ));
    }
    let tiny = result
        .files
        .iter()
        .find(|f| f.path == "tiny.txt")
        .ok_or_else(|| "tiny.txt missing from the subagent summary".to_string())?;
    if tiny.preview.contains("[preview:") {
        return Err("a preview that was not cut must not carry a marker".into());
    }
    if !result.distilled.contains("sub.txt") {
        return Err("the distilled summary must still name each file".into());
    }

    Ok(())
}
