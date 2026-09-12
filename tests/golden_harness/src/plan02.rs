//! PLAN-02 — the planner must be able to discover, and must be told what exists (P0-2).
//!
//! Two halves of one defect. A plan can only name paths it was given, so any goal that says "the
//! notes" or "the config" is a guess; and a planner that is not told which MCP servers are connected
//! or which skills are installed invents them, then burns a replan discovering otherwise. The first
//! half is a missing capability (`fs_list` did not exist — `fs_read` on a directory simply failed),
//! the second is missing context.
//!
//! Both halves are pinned here:
//!
//! * `fs_list` — a real action, through the real `ToolRegistry`, with the same hook, the same read
//!   grant, and the same remedy-bearing denial as `fs_read` (a new read surface that skipped them
//!   would be a hole, not a feature). Its listing is sorted, marks directories, states an empty
//!   directory as data rather than an error, and reports its bound when cut (P1-6's discipline).
//! * the capability block — `PlannerContext` + `build_capability_context`, appended *after*
//!   `build_nl_plan_prompt` so the stable prefix the prefix cache measures stays stable (CACHE-01).
//!   Absence is stated, never omitted: "MCP servers connected: none" plus "do not emit one" is a
//!   fact a planner can use, where a missing section reads as "unknown" and gets filled with an
//!   invention.
//!
//! Deterministic: no model and no network. The planner-prompt assertions are on pure functions, and
//! the execution assertions drive frozen plans through `execute_structured_loop`.

use aether_core::{
    build_capability_context, build_nl_plan_prompt, classify_denial, normalize_nl_plan_json,
    plan_tool_name, render_dir_listing, validate_goal_coverage, validate_nl_plan, DenialCategory,
    LoopConfig, PlannerContext, ToolError, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS,
    FS_LIST_MAX_ENTRIES, PLANNER_CONTEXT_MAX_ENTRIES,
};
use aether_daemon::task_runner::{execute_structured_loop, planner_context};
use aether_db::Database;
use aether_mcp::{McpAllowlist, McpServerConfig};
use aether_skills::SkillDefinition;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

const SERVER: &str = "forge-local";
const SKILL: &str = "release-notes";

struct Run {
    /// Kept alive so `workspace` stays valid for the assertions that follow.
    _tmp: TempDir,
    #[allow(dead_code)]
    workspace: PathBuf,
    outcome: Result<aether_core::LoopRunResult, String>,
}

/// Run a plan through the production loop in a workspace seeded with `seed` (path → content; a
/// trailing `/` on the path creates an empty directory). Grants are read + write, the pair
/// `select_workspace` creates.
fn run_plan(
    db: &Database,
    case_id: &str,
    grant_read: bool,
    seed: &[&str],
    plan: Vec<ToolInvocation>,
) -> Result<Run, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let session_id = format!("sess-plan02-{case_id}");
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'PLAN-02', 'active')",
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
    for entry in seed {
        if entry.ends_with('/') {
            std::fs::create_dir_all(workspace.join(entry.trim_end_matches('/')))
                .map_err(|e| format!("seeding dir {entry} failed: {e}"))?;
        } else {
            let target = workspace.join(entry);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("seeding {entry} failed: {e}"))?;
            }
            std::fs::write(&target, format!("seeded {entry}\n"))
                .map_err(|e| format!("seeding {entry} failed: {e}"))?;
        }
    }

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
        "plan02-case",
    );
    Ok(Run {
        _tmp: tmp,
        workspace,
        outcome: result.map_err(|e| e.to_string()),
    })
}

fn list_observation(run: &Run) -> Result<String, String> {
    let result = run
        .outcome
        .as_ref()
        .map_err(|e| format!("expected the listing plan to run, got: {e}"))?;
    result
        .observations
        .iter()
        .find(|o| o.tool == "fs_list")
        .map(|o| o.output.clone())
        .ok_or_else(|| "no fs_list observation was recorded".to_string())
}

fn one_server_allowlist() -> McpAllowlist {
    McpAllowlist {
        servers: vec![McpServerConfig {
            name: SERVER.into(),
            version: "1.0.0".into(),
            command: "/bin/true".into(),
            args: vec![],
            sha256_pin: "unused-by-this-task".repeat(4),
            entry_sha256_pin: None,
            tools_hash_pin: None,
            default_policy: "deny".into(),
        }],
    }
}

fn one_skill() -> HashMap<String, SkillDefinition> {
    let mut skills = HashMap::new();
    skills.insert(
        SKILL.to_string(),
        SkillDefinition {
            id: SKILL.into(),
            name: "Release notes".into(),
            description: "Draft release notes from a changelog".into(),
            markdown_body: "# Release notes\n".into(),
            steps: vec![],
            capabilities: None,
        },
    );
    skills
}

pub fn test_plan02_impl(db: &Database) -> Result<(), String> {
    // --- A. the listing renderer ------------------------------------------------------------

    // A1. Sorted, directories marked, and the header states the count it is showing.
    let (text, ok) = render_dir_listing(
        ".",
        vec![
            ("zeta.txt".to_string(), false),
            ("src".to_string(), true),
            ("alpha.txt".to_string(), false),
        ],
    );
    if !ok {
        return Err("a listing of an existing directory must succeed".into());
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 4 {
        return Err(format!("expected a header plus three entries, got: {text:?}"));
    }
    if !lines[0].contains("3 of 3 entries in ./") {
        return Err(format!("the header must state the entry count, got: {:?}", lines[0]));
    }
    if lines[1] != "alpha.txt" || lines[2] != "src/" || lines[3] != "zeta.txt" {
        return Err(format!(
            "entries must be sorted with directories marked, got: {lines:?}"
        ));
    }

    // A2. An empty directory is data, not an error: zero results must not read as failure, or a
    // planner retries the same guess forever.
    let (text, ok) = render_dir_listing("empty", vec![]);
    if !ok || !text.contains("0 entries") || !text.contains("empty") {
        return Err(format!(
            "an empty directory must be a successful observation that says it is empty, got \
             ok={ok} {text:?}"
        ));
    }

    // A3. A cut listing reports its bound and where it stopped (P1-6's rule, applied to names).
    let many: Vec<(String, bool)> = (0..FS_LIST_MAX_ENTRIES + 25)
        .map(|i| (format!("file{i:04}.txt"), false))
        .collect();
    let total = many.len();
    let (text, ok) = render_dir_listing("big", many);
    if !ok {
        return Err("a long listing must still succeed".into());
    }
    for required in [
        &format!("{FS_LIST_MAX_ENTRIES} of {total} entries in big/"),
        "[listing truncated",
        &format!("{} not shown", total - FS_LIST_MAX_ENTRIES),
        "list a subdirectory to narrow",
    ] {
        if !text.contains(required) {
            return Err(format!(
                "a cut listing must report its bound; missing {required:?} in the first line and \
                 the tail: {:?} … {:?}",
                text.lines().next(),
                text.lines().last()
            ));
        }
    }
    // The last name shown is the resume point, and the first name not shown sorts after it.
    let last_shown = format!("file{:04}.txt", FS_LIST_MAX_ENTRIES - 1);
    if !text.contains(&last_shown) || text.contains(&format!("file{:04}.txt", total - 1)) {
        return Err(format!(
            "the listing must stop at {last_shown} and say so, got tail {:?}",
            text.lines().last()
        ));
    }

    // --- B. the same action through the production loop --------------------------------------

    // B1. The workspace root, listed by a plan: every seeded entry is named, the directory is
    // marked, and the run completes.
    let run = run_plan(
        db,
        "root",
        true,
        &["notes.txt", "config.json", "src/"],
        vec![ToolInvocation::FsList { path: None }, ToolInvocation::Done],
    )?;
    let observed = list_observation(&run)?;
    // The exact entry count is pinned on the renderer above (A1/A3); here the point is that the
    // production path names what is on disk and marks the directory.
    for required in ["notes.txt", "config.json", "src/", "entries in ./"] {
        if !observed.contains(required) {
            return Err(format!(
                "the production listing must name what exists; missing {required:?} in:\n{observed}"
            ));
        }
    }
    if !run.outcome.as_ref().map(|r| r.done).unwrap_or(false) {
        return Err("a read-only listing plan must still complete".into());
    }

    // B2. A subdirectory, by relative path — discovery is recursive in the sense that matters: one
    // listing per level, each bounded.
    let run = run_plan(
        db,
        "subdir",
        true,
        &["src/main.py"],
        vec![
            ToolInvocation::FsList {
                path: Some("src".into()),
            },
            ToolInvocation::Done,
        ],
    )?;
    let observed = list_observation(&run)?;
    if !observed.contains("main.py") || !observed.contains("entries in src/") {
        return Err(format!("listing a subdirectory must show its entries, got:\n{observed}"));
    }

    // B3. A listing is a read: without the read grant it is refused with the same remedy-bearing
    // denial as `fs_read`, and classified the same way. A new surface that skipped the grant would
    // be a hole.
    let run = run_plan(
        db,
        "no-read-grant",
        false,
        &["notes.txt"],
        vec![ToolInvocation::FsList { path: None }],
    )?;
    let denied = match &run.outcome {
        Err(message) => message.clone(),
        Ok(_) => {
            return Err("a listing without a read grant must be refused, not served".into())
        }
    };
    for required in ["Read denied for", "read grant", "retryable: no"] {
        if !denied.contains(required) {
            return Err(format!(
                "the listing refusal must carry its remedy; missing {required:?} in: {denied}"
            ));
        }
    }
    if ToolError::classify(&denied).retryable {
        return Err(format!("a missing grant is terminal, got retryable: {denied}"));
    }
    if classify_denial(&denied) != DenialCategory::Permission {
        return Err(format!(
            "the refusal must still classify as a permission denial, got {:?}",
            classify_denial(&denied)
        ));
    }

    // B4. Discovery does not widen the boundary: `..` is refused before anything is listed.
    let run = run_plan(
        db,
        "escape",
        true,
        &["notes.txt"],
        vec![ToolInvocation::FsList {
            path: Some("../outside".into()),
        }],
    )?;
    match &run.outcome {
        Err(message) if message.contains("traversal") || message.contains("escapes") => {}
        other => {
            return Err(format!(
                "a listing outside the workspace must be refused as traversal, got: {other:?}"
            ))
        }
    }

    // B5. The sensitive-path hook fires on the new surface too, grant or no grant.
    let run = run_plan(
        db,
        "hook",
        true,
        &[".env"],
        vec![ToolInvocation::FsList {
            path: Some(".env".into()),
        }],
    )?;
    match &run.outcome {
        Err(message) if message.contains("PreToolUse hook blocked") => {}
        other => {
            return Err(format!(
                "listing a denylisted path must hit the same hook as reading it, got: {other:?}"
            ))
        }
    }

    // --- C. the push half: what the planner is told ------------------------------------------

    // C1. A populated inventory is named.
    let context = PlannerContext {
        mcp_servers: vec![SERVER.to_string()],
        skills: vec![SKILL.to_string()],
        workspace_entries: vec!["notes.txt".to_string(), "src/".to_string()],
    };
    let block = build_capability_context(&context);
    for required in [SERVER, SKILL, "notes.txt", "src/", "fs_list"] {
        if !block.contains(required) {
            return Err(format!(
                "the capability block must name what exists; missing {required:?} in:\n{block}"
            ));
        }
    }

    // C2. An empty inventory says so, and says what that means for the plan. Silence reads as
    // "unknown", and "unknown" is what a model fills with an invented server.
    let empty = build_capability_context(&PlannerContext::default());
    for required in [
        "MCP servers connected: none",
        "Skills installed: none",
        "No MCP server is connected",
        "Do not emit one",
        "No skill is installed",
    ] {
        if !empty.contains(required) {
            return Err(format!(
                "absence must be stated, not omitted; missing {required:?} in:\n{empty}"
            ));
        }
    }

    // C3. The workspace section is bounded, and the bound is reported with the way past it.
    let wide = PlannerContext {
        mcp_servers: vec![],
        skills: vec![],
        workspace_entries: (0..PLANNER_CONTEXT_MAX_ENTRIES + 7)
            .map(|i| format!("entry{i:03}.txt"))
            .collect(),
    };
    let block = build_capability_context(&wide);
    if !block.contains(&format!("+7 more")) || !block.contains("use fs_list to see them") {
        return Err(format!(
            "an over-long workspace listing must report what it hid and how to see it:\n{block}"
        ));
    }
    if block.contains("entry046") {
        return Err("entries past the cap must not be named".into());
    }

    // C4. The daemon's collector reads the real inventory: connected servers, installed skills, and
    // the actual top level of the workspace, directories marked.
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    std::fs::write(workspace.join("notes.txt"), "n").map_err(|e| e.to_string())?;
    std::fs::create_dir(workspace.join("src")).map_err(|e| e.to_string())?;
    let collected = planner_context(&workspace, Some(&one_server_allowlist()), &one_skill());
    if collected.mcp_servers != vec![SERVER.to_string()] {
        return Err(format!(
            "the collector must report connected servers, got: {:?}",
            collected.mcp_servers
        ));
    }
    if collected.skills != vec![SKILL.to_string()] {
        return Err(format!(
            "the collector must report installed skills, got: {:?}",
            collected.skills
        ));
    }
    if !collected.workspace_entries.contains(&"notes.txt".to_string())
        || !collected.workspace_entries.contains(&"src/".to_string())
    {
        return Err(format!(
            "the collector must list the workspace top level with directories marked, got: {:?}",
            collected.workspace_entries
        ));
    }
    // No allowlist and no skills is a *known* empty, and must render as one.
    let bare = planner_context(&workspace, None, &HashMap::new());
    if !bare.mcp_servers.is_empty() || !bare.skills.is_empty() {
        return Err("with nothing connected the collector must report nothing".into());
    }
    if !build_capability_context(&bare).contains("MCP servers connected: none") {
        return Err("a known-empty inventory must still be stated".into());
    }

    // C5. The volatile block stays out of the stable prefix, or every workspace change would
    // invalidate the prefix cache CACHE-01 measures.
    let stable = build_nl_plan_prompt("summarise the notes in this project");
    if stable.contains("MCP servers connected") || stable.contains(SERVER) {
        return Err(
            "the capability block must be appended after the stable prefix, not embedded in it"
                .into(),
        );
    }
    if !stable.contains("\"action\":\"fs_list\"") {
        return Err(
            "the action catalog is part of the stable prefix and must offer fs_list".into(),
        );
    }
    if !stable.contains("Never guess a path") {
        return Err("the catalog must tell the planner to discover instead of guessing".into());
    }
    if !block.contains("list first rather than guessing") {
        return Err("the capability block must repeat the discover-before-guessing rule".into());
    }

    // --- D. the plan shape is accepted, and the catalog stays closed --------------------------

    let normalized = normalize_nl_plan_json(
        r#"{"loop":[{"action":"fs_list"},{"action":"fs_list","path":"src"},{"action":"done"}]}"#,
    )
    .map_err(|e| format!("a fs_list plan must decode: {e:?}"))?;
    let plan = validate_nl_plan(&normalized, 8)
        .map_err(|e| format!("a fs_list plan must validate: {e:?}"))?;
    if !matches!(plan[0], ToolInvocation::FsList { path: None }) {
        return Err(format!("fs_list without a path must mean the workspace root: {:?}", plan[0]));
    }
    match &plan[1] {
        ToolInvocation::FsList { path: Some(path) } if path == "src" => {}
        other => return Err(format!("fs_list must carry its path: {other:?}")),
    }
    if plan_tool_name(&plan[0]) != "fs_list" {
        return Err(format!(
            "the action must be named fs_list in telemetry, got {:?}",
            plan_tool_name(&plan[0])
        ));
    }

    // A goal that asks for a listing now requires the action that produces one.
    if validate_goal_coverage("list the files in this project", &[plan[2].clone()]).is_ok() {
        return Err("a goal that asks to list must require fs_list, not accept a done-only plan".into());
    }
    if validate_goal_coverage("list the files in this project", &plan).is_err() {
        return Err("a plan that lists must satisfy a goal that asks to list".into());
    }

    // The catalog is closed: an invented action is still rejected, so adding fs_list did not turn
    // the plan validator into a pass-through.
    let bogus_plan = normalize_nl_plan_json(
        r#"{"loop":[{"action":"fs_delete","path":"x"},{"action":"done"}]}"#,
    )
    .map_err(|e| format!("an invented action should at least parse as JSON: {e:?}"))?;
    let bogus = validate_nl_plan(&bogus_plan, 8)
        .err()
        .ok_or("an unknown action must be rejected, not decoded".to_string())?;
    if !format!("{bogus:?}").contains("InvalidStep") {
        return Err(format!("an unknown action must be an invalid step, got: {bogus:?}"));
    }

    Ok(())
}
