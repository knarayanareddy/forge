//! Standalone INGEST-01 fixture gate (no live Ollama required).

#[path = "../src/ingest01.rs"]
mod ingest01;

#[test]
fn ingest01_fixture_rejects_extract_json_seed() {
    let n = ingest01::ingest01_fixture_ready().expect("fixture ready");
    assert!(n >= 2);
    let fixture = ingest01::load_ingest01_fixture().expect("load");
    assert!(fixture.user_text.contains("NebulaLedger"));
    assert!(fixture.recall_must_contain.contains("Orchid"));
}
