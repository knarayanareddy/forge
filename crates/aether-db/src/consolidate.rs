//! Offline consolidate job (Slice 6.9): dedupe wiki-zone nodes, surface contradictions,
//! write human-reviewable preview artifact. No auto-apply.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Result};

use crate::{
    format_consolidate_review, ConsolidateEdgeDiff, ConsolidateNodeDiff, ConsolidatePreview,
    Database, EdgeAction, GraphEdge, GraphNode, NodeAction,
};

/// Result of an offline consolidate preview run.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsolidationRunRecord {
    pub run_id: i64,
    pub status: String,
    pub review_json_path: PathBuf,
    pub review_md_path: PathBuf,
    pub preview: ConsolidatePreview,
}

/// Summary row for UI / IPC listing of consolidation runs awaiting human review.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConsolidationRunListItem {
    pub run_id: i64,
    pub status: String,
    pub input_node_count: i64,
    pub dedupe_count: i64,
    pub contradiction_count: i64,
    pub review_artifact_path: Option<String>,
    pub started_at: String,
}

impl Database {
    /// Build a consolidate preview for `session_id` without mutating wiki-zone rows.
    pub fn consolidate_memory_preview(&self, session_id: &str) -> Result<ConsolidatePreview> {
        let nodes = self.get_active_graph_nodes(session_id, None)?;
        let edges = self.get_active_graph_edges(session_id, None)?;

        let mut preview = ConsolidatePreview {
            session_id: session_id.to_string(),
            run_id: None,
            nodes_added: Vec::new(),
            nodes_superseded: Vec::new(),
            edges_changed: Vec::new(),
            dedupe_count: 0,
            contradiction_count: 0,
        };

        dedupe_nodes(&mut preview, &nodes);
        collect_contradictions(&mut preview, &edges, &nodes);

        Ok(preview)
    }

    /// Run offline consolidate: preview diff, write artifacts, record `review_pending` run.
    pub fn consolidate_memory(
        &self,
        session_id: &str,
        artifact_dir: &Path,
    ) -> Result<ConsolidationRunRecord> {
        fs::create_dir_all(artifact_dir).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("create artifact dir {}: {e}", artifact_dir.display()),
            )))
        })?;

        let input_count = self.get_active_graph_nodes(session_id, None)?.len();
        let run_id = self.begin_consolidation_run(input_count)?;

        let mut preview = self.consolidate_memory_preview(session_id)?;
        preview.run_id = Some(run_id);

        let json_path = artifact_dir.join(format!("consolidate-{run_id}.json"));
        let md_path = artifact_dir.join(format!("consolidate-{run_id}.md"));

        let json = serde_json::to_string_pretty(&preview).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("serialize consolidate preview: {e}"),
            )))
        })?;
        fs::write(&json_path, &json).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("write {}: {e}", json_path.display()),
            )))
        })?;

        let markdown = format_consolidate_review(&preview);
        fs::write(&md_path, &markdown).map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("write {}: {e}", md_path.display()),
            )))
        })?;

        let output_count = input_count.saturating_sub(preview.dedupe_count);
        self.finish_consolidation_run(
            run_id,
            "review_pending",
            output_count,
            preview.dedupe_count,
            preview.contradiction_count,
            json_path.to_string_lossy().as_ref(),
        )?;

        Ok(ConsolidationRunRecord {
            run_id,
            status: "review_pending".into(),
            review_json_path: json_path,
            review_md_path: md_path,
            preview,
        })
    }

    fn begin_consolidation_run(&self, input_node_count: usize) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO consolidation_runs (status, input_node_count)
             VALUES ('running', ?1)",
            params![input_node_count as i64],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn finish_consolidation_run(
        &self,
        run_id: i64,
        status: &str,
        output_node_count: usize,
        dedupe_count: usize,
        contradiction_count: usize,
        review_artifact_path: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE consolidation_runs
             SET finished_at = CURRENT_TIMESTAMP,
                 status = ?1,
                 output_node_count = ?2,
                 dedupe_count = ?3,
                 contradiction_count = ?4,
                 review_artifact_path = ?5
             WHERE id = ?6",
            params![
                status,
                output_node_count as i64,
                dedupe_count as i64,
                contradiction_count as i64,
                review_artifact_path,
                run_id,
            ],
        )?;
        Ok(())
    }

    /// List consolidation runs filtered by status (e.g. `review_pending` for the review UI).
    pub fn list_consolidation_runs_by_status(&self, status: &str) -> Result<Vec<ConsolidationRunListItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, status, input_node_count, dedupe_count, contradiction_count,
                    review_artifact_path, started_at
             FROM consolidation_runs
             WHERE status = ?1
             ORDER BY id DESC",
        )?;
        let rows = stmt.query_map(params![status], |row| {
            Ok(ConsolidationRunListItem {
                run_id: row.get(0)?,
                status: row.get(1)?,
                input_node_count: row.get(2)?,
                dedupe_count: row.get(3)?,
                contradiction_count: row.get(4)?,
                review_artifact_path: row.get(5)?,
                started_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Fetch consolidation run status (for review workflow tests).
    pub fn get_consolidation_run_status(&self, run_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT status FROM consolidation_runs WHERE id = ?1")?;
        let mut rows = stmt.query(params![run_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Apply a `review_pending` consolidation run: supersede exactly the node pairs recorded in
    /// its persisted review artifact — not a freshly recomputed diff, which could have drifted
    /// from what a human actually reviewed if new turns landed in between. Returns the number of
    /// nodes superseded.
    ///
    /// Re-applying an already-`applied` run is idempotent: it returns `Ok(0)` rather than an
    /// error, since the end state is unchanged. Every other non-`review_pending` status is a hard
    /// error — in particular, applying a `rejected` run always fails, so a stray apply call can
    /// never silently override an explicit human rejection (Phase 11 slice / CONS-01).
    pub fn apply_consolidation_run(&self, run_id: i64) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();

        let (status, artifact_path): (String, Option<String>) = conn.query_row(
            "SELECT status, review_artifact_path FROM consolidation_runs WHERE id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        if status == "applied" {
            return Ok(0);
        }
        if status != "review_pending" {
            return Err(consolidate_error(format!(
                "consolidation run {run_id} is '{status}', not 'review_pending'; cannot apply"
            )));
        }
        let artifact_path = artifact_path.ok_or_else(|| {
            consolidate_error(format!(
                "consolidation run {run_id} has no review artifact to apply"
            ))
        })?;
        let json = fs::read_to_string(&artifact_path).map_err(|e| {
            consolidate_error(format!("read consolidation artifact {artifact_path}: {e}"))
        })?;
        let preview: ConsolidatePreview = serde_json::from_str(&json).map_err(|e| {
            consolidate_error(format!("parse consolidation artifact {artifact_path}: {e}"))
        })?;

        let tx = conn.transaction()?;
        let mut applied = 0usize;
        for diff in &preview.nodes_superseded {
            if let Some(survivor) = &diff.superseded_by {
                let rows = tx.execute(
                    "UPDATE graph_nodes SET superseded_by = ?1 WHERE id = ?2 AND superseded_by IS NULL",
                    params![survivor, diff.id],
                )?;
                applied += rows;
            }
        }
        tx.execute(
            "UPDATE consolidation_runs SET status = 'applied', applied_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![run_id],
        )?;
        tx.commit()?;

        Ok(applied)
    }

    /// Reject a `review_pending` consolidation run: mark it `rejected` without mutating any graph
    /// node. Fails closed on any status other than `review_pending` (Phase 11 slice / CONS-01).
    pub fn reject_consolidation_run(&self, run_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let status: String = conn.query_row(
            "SELECT status FROM consolidation_runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )?;
        if status != "review_pending" {
            return Err(consolidate_error(format!(
                "consolidation run {run_id} is '{status}', not 'review_pending'; cannot reject"
            )));
        }
        conn.execute(
            "UPDATE consolidation_runs SET status = 'rejected', finished_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![run_id],
        )?;
        Ok(())
    }
}

fn consolidate_error(message: String) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message,
    )))
}

fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn dedupe_nodes(preview: &mut ConsolidatePreview, nodes: &[GraphNode]) {
    let mut groups: HashMap<(String, String), Vec<&GraphNode>> = HashMap::new();
    for node in nodes {
        groups
            .entry((node.entity_type.clone(), normalize_name(&node.canonical_name)))
            .or_default()
            .push(node);
    }

    for group in groups.values() {
        if group.len() <= 1 {
            continue;
        }

        let mut sorted = group.clone();
        sorted.sort_by(|a, b| {
            b.recorded_at
                .cmp(&a.recorded_at)
                .then_with(|| a.id.cmp(&b.id))
        });

        let survivor = sorted[0];
        for duplicate in &sorted[1..] {
            preview.nodes_superseded.push(ConsolidateNodeDiff {
                id: duplicate.id.clone(),
                canonical_name: duplicate.canonical_name.clone(),
                entity_type: duplicate.entity_type.clone(),
                action: NodeAction::Superseded,
                source_uri: duplicate.source_uri.clone(),
                superseded_by: Some(survivor.id.clone()),
                evidence_text: Some(format!(
                    "Duplicate canonical name '{}' merged into survivor '{}'.",
                    duplicate.canonical_name, survivor.canonical_name
                )),
            });
            preview.dedupe_count += 1;
        }
    }
}

fn collect_contradictions(
    preview: &mut ConsolidatePreview,
    edges: &[GraphEdge],
    nodes: &[GraphNode],
) {
    let name_by_id: HashMap<&str, &str> = nodes
        .iter()
        .map(|n| (n.id.as_str(), n.canonical_name.as_str()))
        .collect();

    for edge in edges {
        if edge.relation_type != "contradicts" {
            continue;
        }

        preview.contradiction_count += 1;
        preview.edges_changed.push(ConsolidateEdgeDiff {
            id: edge.id,
            src_name: name_by_id
                .get(edge.src_node_id.as_str())
                .copied()
                .unwrap_or(&edge.src_node_id)
                .to_string(),
            dst_name: name_by_id
                .get(edge.dst_node_id.as_str())
                .copied()
                .unwrap_or(&edge.dst_node_id)
                .to_string(),
            relation_type: edge.relation_type.clone(),
            action: EdgeAction::Changed,
            evidence_text: edge.evidence_text.clone(),
            source_uri: edge.source_uri.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityType, NewGraphEdge, NewGraphNode, RelationType};
    use tempfile::tempdir;

    fn seed_session(db: &Database, session_id: &str) {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES (?1, 'Consolidate test', 'active')",
            params![session_id],
        )
        .unwrap();
    }

    #[test]
    fn dedupe_merges_duplicate_canonical_names_in_preview_only() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-dedupe");

        for (id, recorded_hint) in [("node-a", "2024-01-01"), ("node-b", "2024-02-01")] {
            db.insert_graph_node(NewGraphNode {
                id,
                session_id: "sess-dedupe",
                entity_type: EntityType::Project,
                canonical_name: "AetherForge",
                aliases_json: "[]",
                properties_json: "{}",
                source_uri: "memory://ingest",
                valid_from: Some(recorded_hint),
                valid_to: None,
            })
            .unwrap();
        }

        let preview = db.consolidate_memory_preview("sess-dedupe").unwrap();
        assert_eq!(preview.dedupe_count, 1);
        assert_eq!(preview.nodes_superseded.len(), 1);
        assert_eq!(preview.nodes_superseded[0].superseded_by.as_deref(), Some("node-a"));

        let active = db.get_active_graph_nodes("sess-dedupe", None).unwrap();
        assert_eq!(active.len(), 2, "preview must not auto-apply supersession");
    }

    #[test]
    fn contradiction_edges_surface_in_preview() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-contra");

        db.insert_graph_node(NewGraphNode {
            id: "node-x",
            session_id: "sess-contra",
            entity_type: EntityType::Concept,
            canonical_name: "Fact A",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://a",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();
        db.insert_graph_node(NewGraphNode {
            id: "node-y",
            session_id: "sess-contra",
            entity_type: EntityType::Concept,
            canonical_name: "Fact B",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://b",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        db.insert_graph_edge(NewGraphEdge {
            session_id: "sess-contra",
            src_node_id: "node-x",
            dst_node_id: "node-y",
            relation_type: RelationType::Contradicts,
            weight: 1.0,
            evidence_text: "Conflicting claims about daemon port.",
            source_uri: "memory://extract",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        let preview = db.consolidate_memory_preview("sess-contra").unwrap();
        assert_eq!(preview.contradiction_count, 1);
        assert_eq!(preview.edges_changed.len(), 1);
        assert_eq!(preview.edges_changed[0].relation_type, "contradicts");
    }

    #[test]
    fn run_consolidate_memory_writes_artifacts_and_review_pending() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-run");

        db.insert_graph_node(NewGraphNode {
            id: "node-one",
            session_id: "sess-run",
            entity_type: EntityType::Project,
            canonical_name: "Forge",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://seed",
            valid_from: None,
            valid_to: None,
        })
        .unwrap();

        let dir = tempdir().unwrap();
        let record = db.consolidate_memory("sess-run", dir.path()).unwrap();

        assert_eq!(record.status, "review_pending");
        assert!(record.review_json_path.exists());

        let status = db
            .get_consolidation_run_status(record.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(status, "review_pending");

        let md_path = dir.path().join(format!("consolidate-{}.md", record.run_id));
        assert!(md_path.exists());
        let md = fs::read_to_string(md_path).unwrap();
        assert!(md.contains("review_pending"));
    }

    fn seed_duplicate_pair(db: &Database, session_id: &str, a: &str, b: &str) {
        for (id, recorded_hint) in [(a, "2024-01-01"), (b, "2024-02-01")] {
            db.insert_graph_node(NewGraphNode {
                id,
                session_id,
                entity_type: EntityType::Project,
                canonical_name: "AetherForge",
                aliases_json: "[]",
                properties_json: "{}",
                source_uri: "memory://ingest",
                valid_from: Some(recorded_hint),
                valid_to: None,
            })
            .unwrap();
        }
    }

    #[test]
    fn apply_consolidation_run_supersedes_exactly_the_reviewed_pair() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-apply");
        seed_duplicate_pair(&db, "sess-apply", "node-a", "node-b");

        let dir = tempdir().unwrap();
        let record = db.consolidate_memory("sess-apply", dir.path()).unwrap();
        assert_eq!(record.preview.nodes_superseded.len(), 1);

        // A node added to the graph AFTER review must not be touched by apply — apply replays
        // exactly what was reviewed, not a freshly recomputed diff.
        db.insert_graph_node(NewGraphNode {
            id: "node-c",
            session_id: "sess-apply",
            entity_type: EntityType::Project,
            canonical_name: "AetherForge",
            aliases_json: "[]",
            properties_json: "{}",
            source_uri: "memory://ingest",
            valid_from: Some("2024-03-01"),
            valid_to: None,
        })
        .unwrap();

        let applied = db.apply_consolidation_run(record.run_id).unwrap();
        assert_eq!(applied, 1);

        let status = db
            .get_consolidation_run_status(record.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(status, "applied");

        let active = db.get_active_graph_nodes("sess-apply", None).unwrap();
        let active_ids: Vec<&str> = active.iter().map(|n| n.id.as_str()).collect();
        assert!(!active_ids.contains(&"node-b"), "duplicate must be superseded");
        assert!(active_ids.contains(&"node-a"), "survivor must remain active");
        assert!(
            active_ids.contains(&"node-c"),
            "node added after review must be untouched by apply"
        );
    }

    #[test]
    fn apply_consolidation_run_is_idempotent_when_already_applied() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-apply-twice");
        seed_duplicate_pair(&db, "sess-apply-twice", "node-a", "node-b");

        let dir = tempdir().unwrap();
        let record = db.consolidate_memory("sess-apply-twice", dir.path()).unwrap();
        let first = db.apply_consolidation_run(record.run_id).unwrap();
        assert_eq!(first, 1);

        // Re-applying an already-applied run is a safe no-op, not an error.
        let second = db.apply_consolidation_run(record.run_id).unwrap();
        assert_eq!(second, 0);

        let status = db
            .get_consolidation_run_status(record.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(status, "applied");
    }

    #[test]
    fn apply_consolidation_run_rejects_unknown_run_id() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.apply_consolidation_run(999_999).is_err());
    }

    #[test]
    fn reject_consolidation_run_mutates_no_nodes() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "sess-reject");
        seed_duplicate_pair(&db, "sess-reject", "node-a", "node-b");

        let dir = tempdir().unwrap();
        let record = db.consolidate_memory("sess-reject", dir.path()).unwrap();

        db.reject_consolidation_run(record.run_id).unwrap();

        let status = db
            .get_consolidation_run_status(record.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(status, "rejected");

        let active = db.get_active_graph_nodes("sess-reject", None).unwrap();
        assert_eq!(active.len(), 2, "reject must not supersede any node");

        // A rejected run can never later be applied.
        let err = db.apply_consolidation_run(record.run_id).unwrap_err();
        assert!(err.to_string().contains("not 'review_pending'"));
    }
}
