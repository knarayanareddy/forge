//! User-addable MCP server registry (Phase 10 slice 10.11 / MCP-02).

use crate::{McpAllowlist, McpError, McpServerConfig, McpToolsAudit};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolChange {
    pub name: String,
    pub old_description_hash: Option<String>,
    pub new_description_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolsDiff {
    pub server_name: String,
    pub old_tools_hash: String,
    pub new_tools_hash: String,
    pub added_tools: Vec<String>,
    pub removed_tools: Vec<String>,
    pub changed_tools: Vec<McpToolChange>,
    pub context_cost_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMcpEntry {
    pub config: McpServerConfig,
    pub context_cost_chars: usize,
    pub pinned_tools: Vec<(String, String)>,
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

    fn context_cost(audit: &McpToolsAudit) -> usize {
        audit.tools.iter().map(|t| t.description.len()).sum()
    }

    pub fn add_server(
        &mut self,
        mut config: McpServerConfig,
        audit: &McpToolsAudit,
    ) -> Result<(), McpError> {
        if config.tools_hash_pin.is_none() {
            config.tools_hash_pin = Some(audit.tools_hash.clone());
        }
        let pinned_tools = audit
            .tools
            .iter()
            .map(|t| (t.name.clone(), t.description_hash.clone()))
            .collect();
        self.entries.insert(
            config.name.clone(),
            UserMcpEntry {
                config,
                context_cost_chars: Self::context_cost(audit),
                pinned_tools,
            },
        );
        Ok(())
    }

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

        let old_map: HashMap<_, _> = entry
            .pinned_tools
            .iter()
            .map(|(n, h)| (n.as_str(), h.as_str()))
            .collect();
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        for tool in &audit.tools {
            match old_map.get(tool.name.as_str()) {
                None => added.push(tool.name.clone()),
                Some(old_desc) if *old_desc != tool.description_hash => {
                    changed.push(McpToolChange {
                        name: tool.name.clone(),
                        old_description_hash: Some((*old_desc).to_string()),
                        new_description_hash: tool.description_hash.clone(),
                    });
                }
                _ => {}
            }
        }
        for (name, _) in &entry.pinned_tools {
            if !audit.tools.iter().any(|t| t.name == *name) {
                removed.push(name.clone());
            }
        }

        Ok(Some(McpToolsDiff {
            server_name: name.to_string(),
            old_tools_hash: old_hash.clone(),
            new_tools_hash: audit.tools_hash.clone(),
            added_tools: added,
            removed_tools: removed,
            changed_tools: changed,
            context_cost_chars: Self::context_cost(audit),
        }))
    }

    pub fn approve_update(&mut self, name: &str, audit: &McpToolsAudit) -> Result<(), McpError> {
        let entry = self.entries.get_mut(name).ok_or_else(|| {
            McpError::SecurityViolation(format!("user MCP server '{name}' is not installed"))
        })?;
        entry.config.tools_hash_pin = Some(audit.tools_hash.clone());
        entry.context_cost_chars = Self::context_cost(audit);
        entry.pinned_tools = audit
            .tools
            .iter()
            .map(|t| (t.name.clone(), t.description_hash.clone()))
            .collect();
        Ok(())
    }

    pub fn to_allowlist(&self) -> McpAllowlist {
        McpAllowlist {
            servers: self.entries.values().map(|e| e.config.clone()).collect(),
        }
    }

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

pub fn pin_filesystem_server(
    paths: &crate::FilesystemMcpPaths,
    audit: &McpToolsAudit,
) -> McpServerConfig {
    let mut entry = paths.to_allowlist_entry();
    entry.tools_hash_pin = Some(audit.tools_hash.clone());
    entry
}
