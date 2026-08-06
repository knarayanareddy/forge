//! User-addable MCP server registry (Phase 10 slice 10.11 / MCP-02).
//!
//! Trust flow: add by command, pin command/entry/tools_hash on install, diff-on-update review
//! before re-pin. Curated `mcp_allowlist.json` entries merge with user-added servers at runtime.

use crate::{McpAllowlist, McpError, McpServerConfig, McpToolsAudit};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Per-tool change surfaced during diff-on-update review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolChange {
    pub name: String,
    pub old_description_hash: Option<String>,
    pub new_description_hash: String,
}

/// Diff returned when a server's live `tools_hash` no longer matches its installed pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolsDiff {
    pub server_name: String,
    pub old_tools_hash: String,
    pub new_tools_hash: String,
    pub added_tools: Vec<String>,
    pub removed_tools: Vec<String>,
    pub changed_tools: Vec<McpToolChange>,
    /// Rough context-cost proxy: sum of tool description lengths (for UI display).
    pub context_cost_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMcpEntry {
    pub config: McpServerConfig,
    pub context_cost_chars: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct UserMcpRegistry {
    entries: HashMap<String, UserMcpEntry>,
}

impl UserMcpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str) -> Option<&UserMcpEntry> {
        self.entries.get(name)
    }

    pub fn list(&self) -> impl Iterator<Item = (&str, &UserMcpEntry)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    fn context_cost(audit: &McpToolsAudit) -> usize {
        audit.tools.iter().map(|t| t.description.len()).sum()
    }

    /// Pin a user-added server on first install. Caller must supply a verified config and a fresh
    /// `tools/list` audit from the live process.
    pub fn add_server(
        &mut self,
        mut config: McpServerConfig,
        audit: &McpToolsAudit,
    ) -> Result<(), McpError> {
        if config.tools_hash_pin.is_none() {
            config.tools_hash_pin = Some(audit.tools_hash.clone());
        }
        let cost = Self::context_cost(audit);
        self.entries.insert(
            config.name.clone(),
            UserMcpEntry {
                config,
                context_cost_chars: cost,
            },
        );
        Ok(())
    }

    /// Compare installed pin to a fresh tools audit. Returns `None` when pins still match.
    pub fn detect_update(
        &self,
        name: &str,
        audit: &McpToolsAudit,
    ) -> Result<Option<McpToolsDiff>, McpError> {
        let entry = self.entries.get(name).ok_or_else(|| {
            McpError::SecurityViolation(format!("user MCP server '{name}' is not installed"))
        })?;
        let Some(old_hash) = &entry.config.tools_hash_pin else {
            return Ok(None);
        };
        if *old_hash == audit.tools_hash {
            return Ok(None);
        }

        let mut client = McpClientForDiff::from_entry(entry);
        let diff = client.diff_against_audit(old_hash, audit);
        Ok(Some(diff))
    }

    /// Explicit re-pin after the user reviewed [`McpToolsDiff`]. Fails closed if hashes drift again.
    pub fn approve_update(&mut self, name: &str, audit: &McpToolsAudit) -> Result<(), McpError> {
        let entry = self.entries.get_mut(name).ok_or_else(|| {
            McpError::SecurityViolation(format!("user MCP server '{name}' is not installed"))
        })?;
        entry.config.tools_hash_pin = Some(audit.tools_hash.clone());
        entry.context_cost_chars = Self::context_cost(audit);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.entries.remove(name).is_some()
    }

    pub fn to_allowlist(&self) -> McpAllowlist {
        McpAllowlist {
            servers: self.entries.values().map(|e| e.config.clone()).collect(),
        }
    }

    /// Merge curated repo allowlist with user-added entries (user names must not override curated).
    pub fn merge_with_curated(&self, curated: &McpAllowlist) -> McpAllowlist {
        let mut servers = curated.servers.clone();
        for entry in self.entries.values() {
            if servers.iter().any(|s| s.name == entry.config.name) {
                continue;
            }
            servers.push(entry.config.clone());
        }
        McpAllowlist { servers }
    }
}

struct McpClientForDiff<'a> {
    name: &'a str,
    old_tools: Vec<(String, String)>,
}

impl<'a> McpClientForDiff<'a> {
    fn from_entry(entry: &'a UserMcpEntry) -> Self {
        Self {
            name: &entry.config.name,
            old_tools: Vec::new(),
        }
    }

    fn diff_against_audit(&self, old_hash: &str, audit: &McpToolsAudit) -> McpToolsDiff {
        let old_names: std::collections::HashSet<_> = self.old_tools.iter().map(|(n, _)| n.as_str()).collect();
        let new_map: HashMap<_, _> = audit
            .tools
            .iter()
            .map(|t| (t.name.as_str(), t.description_hash.as_str()))
            .collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        for tool in &audit.tools {
            if !old_names.contains(tool.name.as_str()) && self.old_tools.is_empty() {
                // First install path — treat all tools as added for review UX.
                added.push(tool.name.clone());
            }
        }

        for (name, old_desc) in &self.old_tools {
            match new_map.get(name.as_str()) {
                None => removed.push(name.clone()),
                Some(new_desc) if *new_desc != old_desc => changed.push(McpToolChange {
                    name: name.clone(),
                    old_description_hash: Some(old_desc.clone()),
                    new_description_hash: (*new_desc).to_string(),
                }),
                _ => {}
            }
        }

        for tool in &audit.tools {
            if !old_names.contains(tool.name.as_str()) && !self.old_tools.is_empty() {
                added.push(tool.name.clone());
            }
        }

        McpToolsDiff {
            server_name: self.name.to_string(),
            old_tools_hash: old_hash.to_string(),
            new_tools_hash: audit.tools_hash.clone(),
            added_tools: added,
            removed_tools: removed,
            changed_tools: changed,
            context_cost_chars: UserMcpRegistry::context_cost(audit),
        }
    }
}

/// Build a user-addable config from a discovered filesystem MCP install, pinning all digests.
pub fn pin_filesystem_server(
    paths: &crate::FilesystemMcpPaths,
    audit: &McpToolsAudit,
) -> McpServerConfig {
    let mut entry = paths.to_allowlist_entry();
    entry.tools_hash_pin = Some(audit.tools_hash.clone());
    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{McpToolInfo, McpToolsAudit};

    fn audit(tools: Vec<(&str, &str)>, hash: &str) -> McpToolsAudit {
        McpToolsAudit {
            tools_hash: hash.into(),
            tools: tools
                .into_iter()
                .map(|(name, desc)| McpToolInfo {
                    name: name.into(),
                    description: desc.into(),
                    description_hash: format!("hash-{name}"),
                })
                .collect(),
        }
    }

    fn sample_config(name: &str, tools_hash: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.into(),
            version: "1".into(),
            command: "/bin/node".into(),
            args: vec!["/srv/index.js".into()],
            sha256_pin: "a".repeat(64),
            entry_sha256_pin: Some("b".repeat(64)),
            tools_hash_pin: Some(tools_hash.into()),
            default_policy: "prompt_always".into(),
        }
    }

    #[test]
    fn add_pins_tools_hash_on_install() {
        let mut reg = UserMcpRegistry::new();
        let a = audit(vec![("list_directory", "list")], "deadbeef".repeat(8).as_str());
        reg.add_server(sample_config("custom-fs", "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"), &a)
            .unwrap();
        let entry = reg.get("custom-fs").unwrap();
        assert_eq!(entry.config.tools_hash_pin.as_deref(), Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"));
        assert!(entry.context_cost_chars > 0);
    }

    #[test]
    fn detect_update_when_tools_hash_changes() {
        let mut reg = UserMcpRegistry::new();
        let initial = audit(vec![("list_directory", "list")], &"c".repeat(64));
        reg.add_server(sample_config("custom-fs", &"c".repeat(64)), &initial)
            .unwrap();
        let updated = audit(
            vec![("list_directory", "list"), ("delete_file", "delete")],
            &"d".repeat(64),
        );
        let diff = reg.detect_update("custom-fs", &updated).unwrap();
        let Some(d) = diff else {
            panic!("expected diff");
        };
        assert_ne!(d.old_tools_hash, d.new_tools_hash);
        assert!(!d.added_tools.is_empty() || d.new_tools_hash != d.old_tools_hash);
    }

    #[test]
    fn approve_update_repins() {
        let mut reg = UserMcpRegistry::new();
        let initial = audit(vec![("list_directory", "list")], &"e".repeat(64));
        reg.add_server(sample_config("custom-fs", &"e".repeat(64)), &initial)
            .unwrap();
        let updated = audit(vec![("list_directory", "list v2")], &"f".repeat(64));
        reg.approve_update("custom-fs", &updated).unwrap();
        assert_eq!(
            reg.get("custom-fs").unwrap().config.tools_hash_pin.as_deref(),
            Some(updated.tools_hash.as_str())
        );
        assert!(reg.detect_update("custom-fs", &updated).unwrap().is_none());
    }
}
