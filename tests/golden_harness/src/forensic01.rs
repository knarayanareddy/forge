//! FORENSIC-01 — session log failure classifier + regression export (Phase 12 soft probe).

use aether_daemon::forensics::{
    classification_accuracy, discover_forensic_fixture, export_regression_case,
    load_forensic_corpus, FailureClass, FORENSIC01_MIN_ACCURACY,
};

pub fn forensic01_fixture_ready() -> Result<usize, String> {
    let path = discover_forensic_fixture();
    let corpus = load_forensic_corpus(&path).map_err(|e| e.to_string())?;
    Ok(corpus.cases.len())
}

pub fn test_forensic01_impl() -> Result<bool, String> {
    let path = discover_forensic_fixture();
    let corpus = load_forensic_corpus(&path).map_err(|e| e.to_string())?;
    let accuracy = classification_accuracy(&corpus);
    if accuracy < FORENSIC01_MIN_ACCURACY {
        return Err(format!(
            "classification accuracy {accuracy} below minimum {FORENSIC01_MIN_ACCURACY}"
        ));
    }

    let sample = corpus
        .cases
        .iter()
        .find(|c| c.human_label == "missing_grant")
        .ok_or_else(|| "missing missing_grant sample".to_string())?;
    let exported = export_regression_case(sample);
    if exported.case_id != sample.id {
        return Err("export case_id mismatch".into());
    }
    if exported.predicted_class != FailureClass::MissingGrant.as_str() {
        return Err(format!(
            "expected missing_grant export, got {}",
            exported.predicted_class
        ));
    }
    if exported.records.is_empty() {
        return Err("export records empty".into());
    }

    let json = serde_json::to_value(&exported).map_err(|e| e.to_string())?;
    if json.get("case_id").is_none() || json.get("records").is_none() {
        return Err("export JSON missing required fields".into());
    }

    Ok(false)
}
