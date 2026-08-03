//! CONS-01 — human-in-loop consolidation apply/reject (Phase 11 slice).
//!
//! `consolidate_memory` (Slice 6.9) already builds a `review_pending` run with a persisted JSON
//! artifact and never auto-applies. This task exercises the piece that was previously missing:
//! `Database::apply_consolidation_run`/`reject_consolidation_run`, called directly — the exact
//! functions the `consolidate-memory apply`/`reject` CLI subcommands call, not a harness-only
//! reimplementation.

use aether_db::{Database, EntityType, NewGraphNode};

fn seed_session(db: &Database, session_id: &str) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'CONS-01', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn seed_duplicate_pair(db: &Database, session_id: &str, a: &str, b: &str) -> Result<(), String> {
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
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn test_cons01_impl(db: &Database) -> Result<(), String> {
    // --- Apply path: byte-identical to what was reviewed, ignores later drift. ---
    let session_apply = "sess-cons01-apply";
    seed_session(db, session_apply)?;
    seed_duplicate_pair(db, session_apply, "node-a", "node-b")?;

    let artifact_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let record = db
        .consolidate_memory(session_apply, artifact_dir.path())
        .map_err(|e| e.to_string())?;
    if record.status != "review_pending" {
        return Err(format!(
            "expected review_pending after consolidate, got {}",
            record.status
        ));
    }
    if record.preview.nodes_superseded.len() != 1 {
        return Err(format!(
            "expected exactly one duplicate pair in the preview, got {}",
            record.preview.nodes_superseded.len()
        ));
    }

    // A node added AFTER the review must not be touched by apply — apply replays exactly what
    // was reviewed, not a freshly recomputed diff (the anti-theater guarantee for this slice).
    db.insert_graph_node(NewGraphNode {
        id: "node-c",
        session_id: session_apply,
        entity_type: EntityType::Project,
        canonical_name: "AetherForge",
        aliases_json: "[]",
        properties_json: "{}",
        source_uri: "memory://ingest",
        valid_from: Some("2024-03-01"),
        valid_to: None,
    })
    .map_err(|e| e.to_string())?;

    let applied = db
        .apply_consolidation_run(record.run_id)
        .map_err(|e| e.to_string())?;
    if applied != 1 {
        return Err(format!("expected 1 node superseded, got {applied}"));
    }

    let status = db
        .get_consolidation_run_status(record.run_id)
        .map_err(|e| e.to_string())?
        .ok_or("run status missing after apply")?;
    if status != "applied" {
        return Err(format!("expected status 'applied', got {status}"));
    }

    let active = db
        .get_active_graph_nodes(session_apply, None)
        .map_err(|e| e.to_string())?;
    let active_ids: Vec<&str> = active.iter().map(|n| n.id.as_str()).collect();
    if active_ids.contains(&"node-b") {
        return Err("duplicate node-b must be superseded after apply".into());
    }
    if !active_ids.contains(&"node-a") {
        return Err("survivor node-a must remain active after apply".into());
    }
    if !active_ids.contains(&"node-c") {
        return Err("node added after review must be untouched by apply".into());
    }

    // Re-applying an already-applied run is idempotent: a safe no-op, not an error, and it must
    // not touch node-c or re-supersede node-b a second time.
    let reapplied = db
        .apply_consolidation_run(record.run_id)
        .map_err(|e| e.to_string())?;
    if reapplied != 0 {
        return Err(format!(
            "expected re-apply of an already-applied run to be a no-op, superseded {reapplied} more"
        ));
    }

    // --- Reject path: mutates nothing, and a rejected run can never later be applied. ---
    let session_reject = "sess-cons01-reject";
    seed_session(db, session_reject)?;
    seed_duplicate_pair(db, session_reject, "node-x", "node-y")?;

    let rejected_record = db
        .consolidate_memory(session_reject, artifact_dir.path())
        .map_err(|e| e.to_string())?;
    db.reject_consolidation_run(rejected_record.run_id)
        .map_err(|e| e.to_string())?;

    let rejected_status = db
        .get_consolidation_run_status(rejected_record.run_id)
        .map_err(|e| e.to_string())?
        .ok_or("rejected run status missing")?;
    if rejected_status != "rejected" {
        return Err(format!("expected status 'rejected', got {rejected_status}"));
    }

    let still_active = db
        .get_active_graph_nodes(session_reject, None)
        .map_err(|e| e.to_string())?;
    if still_active.len() != 2 {
        return Err("reject must not supersede any node".into());
    }

    if db.apply_consolidation_run(rejected_record.run_id).is_ok() {
        return Err("expected applying a rejected run to fail".into());
    }

    // --- Unknown run id fails closed, not silently. ---
    if db.apply_consolidation_run(i64::MAX).is_ok() {
        return Err("expected apply on an unknown run id to fail".into());
    }

    Ok(())
}
