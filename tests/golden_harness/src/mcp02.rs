//! MCP-02 — user-addable MCP servers with pin-on-install and diff-on-update review.

use aether_mcp::{
    discover_filesystem_mcp, invoke_with_grant, pin_filesystem_server, McpAllowlist,
    McpClient, UserMcpRegistry,
};
use aether_permissions::{PermissionDecision, PermissionManager};
use serde_json::json;
use std::fs;
use tempfile::tempdir;

pub fn mcp02_fixture_ready() -> Result<(), String> {
    discover_filesystem_mcp().map_err(|e| {
        format!(
            "MCP-02 requires @modelcontextprotocol/server-filesystem: {e}. \
             Install with: npm install -g @modelcontextprotocol/server-filesystem"
        )
    })?;
    Ok(())
}

pub async fn test_mcp02_impl(conn: &rusqlite::Connection) -> Result<bool, String> {
    mcp02_fixture_ready()?;

    let paths = discover_filesystem_mcp().map_err(|e| e.to_string())?;
    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path();
    let workspace_str = workspace.to_string_lossy().to_string();
    fs::write(workspace.join("probe.txt"), "mcp02").map_err(|e| e.to_string())?;

    let mut client = McpClient::spawn_config(
        &paths.to_allowlist_entry(),
        &[workspace_str.clone()],
        &[],
    )
    .map_err(|e| e.to_string())?;
    let audit = client.list_tools().map_err(|e| e.to_string())?;

    let mut registry = UserMcpRegistry::new();
    let config = pin_filesystem_server(&paths, &audit);
    registry
        .add_server(config, &audit)
        .map_err(|e| e.to_string())?;

    let entry = registry.get("filesystem").ok_or("missing user entry")?;
    if entry.config.tools_hash_pin.as_deref() != Some(audit.tools_hash.as_str()) {
        return Err("MCP-02 pin-on-install must set tools_hash_pin".into());
    }
    if entry.context_cost_chars == 0 {
        return Err("MCP-02 context cost estimate must be non-zero".into());
    }

    let session_id = "sess-mcp-02";
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'MCP-02', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'mcp_call')",
        rusqlite::params![session_id, workspace_str],
    )
    .map_err(|e| e.to_string())?;

    let allowlist = registry.to_allowlist();
    let (result, _) = invoke_with_grant(
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
    let text = result
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("text"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "MCP-02 missing list_directory text".to_string())?;
    if !text.contains("probe.txt") {
        return Err(format!("MCP-02 list_directory missing probe.txt: {text}"));
    }

    // Simulate tools_hash change — invoke must fail until approve_update.
    let mut stale_allowlist = registry.to_allowlist();
    if let Some(server) = stale_allowlist
        .servers
        .iter_mut()
        .find(|s| s.name == "filesystem")
    {
        server.tools_hash_pin = Some("0".repeat(64));
    }
    if invoke_with_grant(
        conn,
        session_id,
        &workspace_str,
        &stale_allowlist,
        "filesystem",
        "list_directory",
        json!({ "path": workspace_str }),
        &[workspace_str.clone()],
        &[],
    )
    .is_ok()
    {
        return Err("MCP-02 expected stale tools_hash pin to block invoke".into());
    }

    let mut mutated_audit = audit.clone();
    mutated_audit.tools_hash = "f".repeat(64);
    mutated_audit.tools.push(aether_mcp::McpToolInfo {
        name: "synthetic_probe_tool".into(),
        description: "probe-only".into(),
        description_hash: "probehash".into(),
    });

    let diff = registry
        .detect_update("filesystem", &mutated_audit)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "MCP-02 expected diff when tools_hash changes".to_string())?;
    if diff.old_tools_hash == diff.new_tools_hash {
        return Err("MCP-02 diff must report hash change".into());
    }
    if !diff.added_tools.contains(&"synthetic_probe_tool".to_string()) {
        return Err(format!(
            "MCP-02 diff must list added tools, got {:?}",
            diff.added_tools
        ));
    }

    registry
        .approve_update("filesystem", &mutated_audit)
        .map_err(|e| e.to_string())?;
    let merged = registry.merge_with_curated(&McpAllowlist::resolve_filesystem().map_err(|e| e.to_string())?);
    if merged.servers.iter().filter(|s| s.name == "filesystem").count() != 1 {
        return Err("MCP-02 merge must not duplicate curated filesystem entry".into());
    }

    let approved = PermissionManager::check_file_access(conn, session_id, &workspace_str, "mcp_call")
        .map_err(|e| e.to_string())?;
    if approved != PermissionDecision::Approved {
        return Err("MCP-02 grant check failed".into());
    }

    Ok(std::env::consts::OS == "macos")
}
