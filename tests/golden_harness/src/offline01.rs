//! OFFLINE-01 — airplane-mode degradation matrix (Phase 12 slice 12.9).

use aether_core::{probe_offline_degradation, NetworkPath, PathStatus};
use std::time::{Duration, Instant};

pub fn offline01_fixture_ready() -> Result<(), String> {
    Ok(())
}

pub async fn test_offline01_impl() -> Result<bool, String> {
    offline01_fixture_ready()?;

    let start = Instant::now();
    let matrix = probe_offline_degradation("http://127.0.0.1:1", Duration::from_millis(800)).await;
    let elapsed = start.elapsed();
    if elapsed > Duration::from_secs(8) {
        return Err(format!(
            "OFFLINE-01 matrix probe hung: {:?} (must fail fast)",
            elapsed
        ));
    }

    if matrix.paths.len() != NetworkPath::all().len() {
        return Err(format!(
            "OFFLINE-01 expected {} paths, got {}",
            NetworkPath::all().len(),
            matrix.paths.len()
        ));
    }
    if matrix.degraded_count() != NetworkPath::all().len() {
        return Err(format!(
            "OFFLINE-01 unreachable endpoint must degrade all paths, got {} degraded",
            matrix.degraded_count()
        ));
    }
    if !matrix.all_degraded_with_messages() {
        return Err("OFFLINE-01 every degraded path must carry a non-empty message".into());
    }
    for (label, status) in &matrix.paths {
        if let PathStatus::Degraded { message } = status {
            if !message.contains(label) {
                return Err(format!(
                    "OFFLINE-01 message for {label} must name the path: {message}"
                ));
            }
        }
    }

    Ok(false)
}
