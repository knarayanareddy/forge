use aether_mcp::{discover_filesystem_mcp, invoke_with_grant, McpAllowlist};
use aether_permissions::{PermissionDecision, PermissionManager};
use serde_json::json;
use std::fs;
use tempfile::tempdir;

pub async fn test_mcp_01_impl(conn: &rusqlite::Connection) -> Result<(), String> {
    let paths = discover_filesystem_mcp().map_err(|e| {
        format!(
            "MCP-01 requires @modelcontextprotocol/server-filesystem: {}. Install with: npm install -g @modelcontextprotocol/server-filesystem",
            e
        )
    })?;

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace_path = tmp.path();
    let workspace_str = workspace_path.to_string_lossy().to_string();

    fs::write(workspace_path.join("file1.txt"), "alpha").map_err(|e| e.to_string())?;
    fs::write(workspace_path.join("file2.txt"), "beta").map_err(|e| e.to_string())?;

    let default_allowlist_path = std::path::Path::new("mcp_allowlist.json");
    if default_allowlist_path.exists() {
        let allowlist =
            McpAllowlist::load_from_file(default_allowlist_path).map_err(|e| e.to_string())?;
        let server = allowlist
            .servers
            .iter()
            .find(|s| s.name == "filesystem")
            .ok_or_else(|| "Default allowlist missing filesystem server".to_string())?;
        for (label, pin) in [
            ("sha256_pin", server.sha256_pin.as_str()),
            (
                "entry_sha256_pin",
                server.entry_sha256_pin.as_deref().unwrap_or(""),
            ),
            (
                "tools_hash_pin",
                server.tools_hash_pin.as_deref().unwrap_or(""),
            ),
        ] {
            if pin.is_empty()
                || pin.starts_with("PENDING")
                || pin.starts_with("REPLACE_WITH_")
            {
                return Err(format!(
                    "Default allowlist {} must be a verified 64-char hex pin, got {:?}",
                    label, pin
                ));
            }
            if pin.len() != 64 || !pin.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "Default allowlist {} must be 64-char hex, got {:?}",
                    label, pin
                ));
            }
        }
    }

    let allowlist = McpAllowlist::resolve_filesystem().map_err(|e| e.to_string())?;
    allowlist
        .verify_and_get("filesystem")
        .map_err(|e| e.to_string())?;

    let session_id = "sess-mcp-01";
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'MCP Session', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;

    let denied = PermissionManager::check_file_access(conn, session_id, &workspace_str, "mcp_call")
        .map_err(|e| e.to_string())?;
    if denied != PermissionDecision::Denied {
        return Err("Expected MCP call to ungranted workspace to be denied".into());
    }

    let denied_invoke = invoke_with_grant(
        conn,
        session_id,
        &workspace_str,
        &allowlist,
        "filesystem",
        "list_directory",
        json!({ "path": workspace_str }),
        &[workspace_str.clone()],
        &[],
    );
    if denied_invoke.is_ok() {
        return Err("Expected MCP invoke without grant to be denied".into());
    }

    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'mcp_call')",
        rusqlite::params![session_id, workspace_str],
    )
    .map_err(|e| e.to_string())?;

    let approved = PermissionManager::check_file_access(conn, session_id, &workspace_str, "mcp_call")
        .map_err(|e| e.to_string())?;
    if approved != PermissionDecision::Approved {
        return Err("Expected MCP call to granted workspace to be approved".into());
    }

    let (result, tools_audit) = invoke_with_grant(
        conn,
        session_id,
        &workspace_str,
        &allowlist,
        "filesystem",
        "list_directory",
        json!({ "path": workspace_str }),
        &[workspace_str.clone()],
        &[],
    )
    .map_err(|e| e.to_string())?;

    if tools_audit.tools.is_empty() {
        return Err("tools/list returned no tools".into());
    }

    if !tools_audit
        .tools
        .iter()
        .any(|t| t.name == "list_directory")
    {
        return Err("Expected list_directory tool in tools/list".into());
    }

    if tools_audit.tools_hash.len() != 64 {
        return Err("Expected SHA-256 tools_hash from list_tools audit".into());
    }

    let content = result
        .get("content")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "tools/call missing content array".to_string())?;

    let text = content
        .first()
        .and_then(|block| block.get("text"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "tools/call missing text content".to_string())?;

    if !text.contains("file1.txt") || !text.contains("file2.txt") {
        return Err(format!(
            "list_directory did not return workspace files: {}",
            text
        ));
    }

    let wrong_pin_allowlist = McpAllowlist {
        servers: vec![{
            let mut entry = paths.to_allowlist_entry();
            entry.tools_hash_pin = Some("deadbeef".repeat(8));
            entry
        }],
    };
    let wrong_pin = invoke_with_grant(
        conn,
        session_id,
        &workspace_str,
        &wrong_pin_allowlist,
        "filesystem",
        "list_directory",
        json!({ "path": workspace_str }),
        &[workspace_str.clone()],
        &[],
    );
    if wrong_pin.is_ok() {
        return Err("Expected tools_hash pin mismatch to block MCP invoke".into());
    }

    let pinned_allowlist = McpAllowlist {
        servers: vec![{
            let mut entry = paths.to_allowlist_entry();
            entry.tools_hash_pin = Some(tools_audit.tools_hash.clone());
            entry
        }],
    };
    pinned_allowlist
        .verify_and_get("filesystem")
        .map_err(|e| e.to_string())?;

    Ok(())
}
