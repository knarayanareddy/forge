//! User-inspectable memory (Phase 11 slice 11.11 / MEM-03).

use rusqlite::Result;
use serde::{Deserialize, Serialize};

use crate::{Database, GraphNode};

/// A single user-visible memory fact with provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMemoryFact {
    pub id: String,
    pub entity_type: String,
    pub canonical_name: String,
    pub properties_json: String,
    pub source_uri: String,
    pub recorded_at: String,
}

impl From<GraphNode> for UserMemoryFact {
    fn from(node: GraphNode) -> Self {
        Self {
            id: node.id,
            entity_type: node.entity_type,
            canonical_name: node.canonical_name,
            properties_json: node.properties_json,
            source_uri: node.source_uri,
            recorded_at: node.recorded_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMemoryExport {
    pub schema_version: u32,
    pub session_id: String,
    pub facts: Vec<UserMemoryFact>,
}

impl Database {
    /// List active graph nodes as user-inspectable facts ("what I know about you").
    pub fn list_user_memory_facts(&self, session_id: &str) -> Result<Vec<UserMemoryFact>> {
        Ok(self
            .get_active_graph_nodes(session_id, None)?
            .into_iter()
            .map(UserMemoryFact::from)
            .collect())
    }

    /// Edit canonical name and properties on an active fact.
    pub fn update_user_memory_fact(
        &self,
        node_id: &str,
        canonical_name: &str,
        properties_json: &str,
    ) -> Result<()> {
        let node = self
            .get_graph_node(node_id)?
            .ok_or_else(|| rusqlite::Error::InvalidParameterName(node_id.into()))?;
        if node.superseded_by.is_some() || node.valid_to.is_some() {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "memory fact '{node_id}' is not active"
            )));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE graph_nodes SET canonical_name = ?1, properties_json = ?2 WHERE id = ?3",
            rusqlite::params![canonical_name, properties_json, node_id],
        )?;
        Ok(())
    }

    /// Soft-delete a fact scoped to a session.
    pub fn delete_user_memory_fact(&self, session_id: &str, node_id: &str) -> Result<()> {
        let node = self
            .get_graph_node(node_id)?
            .ok_or_else(|| rusqlite::Error::InvalidParameterName(node_id.into()))?;
        if node.session_id != session_id {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "memory fact '{node_id}' does not belong to session '{session_id}'"
            )));
        }
        if node.valid_to.is_some() {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "memory fact '{node_id}' already deleted"
            )));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE graph_nodes SET valid_to = datetime('now') WHERE id = ?1 AND valid_to IS NULL",
            rusqlite::params![node_id],
        )?;
        Ok(())
    }

    /// Export all active facts for a session as pretty JSON.
    pub fn export_user_memory_json(&self, session_id: &str) -> Result<String> {
        let export = UserMemoryExport {
            schema_version: 1,
            session_id: session_id.to_string(),
            facts: self.list_user_memory_facts(session_id)?,
        };
        serde_json::to_string_pretty(&export).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityType, NewGraphNode};

    #[test]
    fn mem03_list_edit_delete_export() {
        let db = Database::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO sessions (id, title, status) VALUES ('s1', 't', 'active')",
                [],
            )
            .unwrap();
        db.insert_graph_node(NewGraphNode {
            id: "fact-1",
            session_id: "s1",
            entity_type: EntityType::Person,
            canonical_name: "Alice",
            aliases_json: "[]",
            properties_json: r#"{"role":"engineer"}"#,
            source_uri: "memory://s1/turn/1",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        db.update_user_memory_fact("fact-1", "Alice Smith", r#"{"role":"lead"}"#)
            .unwrap();
        db.delete_user_memory_fact("s1", "fact-1").unwrap();
        assert!(db.list_user_memory_facts("s1").unwrap().is_empty());
    }
}
