//! MEM-03 — user-inspectable memory with provenance, edit, delete, export.

use aether_db::{Database, EntityType, NewGraphNode, UserMemoryExport};

pub fn mem03_fixture_ready() -> Result<(), String> {
    let db = Database::open_in_memory().map_err(|e| e.to_string())?;
    db.conn()
        .execute(
            "INSERT INTO sessions (id, title, status) VALUES ('sess-mem03-fix', 'MEM-03', 'active')",
            [],
        )
        .map_err(|e| e.to_string())?;
    db.insert_graph_node(NewGraphNode {
        id: "mem03-fixture",
        session_id: "sess-mem03-fix",
        entity_type: EntityType::Person,
        canonical_name: "fixture fact",
        aliases_json: "[]",
        properties_json: "{}",
        source_uri: "memory://fixture",
        valid_from: None,
        valid_to: None,
    })
    .map_err(|e| e.to_string())?;
    if db.list_user_memory_facts("sess-mem03-fix").map_err(|e| e.to_string())?.len() != 1 {
        return Err("MEM-03 fixture seed failed".into());
    }
    Ok(())
}

pub fn test_mem03_impl(db: &Database) -> Result<bool, String> {
    mem03_fixture_ready()?;

    let session_id = "sess-mem03";
    db.conn()
        .execute(
            "INSERT INTO sessions (id, title, status) VALUES (?1, 'MEM-03', 'active')",
            rusqlite::params![session_id],
        )
        .map_err(|e| e.to_string())?;

    db.insert_graph_node(NewGraphNode {
        id: "mem03-fact-a",
        session_id,
        entity_type: EntityType::Person,
        canonical_name: "Alice prefers tea",
        aliases_json: "[]",
        properties_json: r#"{"topic":"beverage"}"#,
        source_uri: "memory://turn/2",
        valid_from: None,
        valid_to: None,
    })
    .map_err(|e| e.to_string())?;
    db.insert_graph_node(NewGraphNode {
        id: "mem03-fact-b",
        session_id,
        entity_type: EntityType::Concept,
        canonical_name: "works on Zephyr",
        aliases_json: "[]",
        properties_json: "{}",
        source_uri: "memory://turn/4",
        valid_from: None,
        valid_to: None,
    })
    .map_err(|e| e.to_string())?;

    let facts = db
        .list_user_memory_facts(session_id)
        .map_err(|e| e.to_string())?;
    if facts.len() != 2 {
        return Err(format!("MEM-03 expected 2 facts, got {}", facts.len()));
    }
    if !facts.iter().any(|f| f.source_uri == "memory://turn/2") {
        return Err("MEM-03 list must include provenance source_uri".into());
    }

    db.update_user_memory_fact(
        "mem03-fact-a",
        "Alice prefers coffee",
        r#"{"topic":"beverage","user_edited":true}"#,
    )
    .map_err(|e| e.to_string())?;
    let edited = db
        .get_graph_node("mem03-fact-a")
        .map_err(|e| e.to_string())?
        .ok_or("MEM-03 missing edited node")?;
    if edited.canonical_name != "Alice prefers coffee" {
        return Err("MEM-03 edit did not persist canonical_name".into());
    }

    db.delete_user_memory_fact(session_id, "mem03-fact-b")
        .map_err(|e| e.to_string())?;
    let remaining = db
        .list_user_memory_facts(session_id)
        .map_err(|e| e.to_string())?;
    if remaining.len() != 1 || remaining[0].id != "mem03-fact-a" {
        return Err(format!(
            "MEM-03 delete must remove fact from inspectable view, got {:?}",
            remaining.iter().map(|f| &f.id).collect::<Vec<_>>()
        ));
    }

    let exported: UserMemoryExport = serde_json::from_str(
        &db.export_user_memory_json(session_id)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("MEM-03 export JSON invalid: {e}"))?;
    if exported.schema_version != 1 || exported.facts.len() != 1 {
        return Err("MEM-03 export envelope invalid".into());
    }
    if !exported.facts[0].canonical_name.contains("coffee") {
        return Err("MEM-03 export must reflect user edits".into());
    }

    Ok(false)
}
