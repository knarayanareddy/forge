//! Human-reviewable consolidate preview artifact (Rowboat backlink borrow).
//!
//! Offline consolidate job (Slice 6.9) produces this diff; no auto-apply.

use serde::{Deserialize, Serialize};

/// Action taken on a wiki-zone node during consolidate preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeAction {
    Added,
    Superseded,
    Merged,
}

/// Action taken on a graph edge during consolidate preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeAction {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsolidateNodeDiff {
    pub id: String,
    pub canonical_name: String,
    pub entity_type: String,
    pub action: NodeAction,
    pub source_uri: String,
    pub superseded_by: Option<String>,
    pub evidence_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsolidateEdgeDiff {
    pub id: i64,
    pub src_name: String,
    pub dst_name: String,
    pub relation_type: String,
    pub action: EdgeAction,
    pub evidence_text: String,
    pub source_uri: String,
}

/// Pinned preview schema for Slice 6.9 consolidate job output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsolidatePreview {
    pub session_id: String,
    pub run_id: Option<i64>,
    pub nodes_added: Vec<ConsolidateNodeDiff>,
    pub nodes_superseded: Vec<ConsolidateNodeDiff>,
    pub edges_changed: Vec<ConsolidateEdgeDiff>,
    pub dedupe_count: usize,
    pub contradiction_count: usize,
}

impl ConsolidatePreview {
    pub fn node_change_count(&self) -> usize {
        self.nodes_added.len() + self.nodes_superseded.len()
    }
}

/// Render a human-reviewable markdown artifact with Rowboat-style `[[entity]]` backlinks.
pub fn format_consolidate_review(preview: &ConsolidatePreview) -> String {
    let mut out = String::new();

    out.push_str("# Consolidation Review Preview\n\n");
    out.push_str(&format!("**Session:** `{}`\n", preview.session_id));
    if let Some(run_id) = preview.run_id {
        out.push_str(&format!("**Run ID:** {run_id}\n"));
    }
    out.push_str(&format!(
        "**Summary:** {} nodes added, {} superseded, {} edge changes, {} dedupes, {} contradictions\n\n",
        preview.nodes_added.len(),
        preview.nodes_superseded.len(),
        preview.edges_changed.len(),
        preview.dedupe_count,
        preview.contradiction_count
    ));

    if !preview.nodes_added.is_empty() {
        out.push_str("## Nodes added\n\n");
        for node in &preview.nodes_added {
            out.push_str(&format_node_line(node));
        }
        out.push('\n');
    }

    if !preview.nodes_superseded.is_empty() {
        out.push_str("## Nodes superseded\n\n");
        for node in &preview.nodes_superseded {
            out.push_str(&format_node_line(node));
            if let Some(replacement) = &node.superseded_by {
                out.push_str(&format!(
                    "- Superseded by: `{}` (see [[{}]] in wiki zone)\n",
                    replacement, node.canonical_name
                ));
            }
        }
        out.push('\n');
    }

    if !preview.edges_changed.is_empty() {
        out.push_str("## Edge changes\n\n");
        for edge in &preview.edges_changed {
            out.push_str(&format!(
                "- **{:?}** [[{}]] —{}→ [[{}]] (`{}`)\n",
                edge.action,
                edge.src_name,
                backlink_arrow(edge.relation_type.as_str()),
                edge.dst_name,
                edge.relation_type
            ));
            out.push_str(&format!(
                "  - Evidence: \"{}\"\n  - Provenance: `{}`\n",
                edge.evidence_text, edge.source_uri
            ));
        }
        out.push('\n');
    }

    out.push_str("## Review gate\n\n");
    out.push_str(
        "Status must remain `review_pending` until a human approves this artifact. \
         Raw zone (`conversations`, `audit_log`) is never deleted; wiki supersession uses \
         `superseded_by` only after explicit apply (Slice 6.9).\n",
    );

    out
}

fn format_node_line(node: &ConsolidateNodeDiff) -> String {
    let mut line = format!(
        "- **{:?}** [[{}]] (`{}`, `{}`)\n",
        node.action, node.canonical_name, node.entity_type, node.id
    );
    line.push_str(&format!("  - Provenance: `{}`\n", node.source_uri));
    if let Some(evidence) = &node.evidence_text {
        line.push_str(&format!("  - Evidence: \"{evidence}\"\n"));
    }
    line
}

fn backlink_arrow(relation_type: &str) -> &'static str {
    match relation_type {
        "authored_by" | "depends_on" | "part_of" | "implements" | "located_in" => " ",
        "contradicts" => " ⚡ ",
        _ => " — ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_preview() -> ConsolidatePreview {
        ConsolidatePreview {
            session_id: "sess-consolidate-1".into(),
            run_id: Some(42),
            nodes_added: vec![ConsolidateNodeDiff {
                id: "node-forge-v2".into(),
                canonical_name: "AetherForge".into(),
                entity_type: "project".into(),
                action: NodeAction::Added,
                source_uri: "memory://consolidate/run-42".into(),
                superseded_by: None,
                evidence_text: Some("Merged duplicate project mentions.".into()),
            }],
            nodes_superseded: vec![ConsolidateNodeDiff {
                id: "node-forge-v1".into(),
                canonical_name: "AetherForge".into(),
                entity_type: "project".into(),
                action: NodeAction::Superseded,
                source_uri: "memory://ingest/turn-12".into(),
                superseded_by: Some("node-forge-v2".into()),
                evidence_text: Some("Duplicate canonical name after dedupe.".into()),
            }],
            edges_changed: vec![ConsolidateEdgeDiff {
                id: 7,
                src_name: "Alex Maintainer".into(),
                dst_name: "AetherForge".into(),
                relation_type: "authored_by".into(),
                action: EdgeAction::Changed,
                evidence_text: "Alex maintains the AetherForge daemon.".into(),
                source_uri: "memory://consolidate/run-42".into(),
            }],
            dedupe_count: 1,
            contradiction_count: 0,
        }
    }

    #[test]
    fn markdown_contains_entity_names_and_provenance() {
        let md = format_consolidate_review(&sample_preview());
        assert!(md.contains("[[AetherForge]]"));
        assert!(md.contains("[[Alex Maintainer]]"));
        assert!(md.contains("memory://consolidate/run-42"));
        assert!(md.contains("memory://ingest/turn-12"));
        assert!(md.contains("Alex maintains the AetherForge daemon."));
        assert!(md.contains("review_pending"));
    }

    #[test]
    fn preview_serializes_to_json_roundtrip() {
        let preview = sample_preview();
        let json = serde_json::to_string(&preview).unwrap();
        let parsed: ConsolidatePreview = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, preview);
        assert_eq!(parsed.node_change_count(), 2);
    }
}
