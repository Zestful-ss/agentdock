//! V1 policy: manage `.agents`, observe Harness.
//!
//! Harness adapters may discover skills and MCP servers. They must not write,
//! install, enable, or disable anything in a harness-specific directory.

use super::error::AppError;

pub const POLICY_MESSAGE: &str = "V1 refuses harness writes. Canonical locations are ~/.agents/skills and <repo>/.agents/skills.";

pub fn blocked_write() -> AppError {
    AppError::policy(POLICY_MESSAGE)
}

/// True when `relative` is the project-level canonical skills directory.
pub fn is_canonical_project_skills_dir(relative: &str) -> bool {
    let normalized = relative.replace('\\', "/");
    let trimmed = normalized.trim_matches('/');
    trimmed.eq_ignore_ascii_case(".agents/skills")
}

/// True when `path` is inside a canonical `.agents/skills` tree.
pub fn is_canonical_agents_skills_path(path: &std::path::Path) -> bool {
    let raw = path.to_string_lossy();
    let normalized = raw.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/.agents/skills") || normalized.ends_with("/.agents/skills")
}

pub const V1_ADAPTER_IDS: &[&str] = &[
    "codex",
    "claude_code",
    "opencode",
    "deepseek_harness",
    "kimi",
    "grok",
    "pi",
    "antigravity",
    "maka",
    "openchamber",
];

pub fn is_v1_adapter(id: &str) -> bool {
    V1_ADAPTER_IDS.iter().any(|known| *known == id)
}

/// Harnesses known to natively read `~/.agents/skills` / `<repo>/.agents/skills`.
pub fn native_consumers() -> &'static [&'static str] {
    &["codex", "opencode", "deepseek_harness", "kimi", "pi"]
}

pub fn is_native_consumer(id: &str) -> bool {
    native_consumers().iter().any(|known| *known == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn project_canonical_dir_is_agents_skills() {
        assert!(is_canonical_project_skills_dir(".agents/skills"));
        assert!(is_canonical_project_skills_dir(".agents\\skills"));
        assert!(!is_canonical_project_skills_dir(".codex/skills"));
        assert!(!is_canonical_project_skills_dir(".agent/skills"));
    }

    #[test]
    fn windows_path_detects_canonical_tree() {
        assert!(is_canonical_agents_skills_path(Path::new(
            r"C:\Users\charm\.agents\skills\paper-lookup"
        )));
        assert!(!is_canonical_agents_skills_path(Path::new(
            r"C:\Users\charm\.codex\skills\paper-lookup"
        )));
    }
}
