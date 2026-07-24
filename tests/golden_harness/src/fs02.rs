use std::fs::File;
use std::io::{BufWriter, Write};
use tempfile::tempdir;
use aether_sandbox::{SandboxRunner, StreamParser};

pub async fn test_fs_02_impl() -> Result<(), String> {
    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace_path = tmp.path();
    let log_file_path = workspace_path.join("app.jsonl");

    // 1. Generate ~10MB JSONLines log file with known error counts
    {
        let file = File::create(&log_file_path).map_err(|e| e.to_string())?;
        let mut writer = BufWriter::new(file);
        
        for i in 0..200_000 {
            let level = if i % 10 == 0 { "error" } else { "info" };
            let line = format!(r#"{{"timestamp": 1721830000, "level": "{}", "msg": "log entry {:#06x}"}}"#, level, i);
            writeln!(writer, "{}", line).map_err(|e| e.to_string())?;
        }
        writer.flush().map_err(|e| e.to_string())?;
    }

    // 2. Test StreamParser (FS-02 streaming log parser with O(1) memory)
    let (total_lines, error_count) = StreamParser::count_errors_streaming(&log_file_path)
        .map_err(|e| e.to_string())?;

    if total_lines != 200_000 {
        return Err(format!("Expected 200,000 lines, got {}", total_lines));
    }

    if error_count != 20_000 {
        return Err(format!("Expected 20,000 error lines, got {}", error_count));
    }

    // 3. Test SandboxRunner (+ Seatbelt sandbox on file path)
    let runner = SandboxRunner::new("profiles/sandbox_tool.sb");
    
    // Positive test: Allowed read inside workspace path
    let output = runner.run_sandboxed_command("/bin/cat", &[log_file_path.to_str().unwrap()], workspace_path)
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Sandboxed cat inside workspace failed unexpectedly: status={}, stderr={}", output.status, stderr));
    }

    // Negative test: Attempted read outside workspace path (/etc/passwd) must be denied/fail cleanly via Seatbelt or policy
    let outside_result = runner.run_sandboxed_command("/bin/cat", &["/etc/passwd"], workspace_path);
    
    match outside_result {
        Ok(out) => {
            let stdout_str = String::from_utf8_lossy(&out.stdout);
            let stderr_str = String::from_utf8_lossy(&out.stderr);
            if (stdout_str.contains("root:x:") || stdout_str.len() > 0) && out.status.success() {
                return Err(format!("Security violation: sandboxed process successfully read /etc/passwd outside workspace. stdout={}", stdout_str));
            }
        }
        Err(_) => {
            // Expected sandbox denial / restriction violation
        }
    }

    Ok(())
}
