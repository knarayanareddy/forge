use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SandboxError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Sandbox violation or denied: {0}")]
    Violation(String),
    #[error("Process execution failed: {0}")]
    Execution(String),
    #[error("sandbox-exec required: {0}")]
    MissingSandboxExec(String),
}

pub struct SandboxRunner {
    pub profile_path: String,
}

impl SandboxRunner {
    pub fn new(profile_path: &str) -> Self {
        Self {
            profile_path: profile_path.to_string(),
        }
    }

    /// Executes a tool binary within sandbox-exec (Seatbelt) using the profile file at `profile_path`,
    /// constraining file reads/writes strictly to `workspace_path` via parameter passing (`-D WORKSPACE_PATH=...`).
    /// CI Rule: If sandbox-exec is missing, FS-02 must FAIL with "sandbox-exec required". No unsandboxed bypass.
    pub fn run_sandboxed_command(
        &self,
        binary: &str,
        args: &[&str],
        workspace_path: &Path,
    ) -> Result<std::process::Output, SandboxError> {
        let workspace_str = workspace_path.to_string_lossy();
        
        let has_sandbox_exec = std::process::Command::new("which")
            .arg("sandbox-exec")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !has_sandbox_exec {
            // Check if we are on macOS vs Linux/container where sandbox-exec is expected for golden harness
            #[cfg(target_os = "macos")]
            {
                return Err(SandboxError::MissingSandboxExec("sandbox-exec required on macOS".into()));
            }
            #[cfg(not(target_os = "macos"))]
            {
                // For Linux container test harness execution, we simulate sandbox-exec restriction or fail if strict mode requested
                // But per CI rule: "on Linux, if sandbox-exec missing -> FS-02 must FAIL with 'sandbox-exec required'".
                // Let's enforce the exact error message required by audit instruction:
                return Err(SandboxError::MissingSandboxExec("sandbox-exec required".into()));
            }
        }

        let output = std::process::Command::new("sandbox-exec")
            .arg("-f")
            .arg(&self.profile_path)
            .arg("-D")
            .arg(format!("WORKSPACE_PATH={}", workspace_str))
            .arg(binary)
            .args(args)
            .output()
            .map_err(|e| SandboxError::Execution(e.to_string()))?;

        Ok(output)
    }
}

pub struct StreamParser;

impl StreamParser {
    /// FS-02: Stream-parse a JSONLines file line-by-line without loading entire file into memory.
    pub fn count_errors_streaming(file_path: &Path) -> Result<(usize, usize), SandboxError> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);

        let mut total_lines = 0;
        let mut error_count = 0;

        for line_result in reader.lines() {
            let line = line_result?;
            total_lines += 1;
            
            if line.contains("\"level\":\"error\"") || line.contains("\"level\": \"error\"") {
                error_count += 1;
            }
        }

        Ok((total_lines, error_count))
    }
}
