//! CLI entry for offline consolidate job (Slice 6.9) plus human-in-loop apply/reject (CONS-01).

use std::env;
use std::path::PathBuf;
use std::process;

use aether_db::{format_consolidate_review, Database};

fn usage() {
    eprintln!(
        "consolidate-memory — offline wiki-zone consolidate, with human-reviewed apply/reject

Usage:
  consolidate-memory preview --db PATH --session SESSION_ID [--out DIR]
  consolidate-memory apply   --db PATH --run-id ID
  consolidate-memory reject  --db PATH --run-id ID

  preview   Build a review_pending run + JSON/markdown artifacts. No auto-apply.
  apply     Supersede exactly the node pairs recorded in that run's reviewed artifact.
            Fails if the run is not still review_pending (no double-apply, no applying
            a rejected or unknown run).
  reject    Mark a review_pending run rejected. Mutates no graph node.

  --db PATH       SQLite database path (AetherForge data dir)
  --session ID    Session id to consolidate wiki-zone nodes for (preview only)
  --out DIR       Review artifact output directory (preview only, default: artifacts/consolidate)
  --run-id ID     Consolidation run id (apply/reject only)
  -h, --help      Show this help
"
    );
}

enum Command {
    Preview,
    Apply,
    Reject,
}

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("-h") || args.first().map(String::as_str) == Some("--help") {
        usage();
        process::exit(0);
    }

    let command = if !args.is_empty() && !args[0].starts_with("--") {
        let cmd = args.remove(0);
        match cmd.as_str() {
            "preview" => Command::Preview,
            "apply" => Command::Apply,
            "reject" => Command::Reject,
            other => {
                eprintln!("error: unknown command '{other}'");
                usage();
                process::exit(1);
            }
        }
    } else {
        // Backward compatible: no subcommand + flags means preview, matching the pre-CONS-01 CLI.
        Command::Preview
    };

    let mut db_path = None;
    let mut session_id = None;
    let mut out_dir = PathBuf::from("artifacts/consolidate");
    let mut run_id: Option<i64> = None;

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--db" => db_path = iter.next().map(PathBuf::from),
            "--session" => session_id = iter.next(),
            "--out" => out_dir = iter.next().map(PathBuf::from).unwrap_or(out_dir),
            "--run-id" => {
                run_id = iter
                    .next()
                    .and_then(|s| s.parse::<i64>().ok());
            }
            "-h" | "--help" => {
                usage();
                process::exit(0);
            }
            other => {
                eprintln!("error: unknown argument '{other}'");
                usage();
                process::exit(1);
            }
        }
    }

    let Some(db_path) = db_path else {
        eprintln!("error: --db is required");
        usage();
        process::exit(1);
    };

    let db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("error: open database {}: {e}", db_path.display());
            process::exit(1);
        }
    };

    match command {
        Command::Preview => {
            let Some(session_id) = session_id else {
                eprintln!("error: --session is required for preview");
                usage();
                process::exit(1);
            };
            match db.consolidate_memory(&session_id, &out_dir) {
                Ok(result) => {
                    println!("consolidate run {} complete (review_pending)", result.run_id);
                    println!("  dedupes: {}", result.preview.dedupe_count);
                    println!("  contradictions: {}", result.preview.contradiction_count);
                    println!("  json: {}", result.review_json_path.display());
                    println!("  markdown: {}", result.review_md_path.display());
                    let preview_md = format_consolidate_review(&result.preview);
                    if preview_md.contains("review_pending") {
                        println!("  status: review_pending (no auto-apply)");
                    }
                    println!(
                        "  next: consolidate-memory apply --db {} --run-id {} (after review)",
                        db_path.display(),
                        result.run_id
                    );
                }
                Err(e) => {
                    eprintln!("error: consolidate failed: {e}");
                    process::exit(1);
                }
            }
        }
        Command::Apply => {
            let Some(run_id) = run_id else {
                eprintln!("error: --run-id is required for apply");
                usage();
                process::exit(1);
            };
            match db.apply_consolidation_run(run_id) {
                Ok(applied) => {
                    println!("consolidation run {run_id} applied: {applied} node(s) superseded");
                }
                Err(e) => {
                    eprintln!("error: apply failed: {e}");
                    process::exit(1);
                }
            }
        }
        Command::Reject => {
            let Some(run_id) = run_id else {
                eprintln!("error: --run-id is required for reject");
                usage();
                process::exit(1);
            };
            match db.reject_consolidation_run(run_id) {
                Ok(()) => {
                    println!("consolidation run {run_id} rejected (no nodes changed)");
                }
                Err(e) => {
                    eprintln!("error: reject failed: {e}");
                    process::exit(1);
                }
            }
        }
    }
}
