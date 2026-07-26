use std::fs::File;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
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
    #[error("sandbox profile missing: {0}")]
    MissingProfile(String),
    #[error("sandbox target escapes workspace: {0}")]
    InvalidPath(String),
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

/// Production tool sandbox shared by core filesystem, git, lint, skill, and MCP paths.
///
/// On Darwin every command is wrapped by Seatbelt and fails closed when the executable/profile
/// is unavailable. Other platforms retain OS-native execution for CI portability, but still use
/// the same path validation and scrubbed child environment.
pub struct ProductionSandbox;

impl ProductionSandbox {
    #[cfg(target_os = "macos")]
    fn resolve_macos_executable(binary: &str) -> Result<PathBuf, SandboxError> {
        let path = Path::new(binary);
        if path.is_absolute() {
            // /usr/bin/git and /usr/bin/python3 are xcrun shims. Resolve their real developer
            // binaries before entering Seatbelt so xcrun does not need an external cache.
            if binary != "/usr/bin/git" && binary != "/usr/bin/python3" {
                return Ok(path.to_path_buf());
            }
        }

        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or(binary);
        if matches!(name, "git" | "python3") {
            let output = Command::new("/usr/bin/xcrun")
                .args(["--find", name])
                .output()
                .map_err(|e| SandboxError::Execution(format!("xcrun --find {name}: {e}")))?;
            if output.status.success() {
                let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !resolved.is_empty() && Path::new(&resolved).is_file() {
                    return Ok(PathBuf::from(resolved));
                }
            }
            return Err(SandboxError::Execution(format!(
                "xcrun could not resolve {name}: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
        for root in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            let candidate = Path::new(root).join(binary);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(SandboxError::Execution(format!(
            "tool executable not found in fixed path: {binary}"
        )))
    }

    pub fn resolve_profile() -> Result<PathBuf, SandboxError> {
        if let Some(path) = std::env::var_os("AETHER_SANDBOX_PROFILE") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return path.canonicalize().map_err(SandboxError::Io);
            }
            return Err(SandboxError::MissingProfile(path.display().to_string()));
        }

        let development = PathBuf::from("profiles/sandbox_tool.sb");
        if development.is_file() {
            return development.canonicalize().map_err(SandboxError::Io);
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let bundled = exe_dir.join("../Resources/profiles/sandbox_tool.sb");
                if bundled.is_file() {
                    return bundled.canonicalize().map_err(SandboxError::Io);
                }
            }
        }
        Err(SandboxError::MissingProfile(
            "set AETHER_SANDBOX_PROFILE or bundle Contents/Resources/profiles/sandbox_tool.sb"
                .into(),
        ))
    }

    pub fn validate_target(workspace: &Path, target: &Path) -> Result<PathBuf, SandboxError> {
        if target
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(SandboxError::InvalidPath(target.display().to_string()));
        }

        let workspace = workspace.canonicalize()?;
        let candidate = if target.is_absolute() {
            target.to_path_buf()
        } else {
            workspace.join(target)
        };
        let resolved = if candidate.exists() {
            candidate.canonicalize()?
        } else {
            let parent = candidate.parent().ok_or_else(|| {
                SandboxError::InvalidPath(candidate.display().to_string())
            })?;
            parent.canonicalize()?.join(
                candidate
                    .file_name()
                    .ok_or_else(|| SandboxError::InvalidPath(candidate.display().to_string()))?,
            )
        };
        if !resolved.starts_with(&workspace) {
            return Err(SandboxError::InvalidPath(resolved.display().to_string()));
        }
        Ok(resolved)
    }

    pub fn command<I, S>(
        binary: &str,
        args: I,
        workspace: &Path,
    ) -> Result<Command, SandboxError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let workspace = workspace.canonicalize()?;
        let temp = workspace.join(".aether-tmp");
        std::fs::create_dir_all(&temp)?;

        #[cfg(target_os = "macos")]
        let mut command = {
            let profile = Self::resolve_profile()?;
            let binary = Self::resolve_macos_executable(binary)?;
            if !Path::new("/usr/bin/sandbox-exec").is_file() {
                return Err(SandboxError::MissingSandboxExec(
                    "/usr/bin/sandbox-exec required on macOS".into(),
                ));
            }
            let mut command = Command::new("/usr/bin/sandbox-exec");
            command
                .arg("-f")
                .arg(profile)
                .arg("-D")
                .arg(format!("WORKSPACE_PATH={}", workspace.display()))
                .arg(binary);
            command
        };

        #[cfg(not(target_os = "macos"))]
        let mut command = Command::new(binary);

        command.args(args);
        command.env_clear();
        command.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin");
        command.env("HOME", &workspace);
        command.env("TMPDIR", &temp);
        command.env("TZ", "UTC");
        command.env("GIT_AUTHOR_NAME", "AetherForge");
        command.env("GIT_AUTHOR_EMAIL", "aetherforge@localhost");
        command.env("GIT_COMMITTER_NAME", "AetherForge");
        command.env("GIT_COMMITTER_EMAIL", "aetherforge@localhost");
        Ok(command)
    }

    pub fn read_to_string(workspace: &Path, target: &Path) -> Result<String, SandboxError> {
        let target = Self::validate_target(workspace, target)?;
        let output = Self::command("/bin/cat", [&target], workspace)?
            .output()
            .map_err(|e| SandboxError::Execution(e.to_string()))?;
        if !output.status.success() {
            return Err(SandboxError::Violation(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        String::from_utf8(output.stdout)
            .map_err(|e| SandboxError::Execution(format!("non-UTF8 file: {e}")))
    }

    pub fn write_file(
        workspace: &Path,
        target: &Path,
        content: &[u8],
    ) -> Result<(), SandboxError> {
        Self::write_with_tee(workspace, target, content, false)
    }

    pub fn append_file(
        workspace: &Path,
        target: &Path,
        content: &[u8],
    ) -> Result<(), SandboxError> {
        Self::write_with_tee(workspace, target, content, true)
    }

    fn write_with_tee(
        workspace: &Path,
        target: &Path,
        content: &[u8],
        append: bool,
    ) -> Result<(), SandboxError> {
        let target = Self::validate_target(workspace, target)?;
        let mut args = Vec::new();
        if append {
            args.push(OsStr::new("-a"));
        }
        args.push(target.as_os_str());
        let mut command = Self::command("/usr/bin/tee", args, workspace)?;
        command.stdin(Stdio::piped()).stdout(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|e| SandboxError::Execution(e.to_string()))?;
        child
            .stdin
            .take()
            .ok_or_else(|| SandboxError::Execution("tee stdin unavailable".into()))?
            .write_all(content)?;
        let output = child
            .wait_with_output()
            .map_err(|e| SandboxError::Execution(e.to_string()))?;
        if !output.status.success() {
            return Err(SandboxError::Violation(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        Ok(())
    }

    pub fn create_dir_all(workspace: &Path, target: &Path) -> Result<(), SandboxError> {
        let target = Self::validate_target(workspace, target)?;
        let output = Self::command("/bin/mkdir", [OsStr::new("-p"), target.as_os_str()], workspace)?
            .output()
            .map_err(|e| SandboxError::Execution(e.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(SandboxError::Violation(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_file_helpers_stay_inside_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("note.txt");
        ProductionSandbox::write_file(temp.path(), &file, b"one").unwrap();
        ProductionSandbox::append_file(temp.path(), &file, b"-two").unwrap();
        assert_eq!(
            ProductionSandbox::read_to_string(temp.path(), &file).unwrap(),
            "one-two"
        );
    }

    #[test]
    fn production_file_helpers_reject_parent_and_symlink_escape() {
        let temp = tempfile::tempdir().unwrap();
        assert!(ProductionSandbox::validate_target(temp.path(), Path::new("../escape")).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", temp.path().join("link")).unwrap();
            assert!(
                ProductionSandbox::validate_target(temp.path(), &temp.path().join("link"))
                    .is_err()
            );
        }
    }

    #[test]
    fn production_command_scrubs_parent_secrets() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("AETHER_SANDBOX_UNIT_SECRET", "do-not-inherit");
        let result = ProductionSandbox::command(
            "/usr/bin/env",
            std::iter::empty::<&str>(),
            temp.path(),
        )
        .and_then(|mut command| command.output().map_err(SandboxError::Io));
        std::env::remove_var("AETHER_SANDBOX_UNIT_SECRET");
        let output = result.unwrap();
        let environment = String::from_utf8_lossy(&output.stdout);
        assert!(!environment.contains("AETHER_SANDBOX_UNIT_SECRET"));
        assert!(!environment.contains("do-not-inherit"));
    }
}
