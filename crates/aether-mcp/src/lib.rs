mod runtime;

pub use runtime::{invoke_with_grant, McpClient, McpToolInfo, McpToolsAudit};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

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
    #[serde(default)]
    pub entry_sha256_pin: Option<String>,
    #[serde(default)]
    pub tools_hash_pin: Option<String>,
    pub default_policy: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpAllowlist {
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone)]
pub struct FilesystemMcpPaths {
    pub node: PathBuf,
    pub server_script: PathBuf,
    pub node_sha256: String,
    pub script_sha256: String,
}

impl McpAllowlist {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, McpError> {
        let content = fs::read_to_string(path)?;
        let allowlist: McpAllowlist = serde_json::from_str(&content)?;
        Ok(allowlist)
    }

    pub fn verify_and_get(&self, name: &str) -> Result<McpServerConfig, McpError> {
        for server in &self.servers {
            if server.name == name {
                if server.sha256_pin.starts_with("PENDING") || server.sha256_pin.is_empty() {
                    return Err(McpError::SecurityViolation(format!(
                        "MCP Server '{}' has unverified pending digest pin ('{}'). Execution blocked.",
                        name, server.sha256_pin
                    )));
                }

                verify_file_hash(Path::new(&server.command), &server.sha256_pin, "command")?;

                if let Some(entry_pin) = &server.entry_sha256_pin {
                    if entry_pin.starts_with("PENDING") || entry_pin.is_empty() {
                        return Err(McpError::SecurityViolation(format!(
                            "MCP Server '{}' entry script pin is pending",
                            name
                        )));
                    }
                    let entry = server
                        .args
                        .first()
                        .ok_or_else(|| {
                            McpError::SecurityViolation(format!(
                                "MCP Server '{}' missing entry script arg",
                                name
                            ))
                        })?;
                    verify_file_hash(Path::new(entry), entry_pin, "entry script")?;
                }

                return Ok(server.clone());
            }
        }
        Err(McpError::SecurityViolation(format!(
            "MCP Server '{}' is not in the curated allowlist",
            name
        )))
    }
}

fn verify_file_hash(path: &Path, expected: &str, label: &str) -> Result<(), McpError> {
    if !path.exists() || !path.is_file() {
        return Err(McpError::SecurityViolation(format!(
            "MCP {} path missing: {}",
            label,
            path.display()
        )));
    }

    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let computed_hash = format!("{:x}", hasher.finalize());

    if computed_hash != expected {
        return Err(McpError::SecurityViolation(format!(
            "MCP {} hash mismatch for {}: computed {}, expected {}",
            label,
            path.display(),
            computed_hash,
            expected
        )));
    }

    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, McpError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Resolve node + @modelcontextprotocol/server-filesystem for Darwin harness/runtime.
pub fn discover_filesystem_mcp() -> Result<FilesystemMcpPaths, McpError> {
    let node = std::env::var("AETHER_MCP_NODE")
        .ok()
        .map(PathBuf::from)
        .or_else(which_node)
        .ok_or_else(|| {
            McpError::SecurityViolation(
                "node not found — install Node.js or set AETHER_MCP_NODE".into(),
            )
        })?;

    let server_script = std::env::var("AETHER_MCP_FILESYSTEM_SCRIPT")
        .ok()
        .map(PathBuf::from)
        .or_else(discover_server_script)
        .ok_or_else(|| {
            McpError::SecurityViolation(
                "@modelcontextprotocol/server-filesystem not installed — run: npm install -g @modelcontextprotocol/server-filesystem (or set AETHER_MCP_FILESYSTEM_SCRIPT)".into(),
            )
        })?;

    if !server_script.exists() {
        return Err(McpError::SecurityViolation(format!(
            "MCP filesystem server script missing: {}",
            server_script.display()
        )));
    }

    Ok(FilesystemMcpPaths {
        node_sha256: sha256_file(&node)?,
        script_sha256: sha256_file(&server_script)?,
        node,
        server_script,
    })
}

fn which_node() -> Option<PathBuf> {
    let output = Command::new("which").arg("node").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn discover_server_script() -> Option<PathBuf> {
    for candidate in candidate_server_scripts() {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let output = Command::new("npm")
        .args(["root", "-g"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        return None;
    }
    let script = PathBuf::from(root)
        .join("@modelcontextprotocol/server-filesystem/dist/index.js");
    if script.exists() {
        Some(script)
    } else {
        None
    }
}

fn candidate_server_scripts() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/lib/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"),
        PathBuf::from("/usr/local/lib/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"),
    ]
}

impl FilesystemMcpPaths {
    pub fn to_allowlist_entry(&self) -> McpServerConfig {
        McpServerConfig {
            name: "filesystem".into(),
            version: "2026.7.10".into(),
            command: self.node.to_string_lossy().into_owned(),
            args: vec![self.server_script.to_string_lossy().into_owned()],
            sha256_pin: self.node_sha256.clone(),
            entry_sha256_pin: Some(self.script_sha256.clone()),
            tools_hash_pin: None,
            default_policy: "prompt_always".into(),
        }
    }
}
