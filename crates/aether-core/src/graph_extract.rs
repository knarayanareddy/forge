//! Pinned JSON schema + validation for Ollama `graph_extract` output (Slice 6.4).
//!
//! Borrows Graphify provenance (`extracted` | `inferred`) and mandatory `evidence_text`
//! on every node and edge. Ollama prompt + call wired via `run_graph_extract`.

use crate::{CompleteError, ModelRouter};
use aether_db::{EntityType, NewGraphEdge, NewGraphNode, RelationType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

/// Embedded schema path relative to the `aether-core` crate root.
pub const GRAPH_EXTRACT_SCHEMA_PATH: &str = "schemas/graph_extract.schema.json";

/// Pinned schema document (fail-closed drift guard for Slice 6.4 prompts).
pub fn graph_extract_schema_json() -> &'static str {
    include_str!("../schemas/graph_extract.schema.json")
}

/// Graphify-style provenance for extracted facts vs resolver inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    Extracted,
    Inferred,
}

impl Provenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extracted => "extracted",
            Self::Inferred => "inferred",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractNode {
    pub id: String,
    pub entity_type: EntityType,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub properties: serde_json::Map<String, serde_json::Value>,
    pub provenance: Provenance,
    pub evidence_text: String,
    #[serde(default)]
    pub source_uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractEdge {
    pub src_node_id: String,
    pub dst_node_id: String,
    pub relation_type: RelationType,
    #[serde(default = "default_edge_weight")]
    pub weight: f64,
    pub provenance: Provenance,
    pub evidence_text: String,
    #[serde(default)]
    pub source_uri: Option<String>,
}

fn default_edge_weight() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphExtractPayload {
    pub nodes: Vec<ExtractNode>,
    pub edges: Vec<ExtractEdge>,
}

#[derive(Error, Debug, PartialEq)]
pub enum GraphExtractError {
    #[error("Malformed JSON: {0}")]
    Json(String),
    #[error("Missing evidence_text on {kind} at index {index}")]
    MissingEvidenceText { kind: &'static str, index: usize },
    #[error("Invalid provenance on {kind} at index {index}: {value}")]
    InvalidProvenance {
        kind: &'static str,
        index: usize,
        value: String,
    },
    #[error("Empty id on node at index {index}")]
    EmptyNodeId { index: usize },
    #[error("Empty canonical_name on node at index {index}")]
    EmptyCanonicalName { index: usize },
    #[error("Edge at index {index} references unknown node id: {node_id}")]
    UnknownNodeReference { index: usize, node_id: String },
    #[error("Duplicate node id: {id}")]
    DuplicateNodeId { id: String },
    #[error("Invalid edge weight at index {index}: {weight}")]
    InvalidEdgeWeight { index: usize, weight: f64 },
    #[error("Entity cap exceeded: {count} nodes (max {max})")]
    TooManyEntities { count: usize, max: usize },
    #[error("Ollama graph_extract failed: {0}")]
    Ollama(String),
}

/// Build the bounded Ollama prompt for session-turn graph extraction.
pub fn build_graph_extract_prompt(normalized_text: &str, max_entities: usize) -> String {
    format!(
        r#"Extract entities and relations from the session turn below.
Return ONLY valid JSON matching this schema (no markdown fences):
{schema}

Rules:
- Emit at most {max_entities} nodes; prefer highest-signal entities.
- Every node and edge MUST include non-empty evidence_text and provenance ("extracted" or "inferred").
- Node ids must be stable slugs (e.g. "node-forge", "node-alex").
- entity_type: person | project | concept | file | tool | event | other
- relation_type: related_to | part_of | authored_by | depends_on | located_in | implements | contradicts | other
- Use provenance "extracted" when explicitly stated; "inferred" only when strongly implied.
- Omit nodes/edges when evidence is weak; empty arrays are valid.

Session turn:
{normalized_text}"#,
        schema = graph_extract_schema_json(),
        max_entities = max_entities,
        normalized_text = normalized_text,
    )
}

/// Strip optional markdown JSON fences from model output.
pub fn strip_json_fence(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    trimmed.to_string()
}

/// Fail closed when validated payload exceeds the per-turn entity cap.
pub fn enforce_max_entities(
    payload: &GraphExtractPayload,
    max_entities: usize,
) -> Result<(), GraphExtractError> {
    let count = payload.nodes.len();
    if count > max_entities {
        return Err(GraphExtractError::TooManyEntities { count, max: max_entities });
    }
    Ok(())
}

/// Call Ollama via `ModelRouter`, validate JSON, and enforce entity cap.
pub async fn run_graph_extract(
    router: &ModelRouter,
    normalized_text: &str,
    max_entities: usize,
) -> Result<GraphExtractPayload, GraphExtractError> {
    let prompt = build_graph_extract_prompt(normalized_text, max_entities);
    let raw = call_graph_extract_json(router, &prompt).await?;
    let json = strip_json_fence(&raw);
    let payload = validate_graph_extract(&json)?;
    enforce_max_entities(&payload, max_entities)?;
    Ok(payload)
}

const GRAPH_EXTRACT_NUM_PREDICT: u32 = 2048;

async fn call_graph_extract_json(
    router: &ModelRouter,
    prompt: &str,
) -> Result<String, GraphExtractError> {
    router
        .complete_json(prompt, GRAPH_EXTRACT_NUM_PREDICT)
        .await
        .map(|result| result.content)
        .map_err(|e| GraphExtractError::Ollama(e.to_string()))
}

impl From<CompleteError> for GraphExtractError {
    fn from(err: CompleteError) -> Self {
        GraphExtractError::Ollama(err.to_string())
    }
}

/// Validate raw JSON against the pinned graph_extract contract.
pub fn validate_graph_extract(json: &str) -> Result<GraphExtractPayload, GraphExtractError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| GraphExtractError::Json(e.to_string()))?;

    let nodes_val = value
        .get("nodes")
        .ok_or_else(|| GraphExtractError::Json("missing field `nodes`".into()))?;
    let edges_val = value
        .get("edges")
        .ok_or_else(|| GraphExtractError::Json("missing field `edges`".into()))?;

    if !nodes_val.is_array() {
        return Err(GraphExtractError::Json("`nodes` must be an array".into()));
    }
    if !edges_val.is_array() {
        return Err(GraphExtractError::Json("`edges` must be an array".into()));
    }

    let mut nodes = Vec::new();
    let mut node_ids = HashSet::new();

    for (index, node_val) in nodes_val.as_array().unwrap().iter().enumerate() {
        validate_provenance_field(node_val, "node", index)?;

        let evidence = node_val
            .get("evidence_text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if evidence.trim().is_empty() {
            return Err(GraphExtractError::MissingEvidenceText {
                kind: "node",
                index,
            });
        }

        let id = node_val
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if id.is_empty() {
            return Err(GraphExtractError::EmptyNodeId { index });
        }

        let canonical_name = node_val
            .get("canonical_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if canonical_name.is_empty() {
            return Err(GraphExtractError::EmptyCanonicalName { index });
        }

        if !node_ids.insert(id.clone()) {
            return Err(GraphExtractError::DuplicateNodeId { id });
        }

        let entity_type: EntityType = serde_json::from_value(
            node_val
                .get("entity_type")
                .cloned()
                .ok_or_else(|| GraphExtractError::Json(format!("node[{index}] missing entity_type")))?,
        )
        .map_err(|e| GraphExtractError::Json(format!("node[{index}] entity_type: {e}")))?;

        let provenance: Provenance = serde_json::from_value(
            node_val
                .get("provenance")
                .cloned()
                .ok_or_else(|| GraphExtractError::Json(format!("node[{index}] missing provenance")))?,
        )
        .map_err(|e| GraphExtractError::Json(format!("node[{index}] provenance: {e}")))?;

        let aliases: Vec<String> = node_val
            .get("aliases")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|e| GraphExtractError::Json(format!("node[{index}] aliases: {e}")))?
            .unwrap_or_default();

        let properties: serde_json::Map<String, serde_json::Value> = node_val
            .get("properties")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|e| GraphExtractError::Json(format!("node[{index}] properties: {e}")))?
            .unwrap_or_default();

        let source_uri = node_val
            .get("source_uri")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        nodes.push(ExtractNode {
            id,
            entity_type,
            canonical_name,
            aliases,
            properties,
            provenance,
            evidence_text: evidence.to_string(),
            source_uri,
        });
    }

    let mut edges = Vec::new();
    for (index, edge_val) in edges_val.as_array().unwrap().iter().enumerate() {
        validate_provenance_field(edge_val, "edge", index)?;

        let evidence = edge_val
            .get("evidence_text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if evidence.trim().is_empty() {
            return Err(GraphExtractError::MissingEvidenceText {
                kind: "edge",
                index,
            });
        }

        let src_node_id = edge_val
            .get("src_node_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let dst_node_id = edge_val
            .get("dst_node_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !node_ids.contains(&src_node_id) {
            return Err(GraphExtractError::UnknownNodeReference {
                index,
                node_id: src_node_id,
            });
        }
        if !node_ids.contains(&dst_node_id) {
            return Err(GraphExtractError::UnknownNodeReference {
                index,
                node_id: dst_node_id,
            });
        }

        let relation_type: RelationType = serde_json::from_value(
            edge_val
                .get("relation_type")
                .cloned()
                .ok_or_else(|| GraphExtractError::Json(format!("edge[{index}] missing relation_type")))?,
        )
        .map_err(|e| GraphExtractError::Json(format!("edge[{index}] relation_type: {e}")))?;

        let provenance: Provenance = serde_json::from_value(
            edge_val
                .get("provenance")
                .cloned()
                .ok_or_else(|| GraphExtractError::Json(format!("edge[{index}] missing provenance")))?,
        )
        .map_err(|e| GraphExtractError::Json(format!("edge[{index}] provenance: {e}")))?;

        let weight = edge_val.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0);
        if weight < 0.0 || !weight.is_finite() {
            return Err(GraphExtractError::InvalidEdgeWeight { index, weight });
        }

        let source_uri = edge_val
            .get("source_uri")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        edges.push(ExtractEdge {
            src_node_id,
            dst_node_id,
            relation_type,
            weight,
            provenance,
            evidence_text: evidence.to_string(),
            source_uri,
        });
    }

    Ok(GraphExtractPayload { nodes, edges })
}

fn validate_provenance_field(
    value: &serde_json::Value,
    kind: &'static str,
    index: usize,
) -> Result<(), GraphExtractError> {
    let Some(provenance) = value.get("provenance") else {
        return Err(GraphExtractError::InvalidProvenance {
            kind,
            index,
            value: "<missing>".into(),
        });
    };

    let provenance_str = provenance
        .as_str()
        .ok_or_else(|| GraphExtractError::InvalidProvenance {
            kind,
            index,
            value: provenance.to_string(),
        })?;

    match provenance_str {
        "extracted" | "inferred" => Ok(()),
        other => Err(GraphExtractError::InvalidProvenance {
            kind,
            index,
            value: other.to_string(),
        }),
    }
}

/// Owned node row ready for `Database::insert_graph_node`.
#[derive(Debug, Clone)]
pub struct PreparedGraphNode {
    pub id: String,
    pub entity_type: EntityType,
    pub canonical_name: String,
    pub aliases_json: String,
    pub properties_json: String,
    pub source_uri: String,
}

/// Owned edge row ready for `Database::insert_graph_edge`.
#[derive(Debug, Clone)]
pub struct PreparedGraphEdge {
    pub src_node_id: String,
    pub dst_node_id: String,
    pub relation_type: RelationType,
    pub weight: f64,
    pub evidence_text: String,
    pub source_uri: String,
}

impl PreparedGraphNode {
    pub fn as_new<'a>(&'a self, session_id: &'a str) -> NewGraphNode<'a> {
        NewGraphNode {
            id: &self.id,
            session_id,
            entity_type: self.entity_type,
            canonical_name: &self.canonical_name,
            aliases_json: &self.aliases_json,
            properties_json: &self.properties_json,
            source_uri: &self.source_uri,
            valid_from: None,
            valid_to: None,
        }
    }
}

impl PreparedGraphEdge {
    pub fn as_new<'a>(&'a self, session_id: &'a str) -> NewGraphEdge<'a> {
        NewGraphEdge {
            session_id,
            src_node_id: &self.src_node_id,
            dst_node_id: &self.dst_node_id,
            relation_type: self.relation_type,
            weight: self.weight,
            evidence_text: &self.evidence_text,
            source_uri: &self.source_uri,
            valid_from: None,
            valid_to: None,
        }
    }
}

/// Map a validated payload to owned insert rows (Slice 6.4 daemon path).
pub fn payload_to_graph_inserts(
    payload: &GraphExtractPayload,
    default_source_uri: &str,
) -> Result<(Vec<PreparedGraphNode>, Vec<PreparedGraphEdge>), GraphExtractError> {
    let mut nodes = Vec::with_capacity(payload.nodes.len());
    for node in &payload.nodes {
        let source_uri = node
            .source_uri
            .as_deref()
            .unwrap_or(default_source_uri)
            .to_string();
        let aliases_json = serde_json::to_string(&node.aliases)
            .map_err(|e| GraphExtractError::Json(e.to_string()))?;
        let mut props = node.properties.clone();
        props.insert(
            "provenance".into(),
            serde_json::Value::String(node.provenance.as_str().into()),
        );
        props.insert(
            "evidence_text".into(),
            serde_json::Value::String(node.evidence_text.clone()),
        );
        let properties_json = serde_json::to_string(&props)
            .map_err(|e| GraphExtractError::Json(e.to_string()))?;

        nodes.push(PreparedGraphNode {
            id: node.id.clone(),
            entity_type: node.entity_type,
            canonical_name: node.canonical_name.clone(),
            aliases_json,
            properties_json,
            source_uri,
        });
    }

    let mut edges = Vec::with_capacity(payload.edges.len());
    for edge in &payload.edges {
        let source_uri = edge
            .source_uri
            .as_deref()
            .unwrap_or(default_source_uri)
            .to_string();
        edges.push(PreparedGraphEdge {
            src_node_id: edge.src_node_id.clone(),
            dst_node_id: edge.dst_node_id.clone(),
            relation_type: edge.relation_type,
            weight: edge.weight,
            evidence_text: edge.evidence_text.clone(),
            source_uri,
        });
    }

    Ok((nodes, edges))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_PAYLOAD: &str = r#"{
        "nodes": [
            {
                "id": "node-forge",
                "entity_type": "project",
                "canonical_name": "AetherForge",
                "aliases": ["Forge"],
                "provenance": "extracted",
                "evidence_text": "The user mentioned AetherForge as the project name."
            },
            {
                "id": "node-maintainer",
                "entity_type": "person",
                "canonical_name": "Alex Maintainer",
                "provenance": "inferred",
                "evidence_text": "Alex was inferred as maintainer from context."
            }
        ],
        "edges": [
            {
                "src_node_id": "node-maintainer",
                "dst_node_id": "node-forge",
                "relation_type": "authored_by",
                "provenance": "extracted",
                "evidence_text": "Alex maintains the AetherForge daemon."
            }
        ]
    }"#;

    #[test]
    fn schema_file_is_valid_json() {
        let _: serde_json::Value =
            serde_json::from_str(graph_extract_schema_json()).expect("schema must parse");
    }

    #[test]
    fn valid_payload_passes_validation() {
        let payload = validate_graph_extract(VALID_PAYLOAD).unwrap();
        assert_eq!(payload.nodes.len(), 2);
        assert_eq!(payload.edges.len(), 1);
        assert_eq!(payload.nodes[0].provenance, Provenance::Extracted);
        assert_eq!(payload.edges[0].relation_type, RelationType::AuthoredBy);
    }

    #[test]
    fn missing_evidence_text_on_edge_fails() {
        let json = r#"{
            "nodes": [{
                "id": "n1",
                "entity_type": "concept",
                "canonical_name": "Fact",
                "provenance": "extracted",
                "evidence_text": "explicit mention"
            }],
            "edges": [{
                "src_node_id": "n1",
                "dst_node_id": "n1",
                "relation_type": "related_to",
                "provenance": "extracted",
                "evidence_text": ""
            }]
        }"#;
        let err = validate_graph_extract(json).unwrap_err();
        assert_eq!(
            err,
            GraphExtractError::MissingEvidenceText {
                kind: "edge",
                index: 0
            }
        );
    }

    #[test]
    fn missing_evidence_text_on_node_fails() {
        let json = r#"{
            "nodes": [{
                "id": "n1",
                "entity_type": "concept",
                "canonical_name": "Fact",
                "provenance": "extracted",
                "evidence_text": "   "
            }],
            "edges": []
        }"#;
        let err = validate_graph_extract(json).unwrap_err();
        assert_eq!(
            err,
            GraphExtractError::MissingEvidenceText {
                kind: "node",
                index: 0
            }
        );
    }

    #[test]
    fn invalid_provenance_fails() {
        let json = r#"{
            "nodes": [{
                "id": "n1",
                "entity_type": "concept",
                "canonical_name": "Fact",
                "provenance": "guessed",
                "evidence_text": "some text"
            }],
            "edges": []
        }"#;
        let err = validate_graph_extract(json).unwrap_err();
        assert_eq!(
            err,
            GraphExtractError::InvalidProvenance {
                kind: "node",
                index: 0,
                value: "guessed".into()
            }
        );
    }

    #[test]
    fn malformed_json_fails() {
        let err = validate_graph_extract("{not json").unwrap_err();
        assert!(matches!(err, GraphExtractError::Json(_)));
    }

    #[test]
    fn payload_maps_to_graph_inserts() {
        let payload = validate_graph_extract(VALID_PAYLOAD).unwrap();
        let (nodes, edges) =
            payload_to_graph_inserts(&payload, "memory://turn/1").unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert_eq!(nodes[0].entity_type, EntityType::Project);
        assert_eq!(edges[0].relation_type, RelationType::AuthoredBy);
        assert!(edges[0].evidence_text.contains("maintains"));

        let session_id = "sess-1";
        let db_node = nodes[0].as_new(session_id);
        assert_eq!(db_node.canonical_name, "AetherForge");
    }

    #[test]
    fn build_prompt_includes_entity_cap_and_schema() {
        let prompt = build_graph_extract_prompt("Alex maintains Forge.", 16);
        assert!(prompt.contains("Alex maintains Forge."));
        assert!(prompt.contains("at most 16 nodes"));
        assert!(prompt.contains("evidence_text"));
    }

    #[test]
    fn strip_json_fence_removes_markdown_wrapper() {
        let raw = "```json\n{\"nodes\":[],\"edges\":[]}\n```";
        assert_eq!(strip_json_fence(raw), r#"{"nodes":[],"edges":[]}"#);
    }

    #[test]
    fn enforce_max_entities_rejects_overflow() {
        let payload = validate_graph_extract(VALID_PAYLOAD).unwrap();
        let err = enforce_max_entities(&payload, 1).unwrap_err();
        assert_eq!(
            err,
            GraphExtractError::TooManyEntities {
                count: 2,
                max: 1
            }
        );
    }
}
