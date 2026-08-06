//! RELY-01 — per-quantization tool reliability ranking (Phase 12).
use aether_core::{evaluate_profile_reliability, rank_profiles_by_reliability, FrozenToolResponse, ToolCallCase};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

pub const RELY01_MIN_CASES: usize = 8;

#[derive(Debug, Clone, Deserialize)]
pub struct Rely01Fixture {
    pub schema_version: u32,
    pub cases: Vec<ToolCallCase>,
    pub profiles: HashMap<String, HashMap<String, FrozenToolResponse>>,
}

pub fn rely01_fixture_ready() -> Result<usize, String> {
    Ok(load_rely01_fixture()?.cases.len())
}

pub fn load_rely01_fixture() -> Result<Rely01Fixture, String> {
    let path = [Path::new("tests/golden_harness/fixtures/rely01_tool_calls.json"), Path::new("fixtures/rely01_tool_calls.json")]
        .into_iter()
        .find(|p| p.exists())
        .ok_or("RELY-01 fixture not found")?
        .to_path_buf();
    let fixture: Rely01Fixture = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("parse: {e}"))?;
    if fixture.schema_version != 1 || fixture.cases.len() < RELY01_MIN_CASES {
        return Err("RELY-01 fixture invalid".into());
    }
    for id in ["q4", "q8"] {
        fixture.profiles.get(id).ok_or_else(|| format!("missing {id}"))?;
    }
    Ok(fixture)
}

pub fn test_rely01_impl() -> Result<(), String> {
    let fixture = load_rely01_fixture()?;
    let q4 = fixture.profiles.get("q4").unwrap();
    let q8 = fixture.profiles.get("q8").unwrap();
    if evaluate_profile_reliability(&fixture.cases, q8) <= evaluate_profile_reliability(&fixture.cases, q4) {
        return Err("q8 must outscore q4".into());
    }
    let profiles: Vec<_> = fixture.profiles.iter().map(|(id, r)| (id.clone(), r.clone())).collect();
    if rank_profiles_by_reliability(&profiles, &fixture.cases)[0].0 != "q8" {
        return Err("q8 must rank first".into());
    }
    Ok(())
}
