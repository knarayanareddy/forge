//! CLI entry for offline consolidate job (Slice 6.9).

use std::env;
use std::path::PathBuf;
use std::process;

use aether_db::{format_consolidate_review, Database};

fn usage() {
    eprintln!(
        "consolidate-memory — offline wiki-zone consolidate (preview only, no auto-apply)

Usage:
  consolidate-memory --db PATH --session SESSION_ID [--out DIR]

  --db PATH           SQLite database path (AetherForge data dir)
  --session ID        Session id to consolidate wiki-zone nodes for
  --out DIR           Review artifact output directory (default: artifacts/consolidate)
  -h, --help          Show this help
"
    );
}

fn main() {
    let mut db_path = None;
    let mut session_id = None;
    let mut out_dir = PathBuf::from("artifacts/consolidate");

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db_path = args.next().map(PathBuf::from),
            "--session" => session_id = args.next(),
            "--out" => out_dir = args.next().map(PathBuf::from).unwrap_or(out_dir),
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
    let Some(session_id) = session_id else {
        eprintln!("error: --session is required");
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
        }
        Err(e) => {
            eprintln!("error: consolidate failed: {e}");
            process::exit(1);
        }
    }
}
