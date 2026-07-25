use rusqlite::{params, Result};
use serde::{Deserialize, Serialize};

use crate::Database;

/// Runtime query routing policy from `query_policy` (schema zone).
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPolicy {
    pub policy_name: String,
    pub rrf_k: f64,
    pub graph_hop_depth: i32,
    pub fts_weight: f64,
    pub vec_weight: f64,
    pub graph_weight: f64,
    pub max_graph_expansion: i32,
}

/// Typed entity kinds stored in `graph_nodes.entity_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Person,
    Project,
    Concept,
    File,
    Tool,
    Event,
    Other,
}

impl EntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Project => "project",
            Self::Concept => "concept",
            Self::File => "file",
            Self::Tool => "tool",
            Self::Event => "event",
            Self::Other => "other",
        }
    }
}

/// Typed relation kinds stored in `graph_edges.relation_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    RelatedTo,
    PartOf,
    AuthoredBy,
    DependsOn,
    LocatedIn,
    Implements,
    Contradicts,
    Other,
}

impl RelationType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RelatedTo => "related_to",
            Self::PartOf => "part_of",
            Self::AuthoredBy => "authored_by",
            Self::DependsOn => "depends_on",
            Self::LocatedIn => "located_in",
            Self::Implements => "implements",
            Self::Contradicts => "contradicts",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub session_id: String,
    pub entity_type: String,
    pub canonical_name: String,
    pub aliases_json: String,
    pub properties_json: String,
    pub source_uri: String,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub recorded_at: String,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    pub id: i64,
    pub session_id: String,
    pub src_node_id: String,
    pub dst_node_id: String,
    pub relation_type: String,
    pub weight: f64,
    pub evidence_text: String,
    pub source_uri: String,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub recorded_at: String,
}

/// Neighbor returned by 1-hop traversal (edge + destination node name).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNeighbor {
    pub edge: GraphEdge,
    pub dst_name: String,
}

#[derive(Debug, Clone)]
pub struct NewGraphNode<'a> {
    pub id: &'a str,
    pub session_id: &'a str,
    pub entity_type: EntityType,
    pub canonical_name: &'a str,
    pub aliases_json: &'a str,
    pub properties_json: &'a str,
    pub source_uri: &'a str,
    pub valid_from: Option<&'a str>,
    pub valid_to: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct NewGraphEdge<'a> {
    pub session_id: &'a str,
    pub src_node_id: &'a str,
    pub dst_node_id: &'a str,
    pub relation_type: RelationType,
    pub weight: f64,
    pub evidence_text: &'a str,
    pub source_uri: &'a str,
    pub valid_from: Option<&'a str>,
    pub valid_to: Option<&'a str>,
}

impl Database {
    pub fn insert_graph_node(&self, node: NewGraphNode<'_>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO graph_nodes (
                id, session_id, entity_type, canonical_name,
                aliases_json, properties_json, source_uri,
                valid_from, valid_to
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                COALESCE(?8, CURRENT_TIMESTAMP),
                ?9)",
            params![
                node.id,
                node.session_id,
                node.entity_type.as_str(),
                node.canonical_name,
                node.aliases_json,
                node.properties_json,
                node.source_uri,
                node.valid_from,
                node.valid_to,
            ],
        )?;
        Ok(())
    }

    pub fn insert_graph_edge(&self, edge: NewGraphEdge<'_>) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO graph_edges (
                session_id, src_node_id, dst_node_id, relation_type,
                weight, evidence_text, source_uri, valid_from, valid_to
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                COALESCE(?8, CURRENT_TIMESTAMP),
                ?9)",
            params![
                edge.session_id,
                edge.src_node_id,
                edge.dst_node_id,
                edge.relation_type.as_str(),
                edge.weight,
                edge.evidence_text,
                edge.source_uri,
                edge.valid_from,
                edge.valid_to,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn link_graph_chunk(&self, chunk_id: &str, node_id: &str, link_confidence: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO graph_chunk_links (chunk_id, node_id, link_confidence)
             VALUES (?1, ?2, ?3)",
            params![chunk_id, node_id, link_confidence],
        )?;
        Ok(())
    }

    pub fn get_graph_node(&self, node_id: &str) -> Result<Option<GraphNode>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, entity_type, canonical_name, aliases_json,
                    properties_json, source_uri, valid_from, valid_to,
                    recorded_at, superseded_by
             FROM graph_nodes WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![node_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_graph_node(row)?))
        } else {
            Ok(None)
        }
    }

    /// Active nodes at query time `as_of` (defaults to SQLite `datetime('now')`).
    pub fn get_active_graph_nodes(&self, session_id: &str, as_of: Option<&str>) -> Result<Vec<GraphNode>> {
        let conn = self.conn.lock().unwrap();
        let (sql, query_params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(ts) = as_of {
            (
                "SELECT id, session_id, entity_type, canonical_name, aliases_json,
                        properties_json, source_uri, valid_from, valid_to,
                        recorded_at, superseded_by
                 FROM graph_nodes
                 WHERE session_id = ?1
                   AND valid_from <= ?2
                   AND (valid_to IS NULL OR valid_to > ?2)
                   AND superseded_by IS NULL
                 ORDER BY canonical_name",
                vec![
                    Box::new(session_id.to_string()),
                    Box::new(ts.to_string()),
                ],
            )
        } else {
            (
                "SELECT id, session_id, entity_type, canonical_name, aliases_json,
                        properties_json, source_uri, valid_from, valid_to,
                        recorded_at, superseded_by
                 FROM graph_nodes
                 WHERE session_id = ?1
                   AND valid_from <= datetime('now')
                   AND (valid_to IS NULL OR valid_to > datetime('now'))
                   AND superseded_by IS NULL
                 ORDER BY canonical_name",
                vec![Box::new(session_id.to_string())],
            )
        };

        let mut stmt = conn.prepare(sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            query_params.iter().map(|p| p.as_ref()).collect();
        let mut rows = stmt.query(param_refs.as_slice())?;

        let mut nodes = Vec::new();
        while let Some(row) = rows.next()? {
            nodes.push(row_to_graph_node(row)?);
        }
        Ok(nodes)
    }

    /// 1-hop neighbors from seed node ids, filtered by bi-temporal validity at `as_of`.
    pub fn get_one_hop_neighbors(
        &self,
        session_id: &str,
        src_node_ids: &[&str],
        as_of: Option<&str>,
    ) -> Result<Vec<GraphNeighbor>> {
        if src_node_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: String = std::iter::repeat("?")
            .take(src_node_ids.len())
            .collect::<Vec<_>>()
            .join(", ");

        let conn = self.conn.lock().unwrap();

        let sql = format!(
            "SELECT DISTINCT e.id, e.session_id, e.src_node_id, e.dst_node_id,
                    e.relation_type, e.weight, e.evidence_text, e.source_uri,
                    e.valid_from, e.valid_to, e.recorded_at,
                    n.canonical_name AS dst_name
             FROM graph_edges e
             JOIN graph_nodes n ON n.id = e.dst_node_id
             WHERE e.session_id = ?1
               AND e.src_node_id IN ({placeholders})
               AND e.valid_from <= {as_of_expr}
               AND (e.valid_to IS NULL OR e.valid_to > {as_of_expr})
               AND n.valid_from <= {as_of_expr}
               AND (n.valid_to IS NULL OR n.valid_to > {as_of_expr})
               AND n.superseded_by IS NULL
             ORDER BY e.weight DESC, n.canonical_name",
            as_of_expr = if as_of.is_some() { "?2" } else { "datetime('now')" },
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut query_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        query_params.push(Box::new(session_id.to_string()));
        if let Some(ts) = as_of {
            query_params.push(Box::new(ts.to_string()));
        }
        for id in src_node_ids {
            query_params.push(Box::new(id.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            query_params.iter().map(|p| p.as_ref()).collect();

        let mut rows = stmt.query(param_refs.as_slice())?;
        let mut neighbors = Vec::new();
        while let Some(row) = rows.next()? {
            neighbors.push(GraphNeighbor {
                edge: GraphEdge {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    src_node_id: row.get(2)?,
                    dst_node_id: row.get(3)?,
                    relation_type: row.get(4)?,
                    weight: row.get(5)?,
                    evidence_text: row.get(6)?,
                    source_uri: row.get(7)?,
                    valid_from: row.get(8)?,
                    valid_to: row.get(9)?,
                    recorded_at: row.get(10)?,
                },
                dst_name: row.get(11)?,
            });
        }
        Ok(neighbors)
    }

    pub fn supersede_graph_node(&self, old_id: &str, new_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE graph_nodes SET superseded_by = ?1 WHERE id = ?2",
            params![new_id, old_id],
        )?;
        Ok(())
    }

    pub fn get_query_policy(&self, policy_name: &str) -> Result<QueryPolicy> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT policy_name, rrf_k, graph_hop_depth, fts_weight, vec_weight,
                    graph_weight, max_graph_expansion
             FROM query_policy WHERE policy_name = ?1",
            params![policy_name],
            |row| {
                Ok(QueryPolicy {
                    policy_name: row.get(0)?,
                    rrf_k: row.get(1)?,
                    graph_hop_depth: row.get(2)?,
                    fts_weight: row.get(3)?,
                    vec_weight: row.get(4)?,
                    graph_weight: row.get(5)?,
                    max_graph_expansion: row.get(6)?,
                })
            },
        )
    }

    /// Node ids linked to semantic chunks (RRF seed bridge).
    pub fn get_node_ids_for_chunks(&self, chunk_ids: &[&str]) -> Result<Vec<String>> {
        if chunk_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: String = std::iter::repeat("?")
            .take(chunk_ids.len())
            .collect::<Vec<_>>()
            .join(", ");

        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT DISTINCT node_id FROM graph_chunk_links
             WHERE chunk_id IN ({placeholders})
             ORDER BY node_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk_ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        let mut rows = stmt.query(param_refs.as_slice())?;

        let mut node_ids = Vec::new();
        while let Some(row) = rows.next()? {
            node_ids.push(row.get(0)?);
        }
        Ok(node_ids)
    }

    /// Chunks linked to graph nodes, ranked by `link_confidence * node_score`.
    pub fn get_graph_ranked_chunks(
        &self,
        node_scores: &[(String, f64)],
        limit: usize,
    ) -> Result<Vec<(String, String, f64)>> {
        if node_scores.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let placeholders: String = std::iter::repeat("?")
            .take(node_scores.len())
            .collect::<Vec<_>>()
            .join(", ");

        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT gcl.node_id, sm.chunk_id, sm.chunk_text, gcl.link_confidence
             FROM graph_chunk_links gcl
             JOIN semantic_memory sm ON sm.chunk_id = gcl.chunk_id
             WHERE gcl.node_id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let query_params: Vec<Box<dyn rusqlite::types::ToSql>> = node_scores
            .iter()
            .map(|(id, _)| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            query_params.iter().map(|p| p.as_ref()).collect();
        let mut rows = stmt.query(param_refs.as_slice())?;

        let score_map: std::collections::HashMap<&str, f64> = node_scores
            .iter()
            .map(|(id, score)| (id.as_str(), *score))
            .collect();

        let mut chunk_scores: std::collections::HashMap<String, (String, f64)> =
            std::collections::HashMap::new();
        while let Some(row) = rows.next()? {
            let node_id: String = row.get(0)?;
            let chunk_id: String = row.get(1)?;
            let chunk_text: String = row.get(2)?;
            let link_confidence: f64 = row.get(3)?;
            let node_score = score_map.get(node_id.as_str()).copied().unwrap_or(1.0);
            let graph_score = link_confidence * node_score;
            chunk_scores
                .entry(chunk_id.clone())
                .and_modify(|(_, s)| {
                    if graph_score > *s {
                        *s = graph_score;
                    }
                })
                .or_insert((chunk_text, graph_score));
        }

        let mut ranked: Vec<(String, String, f64)> = chunk_scores
            .into_iter()
            .map(|(id, (text, score))| (id, text, score))
            .collect();
        ranked.sort_by(|a, b| {
            b.2.partial_cmp(&a.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        ranked.truncate(limit);
        Ok(ranked)
    }
}

fn row_to_graph_node(row: &rusqlite::Row<'_>) -> Result<GraphNode> {
    Ok(GraphNode {
        id: row.get(0)?,
        session_id: row.get(1)?,
        entity_type: row.get(2)?,
        canonical_name: row.get(3)?,
        aliases_json: row.get(4)?,
        properties_json: row.get(5)?,
        source_uri: row.get(6)?,
        valid_from: row.get(7)?,
        valid_to: row.get(8)?,
        recorded_at: row.get(9)?,
        superseded_by: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_session(db: &Database, session_id: &str) {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES (?1, 'Graph test', 'active')",
            params![session_id],
        )
        .unwrap();
    }

    #[test]
    fn graph_ddl_tables_exist_after_init() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        for table in [
            "graph_nodes",
            "graph_edges",
            "graph_chunk_links",
            "consolidation_runs",
            "query_policy",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
                    params![table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "missing table {table}");
        }

        let default_policy: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM query_policy WHERE policy_name = 'default'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(default_policy, 1);
    }

    #[test]
    fn insert_and_query_graph_nodes_and_edges() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-graph-1");

        db.insert_graph_node(NewGraphNode {
            id: "node-forge",
            session_id: "sess-graph-1",
            entity_type: EntityType::Project,
            canonical_name: "AetherForge",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://seed",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        db.insert_graph_node(NewGraphNode {
            id: "node-maintainer",
            session_id: "sess-graph-1",
            entity_type: EntityType::Person,
            canonical_name: "Alex Maintainer",
            aliases_json: r#"["Alex"]"#,
            properties_json: r#"{"role":"maintainer"}"#,
            source_uri: "memory://seed",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        let edge_id = db
            .insert_graph_edge(NewGraphEdge {
                session_id: "sess-graph-1",
                src_node_id: "node-maintainer",
                dst_node_id: "node-forge",
                relation_type: RelationType::AuthoredBy,
                weight: 1.0,
                evidence_text: "Alex maintains the AetherForge daemon.",
                source_uri: "memory://seed",
                valid_from: None,
                valid_to: None,
            })
            .unwrap();
        assert!(edge_id > 0);

        db.link_graph_chunk("chk-forge", "node-forge", 0.95).unwrap();

        let active = db.get_active_graph_nodes("sess-graph-1", None).unwrap();
        assert_eq!(active.len(), 2);

        let neighbors = db
            .get_one_hop_neighbors("sess-graph-1", &["node-maintainer"], None)
            .unwrap();
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].dst_name, "AetherForge");
        assert_eq!(neighbors[0].edge.relation_type, "authored_by");

        let node = db.get_graph_node("node-forge").unwrap().unwrap();
        assert_eq!(node.canonical_name, "AetherForge");
    }

    #[test]
    fn bi_temporal_validity_excludes_expired_nodes() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-temporal");

        db.insert_graph_node(NewGraphNode {
            id: "node-old",
            session_id: "sess-temporal",
            entity_type: EntityType::Concept,
            canonical_name: "Deprecated Fact",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://old",
            valid_from: Some("2020-01-01 00:00:00"),
            valid_to: Some("2021-01-01 00:00:00"),
        })
        .unwrap();

        db.insert_graph_node(NewGraphNode {
            id: "node-current",
            session_id: "sess-temporal",
            entity_type: EntityType::Concept,
            canonical_name: "Current Fact",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://current",
            valid_from: Some("2022-01-01 00:00:00"),
            valid_to: None,
        })
        .unwrap();

        let at_2023 = db
            .get_active_graph_nodes("sess-temporal", Some("2023-06-01 00:00:00"))
            .unwrap();
        assert_eq!(at_2023.len(), 1);
        assert_eq!(at_2023[0].id, "node-current");

        let at_2020 = db
            .get_active_graph_nodes("sess-temporal", Some("2020-06-01 00:00:00"))
            .unwrap();
        assert_eq!(at_2020.len(), 1);
        assert_eq!(at_2020[0].id, "node-old");
    }

    #[test]
    fn superseded_nodes_excluded_from_active_query() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-super");

        db.insert_graph_node(NewGraphNode {
            id: "node-v1",
            session_id: "sess-super",
            entity_type: EntityType::Person,
            canonical_name: "Taylor v1",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://v1",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        db.insert_graph_node(NewGraphNode {
            id: "node-v2",
            session_id: "sess-super",
            entity_type: EntityType::Person,
            canonical_name: "Taylor v2",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://v2",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        db.supersede_graph_node("node-v1", "node-v2").unwrap();

        let active = db.get_active_graph_nodes("sess-super", None).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "node-v2");
    }

    #[test]
    fn init_schema_is_idempotent_for_graph_tables() {
        let db = Database::open_in_memory().unwrap();
        db.init_schema().unwrap();
        db.init_schema().unwrap();

        let conn = db.conn();
        let nodes: i64 = conn
            .query_row("SELECT COUNT(*) FROM graph_nodes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(nodes, 0);
    }
}
