//! LOOP-03 — production NL validator accepts non-gold tool order (Slice 8.0a probe).
//!
//! Not yet in the golden TASKS scoreboard; run via unit tests in `aether-core` or this probe.

use aether_core::validate_nl_plan;

/// Verify production `validate_nl_plan` accepts fs_read-first plans (non-gold order).
pub fn loop03_deharness_probe() -> Result<(), String> {
    let json = r#"{"loop":[
        {"action":"fs_read","path":"notes.txt"},
        {"action":"done"}
    ]}"#;
    validate_nl_plan(json, 8).map_err(|e| format!("LOOP-03 probe failed: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop03_probe_accepts_fs_read_plan() {
        loop03_deharness_probe().expect("fs_read plan must pass production validator");
    }
}
