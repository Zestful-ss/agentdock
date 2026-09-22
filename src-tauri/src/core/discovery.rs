//! Read-only discovery descriptors and Windows path expansion.
//!
//! Adapters in this module have no write / install / enable / disable methods.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterDescriptor {
    pub id: String,
    pub name: String,
    pub repo: Option<String>,
    pub native_consumer: bool,
    pub skill_user_paths: Vec<String>,
    pub skill_project_paths: Vec<String>,
    pub skill_system_paths: Vec<String>,
    pub mcp_user_paths: Vec<String>,
}

impl AdapterDescriptor {
    fn new(
        id: &str,
        name: &str,
        repo: Option<&str>,
        native_consumer: bool,
        skill_user_paths: &[&str],
        skill_project_paths: &[&str],
        skill_system_paths: &[&str],
        mcp_user_paths: &[&str],
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            repo: repo.map(|s| s.to_string()),
            native_consumer,
            skill_user_paths: skill_user_paths.iter().map(|s| s.to_string()).collect(),
            skill_project_paths: skill_project_paths.iter().map(|s| s.to_string()).collect(),
            skill_system_paths: skill_system_paths.iter().map(|s| s.to_string()).collect(),
            mcp_user_paths: mcp_user_paths.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn expanded_skill_user_paths(&self) -> Vec<PathBuf> {
        self.skill_user_paths
            .iter()
            .map(|p| paths::expand_windows_path(p))
            .collect()
    }

    pub fn expanded_skill_system_paths(&self) -> Vec<PathBuf> {
        self.skill_system_paths
            .iter()
            .map(|p| paths::expand_windows_path(p))
            .collect()
    }

    pub fn expanded_mcp_user_paths(&self) -> Vec<PathBuf> {
        self.mcp_user_paths
            .iter()
            .map(|p| paths::expand_windows_path(p))
            .collect()
    }

    pub fn detect(&self) -> bool {
        self.expanded_skill_user_paths()
            .into_iter()
            .chain(self.expanded_mcp_user_paths())
            .any(|p| p.exists())
    }
}

pub fn v1_descriptors() -> Vec<AdapterDescriptor> {
    vec![
        AdapterDescriptor::new(
            "codex",
            "Codex",
            Some("https://github.com/openai/codex"),
            true,
            &["%USERPROFILE%\\.codex\\skills"],
            &[".codex/skills"],
            &["%USERPROFILE%\\.codex\\skills\\.system"],
            &["%USERPROFILE%\\.codex\\config.toml"],
        ),
        AdapterDescriptor::new(
            "claude_code",
            "Claude Code",
            Some("https://github.com/anthropics/claude-code"),
            false,
            &["%USERPROFILE%\\.claude\\skills"],
            &[".claude/skills"],
            &[],
            &[
                "%APPDATA%\\Claude\\claude_desktop_config.json",
                "%USERPROFILE%\\.claude.json",
            ],
        ),
        AdapterDescriptor::new(
            "opencode",
            "OpenCode",
            Some("https://github.com/anomalyco/opencode"),
            true,
            &["%USERPROFILE%\\.config\\opencode\\skills"],
            &[".opencode/skills"],
            &[],
            &["%USERPROFILE%\\.config\\opencode\\opencode.json"],
        ),
        AdapterDescriptor::new(
            "deepseek_harness",
            "DeepSeek Harness",
            Some("https://github.com/deepseek-ai/deepseek-harness"),
            true,
            &["%USERPROFILE%\\.dsh\\skills"],
            &[".dsh/skills"],
            &[],
            &["%USERPROFILE%\\.dsh\\settings.yaml"],
        ),
        AdapterDescriptor::new(
            "kimi",
            "Kimi Code",
            Some("https://github.com/MoonshotAI/kimi-code"),
            true,
            &[
                "%USERPROFILE%\\.kimi-code\\skills",
                "%USERPROFILE%\\.agents\\skills",
            ],
            &[".kimi-code/skills", ".agents/skills"],
            &[],
            &["%USERPROFILE%\\.kimi-code\\config.json"],
        ),
        AdapterDescriptor::new(
            "grok",
            "Grok Build",
            Some("https://github.com/xai-org/grok-build"),
            false,
            &["%USERPROFILE%\\.grok\\skills"],
            &[".grok/skills"],
            &[],
            &["%USERPROFILE%\\.grok\\config.toml"],
        ),
        AdapterDescriptor::new(
            "pi",
            "Pi",
            Some("https://github.com/earendil-works/pi"),
            true,
            &["%USERPROFILE%\\.pi\\agent\\skills"],
            &[".pi/skills"],
            &[],
            &["%USERPROFILE%\\.pi\\mcp.json"],
        ),
        AdapterDescriptor::new(
            "antigravity",
            "Antigravity CLI",
            Some("https://github.com/google-antigravity/antigravity-cli"),
            false,
            &["%USERPROFILE%\\.gemini\\antigravity\\skills"],
            &[".gemini/antigravity/skills"],
            &[],
            &["%USERPROFILE%\\.gemini\\antigravity\\mcp_config.json"],
        ),
        AdapterDescriptor::new(
            "maka",
            "Maka",
            None,
            false,
            &[],
            &[],
            &[],
            &["%APPDATA%\\Maka\\workspaces\\default\\mcp.json"],
        ),
        AdapterDescriptor::new(
            "openchamber",
            "OpenChamber",
            Some("https://github.com/openchamber/openchamber"),
            false,
            &[],
            &[],
            &[],
            &["%USERPROFILE%\\.config\\openchamber\\settings.json"],
        ),
    ]
}

pub fn descriptor_by_id(id: &str) -> Option<AdapterDescriptor> {
    v1_descriptors().into_iter().find(|d| d.id == id)
}
