//! COMPACT-01 — context compaction with thrashing guard.

use aether_core::{
    compact_turns, mechanical_summarize, CompactRequest, CompactionError, ContextTurn,
};

pub fn compact01_fixture_ready() -> Result<(), String> {
    let turns = vec![
        ContextTurn {
            role: "user".into(),
            content: "x".repeat(200),
        },
        ContextTurn {
            role: "assistant".into(),
            content: "y".repeat(200),
        },
        ContextTurn {
            role: "user".into(),
            content: "tail".into(),
        },
    ];
    let req = CompactRequest {
        keep_recent: 1,
        min_reduction_ratio: 0.05,
        ..CompactRequest::default()
    };
    compact_turns(&turns, &req, mechanical_summarize)
        .map_err(|e| format!("COMPACT-01 fixture failed: {e}"))?;
    Ok(())
}

pub fn test_compact01_impl() -> Result<bool, String> {
    compact01_fixture_ready()?;

    let turns: Vec<_> = (0..8)
        .map(|i| ContextTurn {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            content: format!("turn-{i}: {}", "payload-".repeat(80)),
        })
        .collect();

    let req = CompactRequest {
        instruction: "Preserve file paths and decisions".into(),
        max_attempts: 3,
        min_reduction_ratio: 0.15,
        keep_recent: 2,
    };
    let result = compact_turns(&turns, &req, mechanical_summarize)
        .map_err(|e| format!("COMPACT-01 compaction failed: {e}"))?;

    if result.chars_after >= result.chars_before {
        return Err(format!(
            "COMPACT-01 must shrink context: before {} after {}",
            result.chars_before, result.chars_after
        ));
    }
    if !result.summary.contains("Preserve file paths") {
        return Err("COMPACT-01 summary must reflect steerable instruction".into());
    }
    if result.recent_turns.len() != 2 {
        return Err(format!(
            "COMPACT-01 must keep recent tail, got {} turns",
            result.recent_turns.len()
        ));
    }

    let thrash_req = CompactRequest {
        instruction: "noop".into(),
        max_attempts: 2,
        min_reduction_ratio: 0.99,
        keep_recent: 1,
    };
    let short: Vec<_> = (0..4)
        .map(|i| ContextTurn {
            role: "user".into(),
            content: "x".repeat(500 + i),
        })
        .collect();
    match compact_turns(&short, &thrash_req, |_instr, older| {
        older
            .iter()
            .map(|t| t.content.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }) {
        Err(CompactionError::Thrashing { attempts, .. }) if attempts == 2 => {}
        other => return Err(format!("COMPACT-01 expected thrashing guard, got {other:?}")),
    }

    Ok(false)
}
