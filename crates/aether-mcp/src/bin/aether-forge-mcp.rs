//! Stdio MCP server exposing Forge as a callable tool host (MCPS-01).

fn main() {
    if let Err(e) = aether_mcp::run_forge_mcp_stdio() {
        eprintln!("aether-forge-mcp: {e}");
        std::process::exit(1);
    }
}
