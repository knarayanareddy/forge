use aether_db::{Database, RecoveryManager};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct CrashRecoveryTest;

impl CrashRecoveryTest {
    fn res_crash_child_path() -> Result<std::path::PathBuf, String> {
        use std::path::PathBuf;

        let beside = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("current_exe has no parent")?
            .join("res-crash-child");
        if beside.is_file() {
            return Ok(beside);
        }

        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for profile in ["debug", "release"] {
            let candidate = manifest.join(format!("../../target/{profile}/res-crash-child"));
            if candidate.is_file() {
                return Ok(candidate);
            }
        }

        Err(format!(
            "res-crash-child not found — run `cargo build -p golden-harness --bins` (expected beside {})",
            beside.display()
        ))
    }

    /// Spawns a child process that inserts a pending undo_journal entry, sends SIGTERM
    /// during the in-flight mutation, then verifies RecoveryManager on restart.
    pub fn simulate_sigterm_recovery(db_path: &Path) -> Result<(), String> {
        if db_path.exists() {
            std::fs::remove_file(db_path).map_err(|e| e.to_string())?;
        }

        let child_bin = Self::res_crash_child_path()?;

        let mut child = Command::new(&child_bin)
            .arg(db_path)
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn res-crash-child: {}", e))?;

        let pid = child.id();
        let stdout = child.stdout.take().ok_or("missing child stdout")?;
        let mut reader = BufReader::new(stdout);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut ready = false;
        while Instant::now() < deadline {
            let mut line = String::new();
            reader.read_line(&mut line).map_err(|e| e.to_string())?;
            if line.trim() == "READY" {
                ready = true;
                break;
            }
        }

        if !ready {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Child never signaled READY with pending undo_journal".into());
        }

        #[cfg(unix)]
        {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
        #[cfg(not(unix))]
        {
            child.kill().map_err(|e| e.to_string())?;
        }

        let wait_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(Some(_status)) = child.try_wait() {
                break;
            }
            if Instant::now() > wait_deadline {
                child.kill().map_err(|e| e.to_string())?;
                return Err("res-crash-child did not exit after SIGTERM".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let db = Database::open(db_path).map_err(|e| e.to_string())?;
        let conn = db.conn();

        let session_title: String = conn
            .query_row(
                "SELECT title FROM sessions WHERE id = 'sess-res-01';",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("Session restorable check failed: {}", e))?;

        if session_title != "RES-01 Session" {
            return Err("Restored session title mismatch".into());
        }

        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM undo_journal WHERE status = 'pending';",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if pending != 0 {
            return Err(format!(
                "Expected 0 pending after RecoveryManager, got {}",
                pending
            ));
        }

        let reverted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM undo_journal WHERE session_id = 'sess-res-01' AND status = 'reverted';",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if reverted != 1 {
            return Err(format!("Expected 1 reverted journal entry, got {}", reverted));
        }

        let report = RecoveryManager::recover_on_startup(&conn).map_err(|e| e.to_string())?;
        if report.pending_reverted != 0 {
            return Err("RecoveryManager should be idempotent on second call".into());
        }

        Ok(())
    }
}
