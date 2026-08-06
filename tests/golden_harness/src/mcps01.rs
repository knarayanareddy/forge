//! MCPS-01 — Forge as MCP server over stdio (Track E slice E.3).

use aether_mcp::ForgeMcpServer;

pub fn mcps01_fixture_ready() -> Result<(), String> {
    let server = ForgeMcpServer;
    if server.list_tools().is_empty() {
        return Err("MCPS-01 forge server must expose at least one tool".into());
    }
    Ok(())
}

pub fn test_mcps01_impl() -> Result<bool, String> {
    mcps01_fixture_ready()?;
    let server = ForgeMcpServer;

    let init_resp = server
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
        .map_err(|e| e.to_string())?
        .ok_or("MCPS-01 initialize must return a response")?;
    let init: serde_json::Value = serde_json::from_str(&init_resp)
        .map_err(|e| format!("MCPS-01 invalid initialize JSON: {e}"))?;
    if init["result"]["serverInfo"]["name"].as_str() != Some("aether-forge-mcp") {
        return Err("MCPS-01 serverInfo.name must be aether-forge-mcp".into());
    }

    let list_resp = server
        .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#)
        .map_err(|e| e.to_string())?
        .ok_or("MCPS-01 tools/list must return a response")?;
    let list: serde_json::Value = serde_json::from_str(&list_resp)
        .map_err(|e| format!("MCPS-01 invalid tools/list JSON: {e}"))?;
    let tools = list["result"]["tools"]
        .as_array()
        .ok_or("MCPS-01 tools/list missing tools array")?;
    if !tools.iter().any(|t| t["name"] == "forge_ping") {
        return Err("MCPS-01 must expose forge_ping tool".into());
    }

    let call_resp = server
        .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"forge_ping","arguments":{}}}"#)
        .map_err(|e| e.to_string())?
        .ok_or("MCPS-01 tools/call must return a response")?;
    let call_json: serde_json::Value = serde_json::from_str(&call_resp)
        .map_err(|e| format!("MCPS-01 invalid tools/call JSON: {e}"))?;
    let text = call_json["result"]["content"][0]["text"]
        .as_str()
        .ok_or("MCPS-01 forge_ping missing text content")?;
    if !text.starts_with("pong:") {
        return Err(format!("MCPS-01 forge_ping expected pong, got {text}"));
    }

    Ok(false)
}
