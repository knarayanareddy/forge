use aether_mcp::{discover_filesystem_mcp, McpAllowlist, McpClient};

#[test]
fn print_discovered_tools_hash() {
    let paths = discover_filesystem_mcp().expect("discover");
    let allowlist = McpAllowlist {
        servers: vec![paths.to_allowlist_entry()],
    };
    let config = allowlist.verify_and_get("filesystem").expect("verify");
    let mut client = McpClient::spawn_config(&config, &["/tmp".to_string()]).expect("spawn");
    let audit = client.list_tools().expect("list");
    eprintln!("tools_hash={}", audit.tools_hash);
    assert_eq!(audit.tools_hash.len(), 64);
}
