//! Subagent delegation for read-heavy work (Phase 10 slices 10.3-10.4 / SUB-01).
//!
//! A subagent reads a bounded batch of files under its own file-count budget, separate from the
//! parent's `max_iterations`, and returns a distilled summary — not the raw file contents — as a
//! single tool observation. This is the actual point of a subagent: the parent's context only ever
//! grows by the size of the summary, however much the subagent read internally to produce it. A
//! function that just re-ran the same loop and handed back full observations would not save
//! anything; measuring that the distilled output is materially smaller than the raw input is what
//! makes this a real subagent rather than "a second loop call" (see
//! `docs/ROADMAP_PHASES_9-13.md`'s anti-theater rule for Phase 10 hooks/subagents).
//!
//! Scope: this distillation is mechanical (byte counts + bounded previews), not an LLM-generated
//! summary. A real semantic summarizer is a documented follow-up, not implemented here — this
//! slice proves the structural mechanism (separate budget, bounded output, measured compression),
//! which a smarter distillation step could later slot into without changing the contract.

use crate::loop_engine::resolve_workspace_path;
use aether_sandbox::ProductionSandbox;
use std::path::PathBuf;

/// Own budget, independent of the parent loop's `max_iterations`: how many files a single
/// subagent delegation may read before it must stop and distill what it has.
pub const MAX_SUBAGENT_FILES: usize = 20;
/// Characters kept per file in the distilled summary — enough to be useful, far short of the
/// full file.
const PREVIEW_CHARS: usize = 200;
/// Target ceiling for the whole distilled summary. The roadmap's target is "≤2k tokens"; this
/// uses characters as a conservative proxy (a token is rarely shorter than one character), so a
/// summary within this bound is within the token target too.
pub const MAX_DISTILLED_CHARS: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentFileSummary {
    pub path: String,
    pub bytes: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentResult {
    pub files: Vec<SubagentFileSummary>,
    /// Total bytes of raw file content the subagent actually read — this is what would have
    /// entered the parent's context if it had done the reads itself, one `fs_read` per file.
    pub total_raw_bytes: usize,
    /// The single bounded string returned to the parent as this step's tool observation.
    pub distilled: String,
}

impl SubagentResult {
    /// How many times smaller the distilled output is than the raw content it summarizes.
    /// `f64::INFINITY` when there was no raw content to compare against (an edge case, not
    /// expected in practice since an empty file list is rejected before this is constructed).
    pub fn compression_ratio(&self) -> f64 {
        if self.distilled.is_empty() {
            return f64::INFINITY;
        }
        self.total_raw_bytes as f64 / self.distilled.len() as f64
    }
}

/// Read a bounded batch of files as a subagent and return a distilled summary. Fails if `paths`
/// exceeds the subagent's own file budget or is empty — a subagent with nothing to do is not a
/// valid delegation.
pub fn run_subagent_read_task(
    workspace: &PathBuf,
    paths: &[String],
) -> Result<SubagentResult, String> {
    if paths.is_empty() {
        return Err("subagent read task requires at least one file".into());
    }
    if paths.len() > MAX_SUBAGENT_FILES {
        return Err(format!(
            "subagent file budget exceeded: {} files requested, max {}",
            paths.len(),
            MAX_SUBAGENT_FILES
        ));
    }

    let mut files = Vec::with_capacity(paths.len());
    let mut total_raw_bytes = 0usize;
    for path in paths {
        let full = resolve_workspace_path(workspace, path)?;
        let content =
            ProductionSandbox::read_to_string(workspace, &full).map_err(|e| e.to_string())?;
        total_raw_bytes += content.len();
        let preview: String = content.chars().take(PREVIEW_CHARS).collect();
        files.push(SubagentFileSummary {
            path: path.clone(),
            bytes: content.len(),
            preview,
        });
    }

    let mut distilled = format!(
        "Subagent read {} file(s), {} total bytes.\n",
        files.len(),
        total_raw_bytes
    );
    for file in &files {
        distilled.push_str(&format!(
            "- {} ({} bytes): {}\n",
            file.path, file.bytes, file.preview
        ));
    }
    if distilled.chars().count() > MAX_DISTILLED_CHARS {
        distilled = distilled
            .chars()
            .take(MAX_DISTILLED_CHARS)
            .collect::<String>();
        distilled.push_str("...[truncated]");
    }

    Ok(SubagentResult {
        files,
        total_raw_bytes,
        distilled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_paths_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        assert!(run_subagent_read_task(&workspace, &[]).is_err());
    }

    #[test]
    fn over_budget_file_count_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let mut paths = Vec::new();
        for i in 0..(MAX_SUBAGENT_FILES + 1) {
            let name = format!("f{i}.txt");
            std::fs::write(workspace.join(&name), "x").unwrap();
            paths.push(name);
        }
        let err = run_subagent_read_task(&workspace, &paths).unwrap_err();
        assert!(err.contains("budget exceeded"));
    }

    #[test]
    fn distilled_summary_is_materially_smaller_than_raw_content() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let mut paths = Vec::new();
        for i in 0..5 {
            let name = format!("doc{i}.txt");
            let content = "x".repeat(2_000);
            std::fs::write(workspace.join(&name), &content).unwrap();
            paths.push(name);
        }

        let result = run_subagent_read_task(&workspace, &paths).unwrap();
        assert_eq!(result.total_raw_bytes, 5 * 2_000);
        assert!(
            result.distilled.len() < result.total_raw_bytes / 2,
            "distilled summary ({} bytes) should be materially smaller than raw content ({} bytes)",
            result.distilled.len(),
            result.total_raw_bytes
        );
        assert!(result.compression_ratio() > 2.0);
        assert!(result.distilled.chars().count() <= MAX_DISTILLED_CHARS + 20);
    }

    #[test]
    fn distilled_summary_names_every_file() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        std::fs::write(workspace.join("alpha.txt"), "alpha content").unwrap();
        std::fs::write(workspace.join("beta.txt"), "beta content").unwrap();

        let result = run_subagent_read_task(
            &workspace,
            &["alpha.txt".to_string(), "beta.txt".to_string()],
        )
        .unwrap();
        assert!(result.distilled.contains("alpha.txt"));
        assert!(result.distilled.contains("beta.txt"));
        assert_eq!(result.files.len(), 2);
    }

    #[test]
    fn very_long_content_truncates_the_distilled_summary_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        // Enough files with unique-enough previews that the summary itself would exceed the cap.
        let mut paths = Vec::new();
        for i in 0..20 {
            let name = format!("big{i}.txt");
            let content = format!("{}-", "y".repeat(300));
            std::fs::write(workspace.join(&name), content).unwrap();
            paths.push(name);
        }
        let result = run_subagent_read_task(&workspace, &paths).unwrap();
        assert!(result.distilled.ends_with("...[truncated]"));
        assert!(result.distilled.chars().count() <= MAX_DISTILLED_CHARS + 20);
    }

    #[test]
    fn missing_file_fails_closed_not_silently() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        assert!(run_subagent_read_task(&workspace, &["does_not_exist.txt".to_string()]).is_err());
    }
}
