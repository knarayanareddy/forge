//! Forge-as-MCP-server over stdio (Track E slice E.3 / MCPS-01).

use crate::McpError;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
const FORGE_PING_TOOL: &str = "forge_ping";

/// In-process MCP server stub for harness and stdio binary.
#[derive(Debug, Clone, Default)]
pub struct ForgeMcpServer;

impl ForgeMcpServer {
    pub fn list_tools(&self) -> Vec<String> {
        vec![FORGE_PING_TOOL.into()]
    }

    /// Handle one JSON-RPC line; returns `None` for notifications.
    pub fn handle_line(&self, line: &str) -> Result<Option<String>, McpError> {
        let msg: Value = serde_json::from_str(line)?;
        if msg.get("method").and_then(|v| v.as_str()) == Some("notifications/initialized") {
            return Ok(None);
        }
        let id = msg.get("id").cloned();
        let method = msg
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = handle_request(method, &params)?;
        if id.is_none() {
            return Ok(None);
        }
        Ok(Some(
            serde_json::to_string(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))?,
        ))
    }
}

/// Harness/runtime entry for in-process MCPS-01 probes.
pub fn forge_mcp_handle_request(method: &str, params: &Value) -> Result<Value, McpError> {
    handle_request(method, params)
}

fn handle_request(method: &str, params: &Value) -> Result<Value, McpError> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "aether-forge-mcp", "version": env!("CARGO_PKG_VERSION") }
        })),
        "tools/list" => Ok(json!({
            "tools": [{
                "name": FORGE_PING_TOOL,
                "description": "Health probe — returns forge pong",
                "inputSchema": { "type": "object", "properties": {} }
            }]
        })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if name != FORGE_PING_TOOL {
                return Err(McpError::SecurityViolation(format!(
                    "unknown tool '{name}'"
                )));
            }
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": "pong:forge"
                }],
                "isError": false
            }))
        }
        _ => Err(McpError::SecurityViolation(format!(
            "unsupported method '{method}'"
        ))),
    }
}

/// Run the minimal Forge MCP server loop on stdio until EOF.
pub fn run_forge_mcp_stdio() -> std::io::Result<()> {
    let server = ForgeMcpServer;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match server.handle_line(&line) {
            Ok(Some(resp)) => {
                writeln!(stdout, "{resp}")?;
            }
            Ok(None) => {}
            Err(e) => {
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32603, "message": e.to_string() }
                });
                writeln!(stdout, "{}", serde_json::to_string(&err).unwrap())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_ping_tool_call() {
        let server = ForgeMcpServer;
        let resp = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"forge_ping","arguments":{}}}"#)
            .unwrap()
            .unwrap();
        assert!(resp.contains("pong:"));
    }
}
