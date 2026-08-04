//! SEC-01 — brokered secret absent from context, session log, audit log, and crash dump
//! (Phase 11 slice 11.6).
//!
//! A tool authenticates using a secret *by name* (`secret_env` on `mcp_call`). The resolved
//! value is injected into the MCP subprocess environment at spawn time and must never appear in:
//! 1. plan/context JSON (only the name),
//! 2. the session log transcript,
//! 3. the audit_log `arguments_json` rows,
//! 4. a synthesized crash-dump blob of the above sinks.
//!
//! The fixture MCP server deliberately echoes the raw secret in its tool result so this task
//! proves redaction, not merely that a well-behaved tool happened not to leak.

use aether_core::{
    delete_named_secret, store_named_secret, LoopConfig, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS,
};
use aether_daemon::session_log::SessionLogWriter;
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use aether_mcp::McpAllowlist;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const SECRET_NAME: &str = "API_TOKEN";
const SECRET_VALUE: &str = "sk-sec01-never-leak-this-value";

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

/// Install a brokered secret for the harness: Keychain on Darwin (production path), env var
/// elsewhere (CI/Linux). Restores prior state on drop so the missing-secret case stays clean.
enum BrokeredSecretGuard {
    Keychain {
        name: String,
    },
    Env(EnvGuard),
}

impl BrokeredSecretGuard {
    fn install(name: &str, value: &str) -> Result<Self, String> {
        if cfg!(target_os = "macos") {
            delete_named_secret(name).map_err(|e| e.to_string())?;
            store_named_secret(name, value).map_err(|e| e.to_string())?;
            Ok(Self::Keychain {
                name: name.to_string(),
            })
        } else {
            let env_key = format!("AETHER_SECRET_{}", name.to_ascii_uppercase());
            Ok(Self::Env(EnvGuard::set(&env_key, value)))
        }
    }
}

impl Drop for BrokeredSecretGuard {
    fn drop(&mut self) {
        if let Self::Keychain { name } = self {
            let _ = delete_named_secret(name);
        }
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn which_python3() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("AETHER_SEC01_PYTHON") {
        return Ok(PathBuf::from(p));
    }
    for candidate in ["/usr/bin/python3", "/opt/homebrew/bin/python3", "/usr/local/bin/python3"]
    {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Ok(path);
        }
    }
    let output = Command::new("which")
        .arg("python3")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("python3 not found for SEC-01 fixture MCP server".into());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err("python3 not found for SEC-01 fixture MCP server".into());
    }
    Ok(PathBuf::from(path))
}

fn fixture_script() -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest_dir.join("fixtures/sec01_secret_mcp.py");
    if !script.is_file() {
        return Err(format!("SEC-01 fixture missing: {}", script.display()));
    }
    Ok(script)
}

fn secret_allowlist() -> Result<McpAllowlist, String> {
    let python = which_python3()?;
    let script = fixture_script()?;
    Ok(McpAllowlist {
        servers: vec![aether_mcp::McpServerConfig {
            name: "sec01-secret".into(),
            version: "1.0.0".into(),
            command: python.to_string_lossy().into_owned(),
            args: vec![script.to_string_lossy().into_owned()],
            sha256_pin: sha256_file(&python)?,
            entry_sha256_pin: Some(sha256_file(&script)?),
            tools_hash_pin: None,
            default_policy: "prompt_always".into(),
        }],
    })
}

fn assert_absent(label: &str, haystack: &str, needle: &str) -> Result<(), String> {
    if haystack.contains(needle) {
        return Err(format!(
            "SEC-01 leak: secret value present in {label} ({} chars of sink)",
            haystack.len()
        ));
    }
    Ok(())
}

fn dump_audit_log(conn: &rusqlite::Connection) -> Result<String, String> {
    let mut stmt = conn
        .prepare(
            "SELECT tool_name, arguments_json, decision, content_hash FROM audit_log ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(format!(
                "{}|{}|{}|{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    for row in rows {
        out.push_str(&row.map_err(|e| e.to_string())?);
        out.push('\n');
    }
    Ok(out)
}

pub fn test_sec01_impl(db: &Database) -> Result<(), String> {
    let expected_fp = {
        let mut hasher = Sha256::new();
        hasher.update(SECRET_VALUE.as_bytes());
        format!("{:x}", hasher.finalize())
    };

    // Darwin: brokered secrets resolve from Keychain (production path). Linux/CI: same-named
    // `AETHER_SECRET_<NAME>` env var fallback used by the daemon off Darwin.
    let _guard = BrokeredSecretGuard::install(SECRET_NAME, SECRET_VALUE)?;

    let session_id = "sess-sec01";
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let log_root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let _log_env = EnvGuard::set(
        "AETHER_SESSION_LOG_DIR",
        &log_root.path().to_string_lossy(),
    );

    {
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'SEC-01', 'active')",
            rusqlite::params![session_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             VALUES (?1, ?2, 'mcp_call')",
            rusqlite::params![session_id, workspace.to_string_lossy().to_string()],
        )
        .map_err(|e| e.to_string())?;
    }

    let allowlist = secret_allowlist()?;
    let plan = vec![
        ToolInvocation::McpCall {
            server: "sec01-secret".into(),
            tool: "authenticate".into(),
            args: serde_json::json!({}),
            secret_env: Some(SECRET_NAME.into()),
        },
        ToolInvocation::Done,
    ];

    // --- Sink 1: context / plan must carry the name only (value never constructed into the plan). ---
    let plan_ctx = format!("{plan:?}");
    assert_absent("plan/context", &plan_ctx, SECRET_VALUE)?;
    if !plan_ctx.contains(SECRET_NAME) {
        return Err("expected plan to mention the secret name API_TOKEN".into());
    }

    let mut config = LoopConfig {
        max_iterations: 4,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        session_id: session_id.to_string(),
        workspace: workspace.clone(),
    };

    let (result, events) = {
        let conn = db.conn();
        execute_structured_loop(
            &conn,
            &mut config,
            plan,
            Some(&allowlist),
            &HashMap::new(),
            None,
            "sec01-brokered-secret",
        )
    };
    let run = result.map_err(|e| e.to_string())?;
    if !run.done {
        return Err(format!(
            "expected SEC-01 plan to complete, observations={:?}",
            run.observations
        ));
    }

    let mcp_obs = run
        .observations
        .iter()
        .find(|o| o.tool == "mcp_call")
        .ok_or("expected mcp_call observation")?;
    if !mcp_obs.success {
        return Err(format!(
            "expected brokered-secret mcp_call to succeed, got: {}",
            mcp_obs.output
        ));
    }
    if !mcp_obs.output.contains("authenticated=true") {
        return Err(format!(
            "expected tool to authenticate with brokered secret, got: {}",
            mcp_obs.output
        ));
    }
    if !mcp_obs.output.contains(&expected_fp) {
        return Err(format!(
            "expected fingerprint {expected_fp} proving the secret reached the tool, got: {}",
            mcp_obs.output
        ));
    }
    assert_absent("mcp_call observation", &mcp_obs.output, SECRET_VALUE)?;
    if !mcp_obs.output.contains("[REDACTED]") {
        return Err(
            "expected observation redaction marker [REDACTED] after fixture echoed the secret"
                .into(),
        );
    }

    // --- Sink 2: session log. ---
    let records = SessionLogWriter::from_env()
        .read_session_log(session_id)
        .map_err(|e| e.to_string())?;
    if records.is_empty() {
        return Err("expected session log records for SEC-01 turn".into());
    }
    let session_blob = serde_json::to_string(&records).map_err(|e| e.to_string())?;
    assert_absent("session log", &session_blob, SECRET_VALUE)?;

    // --- Sink 3: audit log. ---
    let audit_blob = {
        let conn = db.conn();
        dump_audit_log(&conn)?
    };
    if audit_blob.is_empty() {
        return Err("expected audit_log rows for SEC-01 mcp_call".into());
    }
    assert_absent("audit log", &audit_blob, SECRET_VALUE)?;

    // --- Sink 4: crash dump (serialized in-process state an abrupt death would retain). ---
    let events_blob = format!("{events:?}");
    let obs_blob = format!("{:?}", run.observations);
    let crash_dump = format!(
        "plan={plan_ctx}\nsession={session_blob}\naudit={audit_blob}\nevents={events_blob}\nobs={obs_blob}"
    );
    assert_absent("crash dump", &crash_dump, SECRET_VALUE)?;

    // Missing secret must fail closed (no silent empty-env auth).
    {
        let missing_session = "sess-sec01-missing";
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'SEC-01-missing', 'active')",
            rusqlite::params![missing_session],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             VALUES (?1, ?2, 'mcp_call')",
            rusqlite::params![missing_session, workspace.to_string_lossy().to_string()],
        )
        .map_err(|e| e.to_string())?;
    }
    drop(_guard); // remove brokered secret before missing-secret case
    let mut missing_config = LoopConfig {
        max_iterations: 4,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        session_id: "sess-sec01-missing".into(),
        workspace,
    };
    let missing_plan = vec![
        ToolInvocation::McpCall {
            server: "sec01-secret".into(),
            tool: "authenticate".into(),
            args: serde_json::json!({}),
            secret_env: Some(SECRET_NAME.into()),
        },
        ToolInvocation::Done,
    ];
    let (missing_result, _) = {
        let conn = db.conn();
        execute_structured_loop(
            &conn,
            &mut missing_config,
            missing_plan,
            Some(&allowlist),
            &HashMap::new(),
            None,
            "sec01-missing-secret",
        )
    };
    match missing_result {
        Err(e) => {
            let msg = e.to_string();
            if !msg.contains("not configured") {
                return Err(format!(
                    "expected missing brokered secret to fail closed with 'not configured', got: {msg}"
                ));
            }
            assert_absent("missing-secret error", &msg, SECRET_VALUE)?;
        }
        Ok(run) => {
            return Err(format!(
                "expected missing brokered secret to fail the turn, got done={}",
                run.done
            ));
        }
    }

    Ok(())
}
