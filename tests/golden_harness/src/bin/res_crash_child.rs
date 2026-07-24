//! Child process for RES-01: holds a pending undo_journal entry until SIGTERM.
use aether_db::Database;
use std::io::{self, Write};
use std::path::PathBuf;
use std::{thread, time::Duration};

fn main() {
    let db_path = std::env::args()
        .nth(1)
        .expect("usage: res-crash-child <db-path>");

    let db = Database::open(PathBuf::from(&db_path)).expect("open db");
    let conn = db.conn();

    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES ('sess-res-01', 'RES-01 Session', 'active')",
        [],
    )
    .expect("insert session");

    conn.execute(
        "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
         VALUES ('sess-res-01', 'file_rename', '/tmp/target.txt', '{}', 'pending')",
        [],
    )
    .expect("insert pending journal");

    println!("READY");
    io::stdout().flush().expect("flush ready");

    loop {
        thread::sleep(Duration::from_millis(100));
    }
}
