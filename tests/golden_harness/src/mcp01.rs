use aether_mcp::{McpAllowlist, McpError};
use aether_permissions::{PermissionManager, PermissionDecision};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::tempdir;
use sha2::{Sha256, Digest};

pub async fn test_mcp_01_impl(conn: &rusqlite::Connection) -> Result<(), String> {
    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace_path = tmp.path();
    let workspace_str = workspace_path.to_string_lossy().to_string();

    // 1. Create a dummy server script to simulate a secure MCP server binary
    let dummy_server_path = workspace_path.join("mock_mcp_filesystem.sh");
    fs::write(&dummy_server_path, "#!/bin/bash\necho '{\"jsonrpc\": \"2.0\", \"result\": {\"files\": [\"file1.txt\", \"file2.txt\"]}, \"id\": 1}'")
        .map_err(|e| e.to_string())?;
    
    // Set executable permission
    let mut perms = fs::metadata(&dummy_server_path).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&dummy_server_path, perms).map_err(|e| e.to_string())?;

    // Compute exact sha256 hash of the dummy binary
    let mut file = fs::File::open(&dummy_server_path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| e.to_string())?;
    let valid_pin = format!("{:x}", hasher.finalize());

    // 2. Test default allowlist with PENDING pin (must fail-closed)
    let default_allowlist_path = Path::new("mcp_allowlist.json");
    if default_allowlist_path.exists() {
        let allowlist = McpAllowlist::load_from_file(default_allowlist_path).map_err(|e| e.to_string())?;
        let res = allowlist.verify_and_get("filesystem");
        match res {
            Err(McpError::SecurityViolation(_)) => {
                // Expected fail-closed behavior on PENDING pin
            }
            _ => {
                return Err("Security violation expected when verifying server with PENDING pin".into());
            }
        }
    }

    // 3. Test programmatic allowlist with valid sha256 pin
    let custom_allowlist_json = format!(
        r#"{{
          "servers": [
            {{
              "name": "filesystem",
              "version": "1.0.0",
              "command": "{}",
              "args": [],
              "sha256_pin": "{}",
              "default_policy": "prompt_always"
            }}
          ]
        }}"#,
        dummy_server_path.to_string_lossy(),
        valid_pin
    );

    let custom_allowlist_path = workspace_path.join("custom_allowlist.json");
    fs::write(&custom_allowlist_path, custom_allowlist_json).map_err(|e| e.to_string())?;

    let verified_allowlist = McpAllowlist::load_from_file(&custom_allowlist_path).map_err(|e| e.to_string())?;
    let server_config = verified_allowlist.verify_and_get("filesystem").map_err(|e| e.to_string())?;
    
    if server_config.name != "filesystem" {
        return Err("Failed to retrieve verified filesystem MCP server config".into());
    }

    // 4. Test capability grant enforcement for MCP queries under authorized path
    let session_id = "sess-mcp-01";
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'MCP Session', 'active')",
        rusqlite::params![session_id],
    ).map_err(|e| e.to_string())?;

    // Ungranted path query must be denied
    let decision_denied = PermissionManager::check_file_access(conn, session_id, &workspace_str, "mcp_call")
        .map_err(|e| e.to_string())?;
    if decision_denied != PermissionDecision::Denied {
        return Err("Expected MCP call to ungranted workspace to be denied".into());
    }

    // Grant capability
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'mcp_call')",
        rusqlite::params![session_id, workspace_str],
    ).map_err(|e| e.to_string())?;

    let decision_approved = PermissionManager::check_file_access(conn, session_id, &workspace_str, "mcp_call")
        .map_err(|e| e.to_string())?;
    if decision_approved != PermissionDecision::Approved {
        return Err("Expected MCP call to granted workspace to be approved".into());
    }

    // 5. Execute mock MCP server and validate valid JSON response
    let output = std::process::Command::new(&server_config.command)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err("Mock MCP server execution failed".into());
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let json_val: serde_json::Value = serde_json::from_str(&stdout_str).map_err(|e| e.to_string())?;
    
    if json_val["jsonrpc"] != "2.0" || json_val["result"]["files"].is_array() == false {
        return Err("Invalid MCP JSON-RPC response format".into());
    }

    Ok(())
}
