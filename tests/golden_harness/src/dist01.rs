//! DIST-01 — distribution verify-codesign + spctl smoke (Darwin-only hard gate).

use std::path::PathBuf;
use std::process::Command;

pub fn test_dist01_impl() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("DIST-01 requires Darwin (verify-codesign / spctl)".into());
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let smoke = root.join("scripts/dist01-smoke.sh");
    if !smoke.is_file() {
        return Err(format!("missing {}", smoke.display()));
    }

    let output = Command::new("bash")
        .arg(&smoke)
        .current_dir(&root)
        .output()
        .map_err(|e| format!("dist01-smoke exec failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "dist01-smoke failed (status {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status.code()
        ));
    }

    Ok(())
}
