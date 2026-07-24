use rusqlite::Connection;
use sha2::{Digest, Sha256};

/// Verifies audit_log hash chain integrity for all rows in insertion order.
pub fn verify_audit_hash_chain(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, tool_name, arguments_json, decision, prev_hash, content_hash
             FROM audit_log ORDER BY id ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut expected_prev = "GENESIS_HASH".to_string();
    let mut count = 0;

    for row in rows {
        let (_id, session_id, tool_name, arguments_json, decision, prev_hash, content_hash) =
            row.map_err(|e| e.to_string())?;

        if prev_hash != expected_prev {
            return Err(format!(
                "Hash chain break at id {}: expected prev_hash '{}', got '{}'",
                _id, expected_prev, prev_hash
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(session_id.as_bytes());
        hasher.update(tool_name.as_bytes());
        hasher.update(arguments_json.as_bytes());
        hasher.update(decision.as_bytes());
        let computed = format!("{:x}", hasher.finalize());

        if computed != content_hash {
            return Err(format!(
                "Content hash mismatch at id {}: computed '{}', stored '{}'",
                _id, computed, content_hash
            ));
        }

        expected_prev = content_hash;
        count += 1;
    }

    if count == 0 {
        return Err("Expected at least one audit_log row for hash chain verification".into());
    }

    Ok(())
}
