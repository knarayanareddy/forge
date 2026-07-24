use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;
use sha2::{Sha256, Digest};

#[derive(Error, Debug)]
pub enum McpError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub version: String,
    pub command: String,
    pub args: Vec<String>,
    pub sha256_pin: String,
    pub default_policy: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpAllowlist {
    pub servers: Vec<McpServerConfig>,
}

impl McpAllowlist {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, McpError> {
        let content = fs::read_to_string(path)?;
        let allowlist: McpAllowlist = serde_json::from_str(&content)?;
        Ok(allowlist)
    }

    /// Verifies server allowlist membership, rejects unpinned/pending digests,
    /// and optionally verifies binary file SHA-256 hash if the binary exists on disk.
    pub fn verify_and_get(&self, name: &str) -> Result<McpServerConfig, McpError> {
        for server in &self.servers {
            if server.name == name {
                // Fail-closed rule: reject pending or placeholder hash pins at runtime
                if server.sha256_pin.starts_with("PENDING") || server.sha256_pin.is_empty() {
                    return Err(McpError::SecurityViolation(format!(
                        "MCP Server '{}' has unverified pending digest pin ('{}'). Execution blocked.",
                        name, server.sha256_pin
                    )));
                }

                // If command binary exists on disk, verify its SHA-256 hash matches the pin
                let cmd_path = Path::new(&server.command);
                if cmd_path.exists() && cmd_path.is_file() {
                    let mut file = fs::File::open(cmd_path)?;
                    let mut hasher = Sha256::new();
                    std::io::copy(&mut file, &mut hasher)?;
                    let computed_hash = format!("{:x}", hasher.finalize());

                    if computed_hash != server.sha256_pin {
                        return Err(McpError::SecurityViolation(format!(
                            "MCP Server '{}' binary hash mismatch! Computed: {}, Expected: {}",
                            name, computed_hash, server.sha256_pin
                        )));
                    }
                }

                return Ok(server.clone());
            }
        }
        Err(McpError::SecurityViolation(format!("MCP Server '{}' is not in the curated allowlist", name)))
    }
}
