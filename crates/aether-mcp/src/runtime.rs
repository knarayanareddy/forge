use crate::{McpAllowlist, McpError, McpServerConfig};
use aether_permissions::{PermissionDecision, PermissionManager};
use aether_sandbox::ProductionSandbox;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub description_hash: String,
}

#[derive(Debug, Clone)]
pub struct McpToolsAudit {
    pub tools: Vec<McpToolInfo>,
    pub tools_hash: String,
}

pub struct McpClient {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
    initialized: bool,
}

impl McpClient {
    /// Load allowlist entry, verify SHA-256 pins, and spawn the MCP server over stdio.
    pub fn spawn_verified(allowlist: &McpAllowlist, server_name: &str, extra_args: &[String]) -> Result<Self, McpError> {
        let config = allowlist.verify_and_get(server_name)?;
        Self::spawn_config(&config, extra_args)
    }

    /// Production spawn: verified MCP server wrapped by the workspace Seatbelt profile.
    pub fn spawn_verified_in_workspace(
        allowlist: &McpAllowlist,
        server_name: &str,
        extra_args: &[String],
        workspace: &Path,
    ) -> Result<Self, McpError> {
        let config = allowlist.verify_and_get(server_name)?;
        Self::spawn_config_in_workspace(&config, extra_args, workspace)
    }

    pub fn spawn_config(config: &McpServerConfig, extra_args: &[String]) -> Result<Self, McpError> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        cmd.args(extra_args);
        Self::spawn_command(config, cmd)
    }

    fn spawn_config_in_workspace(
        config: &McpServerConfig,
        extra_args: &[String],
        workspace: &Path,
    ) -> Result<Self, McpError> {
        let args: Vec<&str> = config
            .args
            .iter()
            .chain(extra_args.iter())
            .map(String::as_str)
            .collect();
        let cmd = ProductionSandbox::command(&config.command, args, workspace).map_err(|e| {
            McpError::SecurityViolation(format!(
                "Failed to prepare sandbox for MCP server '{}': {}",
                config.name, e
            ))
        })?;
        Self::spawn_command(config, cmd)
    }

    fn spawn_command(config: &McpServerConfig, mut cmd: Command) -> Result<Self, McpError> {
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn().map_err(|e| {
            McpError::SecurityViolation(format!(
                "Failed to spawn MCP server '{}': {}",
                config.name, e
            ))
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            McpError::SecurityViolation("MCP server missing stdout".into())
        })?;

        Ok(Self {
            child,
            reader: BufReader::new(stdout),
            next_id: 1,
            initialized: false,
        })
    }

    pub fn initialize(&mut self) -> Result<Value, McpError> {
        let id = self.next_id;
        self.next_id += 1;

        let params = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "aether-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let result = self.request(id, "initialize", params)?;
        self.notify("notifications/initialized", json!({}))?;
        self.initialized = true;
        Ok(result)
    }

    pub fn list_tools(&mut self) -> Result<McpToolsAudit, McpError> {
        if !self.initialized {
            self.initialize()?;
        }

        let id = self.next_id;
        self.next_id += 1;
        let result = self.request(id, "tools/list", json!({}))?;

        let tools_val = result
            .get("tools")
            .ok_or_else(|| McpError::SecurityViolation("tools/list missing tools array".into()))?;

        let tools_array = tools_val
            .as_array()
            .ok_or_else(|| McpError::SecurityViolation("tools/list tools is not array".into()))?;

        let mut tools = Vec::new();
        let mut aggregate = Sha256::new();

        for tool in tools_array {
            let name = tool
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let description = tool
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let mut desc_hasher = Sha256::new();
            desc_hasher.update(description.as_bytes());
            let description_hash = format!("{:x}", desc_hasher.finalize());

            aggregate.update(name.as_bytes());
            aggregate.update(description_hash.as_bytes());

            tools.push(McpToolInfo {
                name,
                description,
                description_hash,
            });
        }

        tools.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(McpToolsAudit {
            tools_hash: format!("{:x}", aggregate.finalize()),
            tools,
        })
    }

    pub fn call_tool(&mut self, tool_name: &str, arguments: Value) -> Result<Value, McpError> {
        if !self.initialized {
            self.initialize()?;
        }

        let id = self.next_id;
        self.next_id += 1;

        let params = json!({
            "name": tool_name,
            "arguments": arguments
        });

        self.request(id, "tools/call", params)
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, McpError> {
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.write_line(&req)?;

        loop {
            let line = self.read_line()?;
            let msg: Value = serde_json::from_str(&line)?;

            if msg.get("id").and_then(|v| v.as_u64()) == Some(id) {
                if let Some(err) = msg.get("error") {
                    return Err(McpError::SecurityViolation(format!(
                        "MCP JSON-RPC error on {}: {}",
                        method, err
                    )));
                }
                return msg
                    .get("result")
                    .cloned()
                    .ok_or_else(|| McpError::SecurityViolation(format!("MCP {} missing result", method)));
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.write_line(&msg)
    }

    fn write_line(&mut self, msg: &Value) -> Result<(), McpError> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| McpError::SecurityViolation("MCP server missing stdin".into()))?;

        let line = serde_json::to_string(msg)?;
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    fn read_line(&mut self) -> Result<String, McpError> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        Ok(line.trim().to_string())
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Grant gate + verified spawn + initialize → tools/list → tools/call.
pub fn invoke_with_grant(
    conn: &Connection,
    session_id: &str,
    workspace_path: &str,
    allowlist: &McpAllowlist,
    server_name: &str,
    tool_name: &str,
    arguments: Value,
    extra_spawn_args: &[String],
) -> Result<(Value, McpToolsAudit), McpError> {
    let start = Instant::now();

    let decision = PermissionManager::check_file_access(conn, session_id, workspace_path, "mcp_call")
        .map_err(|e| McpError::SecurityViolation(format!("Grant check failed: {}", e)))?;

    if decision != PermissionDecision::Approved {
        PermissionManager::audit_decision(
            conn,
            session_id,
            "mcp_call",
            &json!({ "server": server_name, "tool": tool_name, "path": workspace_path }).to_string(),
            &decision,
            Some(1),
            Some(start.elapsed().as_millis() as i64),
        )
        .map_err(|e| McpError::SecurityViolation(e.to_string()))?;

        return Err(McpError::SecurityViolation(format!(
            "mcp_call grant required for workspace {}",
            workspace_path
        )));
    }

    let mut client = McpClient::spawn_verified_in_workspace(
        allowlist,
        server_name,
        extra_spawn_args,
        Path::new(workspace_path),
    )?;
    let tools_audit = client.list_tools()?;

    let config = allowlist.verify_and_get(server_name)?;
    if let Some(pin) = &config.tools_hash_pin {
        if pin.starts_with("PENDING")
            || pin.starts_with("REPLACE_WITH_")
            || pin.is_empty()
        {
            return Err(McpError::SecurityViolation(format!(
                "MCP Server '{}' tools_hash pin is pending",
                server_name
            )));
        }
        if tools_audit.tools_hash != *pin {
            return Err(McpError::SecurityViolation(format!(
                "MCP tools_hash pin mismatch for '{}': expected {}, got {}",
                server_name, pin, tools_audit.tools_hash
            )));
        }
    }

    PermissionManager::audit_decision(
        conn,
        session_id,
        "mcp_tools_list",
        &json!({
            "server": server_name,
            "tools_hash": tools_audit.tools_hash,
            "tool_count": tools_audit.tools.len()
        })
        .to_string(),
        &PermissionDecision::Approved,
        Some(0),
        Some(start.elapsed().as_millis() as i64),
    )
    .map_err(|e| McpError::SecurityViolation(e.to_string()))?;

    let result = client.call_tool(tool_name, arguments)?;

    PermissionManager::audit_decision(
        conn,
        session_id,
        "mcp_call",
        &json!({ "server": server_name, "tool": tool_name, "path": workspace_path }).to_string(),
        &PermissionDecision::Approved,
        Some(0),
        Some(start.elapsed().as_millis() as i64),
    )
    .map_err(|e| McpError::SecurityViolation(e.to_string()))?;

    Ok((result, tools_audit))
}
