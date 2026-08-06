//! Hook engine (Phase 10 slices 10.6–10.7 / HOOK-01+).

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPhase {
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
}

pub const DEFAULT_DENY_PATH_PATTERNS: &[&str] = &[
    ".env", "id_rsa", "id_ed25519", ".ssh/", ".git/config", ".aws/credentials", ".aws/config",
];

pub const DEFAULT_REDACT_OUTPUT_PATTERNS: &[&str] = &[
    "SECRET_KEY=", "API_KEY=", "password=", "Bearer ",
];

pub const DEFAULT_DENY_PROMPT_PATTERNS: &[&str] = &[
    "ignore all safety", "dump secrets", "ignore previous instructions",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision { Allow, Deny(String) }

#[derive(Debug, Clone)]
enum HookRule {
    DenyPathPatterns { patterns: &'static [&'static str] },
    RedactOutputPatterns { patterns: &'static [&'static str] },
    DenyPromptPatterns { patterns: &'static [&'static str] },
}

#[derive(Debug, Clone, Default)]
pub struct HookEngine { rules: Vec<(HookPhase, HookRule)> }

impl HookEngine {
    pub fn production() -> Self {
        Self { rules: vec![
            (HookPhase::UserPromptSubmit, HookRule::DenyPromptPatterns { patterns: DEFAULT_DENY_PROMPT_PATTERNS }),
            (HookPhase::PreToolUse, HookRule::DenyPathPatterns { patterns: DEFAULT_DENY_PATH_PATTERNS }),
            (HookPhase::PostToolUse, HookRule::RedactOutputPatterns { patterns: DEFAULT_REDACT_OUTPUT_PATTERNS }),
        ]}
    }
    pub fn run_user_prompt_submit(&self, prompt: &str) -> HookDecision {
        let lower = prompt.to_ascii_lowercase();
        for (phase, rule) in &self.rules {
            if *phase != HookPhase::UserPromptSubmit { continue; }
            if let HookRule::DenyPromptPatterns { patterns } = rule {
                for pattern in *patterns {
                    if lower.contains(pattern) {
                        return HookDecision::Deny(format!("UserPromptSubmit hook blocked prompt matching {pattern:?}"));
                    }
                }
            }
        }
        HookDecision::Allow
    }
    pub fn run_pre_tool_use(&self, resolved_path: &Path) -> HookDecision { pre_tool_use_path_check(resolved_path) }
    pub fn run_post_tool_use(&self, output: &str) -> String { scrub_output_patterns(output, DEFAULT_REDACT_OUTPUT_PATTERNS) }
}

fn scrub_output_patterns(output: &str, patterns: &[&str]) -> String {
    let mut out = output.to_string();
    for pattern in patterns {
        while let Some(idx) = out.find(pattern) {
            let value_start = idx + pattern.len();
            let value_end = out[value_start..].find(|c: char| c.is_whitespace()).map(|i| value_start + i).unwrap_or(out.len());
            out.replace_range(idx..value_end, "[REDACTED]");
        }
    }
    out
}

pub fn pre_tool_use_path_check(resolved_path: &Path) -> HookDecision {
    let path_str = resolved_path.to_string_lossy().to_ascii_lowercase();
    for pattern in DEFAULT_DENY_PATH_PATTERNS {
        if path_str.contains(pattern) {
            return HookDecision::Deny(format!("PreToolUse hook blocked access to a sensitive path matching {pattern:?}: {}", resolved_path.display()));
        }
    }
    HookDecision::Allow
}

pub fn enforce_user_prompt_submit(prompt: &str) -> Result<(), String> {
    match HookEngine::production().run_user_prompt_submit(prompt) {
        HookDecision::Deny(reason) => Err(reason), HookDecision::Allow => Ok(()),
    }
}

pub fn enforce_post_tool_use(output: &str) -> String { HookEngine::production().run_post_tool_use(output) }
pub fn post_tool_use_scrub_output(output: &str) -> String { enforce_post_tool_use(output) }
