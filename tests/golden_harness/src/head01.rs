//! HEAD-01 — headless `--json` NDJSON stream (Phase 10 slice 10.12).

use aether_core::{ModelBackend, ModelRouter};
use aether_daemon::headless::{run_headless_task, HeadlessExitCode, HeadlessOptions};
use aether_daemon::DaemonState;
use aether_db::Database;

pub fn head01_fixture_ready() -> Result<(), String> {
    Ok(())
}

pub fn test_head01_impl() -> Result<bool, String> {
    let db = Database::open_in_memory().map_err(|e| e.to_string())?;
    let state = DaemonState {
        db,
        router: ModelRouter::new(
            ModelBackend::OllamaMlx {
                endpoint: "http://localhost:11434".into(),
                model: "qwen2.5:3b".into(),
            },
            None,
        ),
        auth_token: String::new(),
    };
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let prompt = r#"{"loop":[
{"action":"fs_write","path":"head01.txt","content":"HEAD-01"},
{"action":"verify_contains","path":"head01.txt","text":"HEAD-01"},
{"action":"python_lint","source":"def ok():\n    return 1\n"},
{"action":"done"}
]}"#;
    let code = run_headless_task(
        &state,
        &HeadlessOptions {
            prompt: prompt.to_string(),
            session_id: "sess-head01".into(),
            workspace_path: Some(tmp.path().to_string_lossy().to_string()),
            max_iterations: None,
            max_tokens: None,
            approved: false,
        },
        &mut buf,
    );
    if code != HeadlessExitCode::Success {
        return Err(format!("HEAD-01 expected exit 0, got {}", code.as_i32()));
    }
    let types: Vec<String> = String::from_utf8(buf)
        .map_err(|e| e.to_string())?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
                .ok_or_else(|| format!("HEAD-01 invalid line: {line}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for required in ["plan", "tool", "observe", "verify", "done"] {
        if !types.iter().any(|t| t == required) {
            return Err(format!("HEAD-01 missing event '{required}'"));
        }
    }
    Ok(true)
}
