//! Windows-first path helpers.
//!
//! Case-insensitive, slash-normalized identity for skill/MCP locations.

use std::path::{Path, PathBuf};

pub fn home_dir() -> PathBuf {
    dirs::home_dir().expect("Cannot determine home directory")
}

pub fn user_agents_skills_dir() -> PathBuf {
    home_dir().join(".agents").join("skills")
}

pub fn project_agents_skills_dir(project_root: &Path) -> PathBuf {
    project_root.join(".agents").join("skills")
}

pub fn expand_windows_path(template: &str) -> PathBuf {
    let mut value = template.replace('/', "\\");
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        value = value.replace("%USERPROFILE%", &userprofile);
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        value = value.replace("%APPDATA%", &appdata);
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        value = value.replace("%LOCALAPPDATA%", &local);
    }
    PathBuf::from(value)
}

/// Platform-aware path identity used by the managed index.
///
/// Windows paths are case-insensitive and accept both separators. Unix paths
/// are case-sensitive and may legally contain a backslash, so applying the
/// Windows normalization there would merge distinct skills.
pub fn identity_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        let raw = path.to_string_lossy().replace('/', "\\");
        let mut key = raw.to_ascii_lowercase();
        while key.contains("\\\\") {
            key = key.replace("\\\\", "\\");
        }
        if let Ok(canon) = path.canonicalize() {
            return canon
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase();
        }
        key.trim_end_matches('\\').to_string()
    }
    #[cfg(not(windows))]
    {
        if let Ok(canon) = path.canonicalize() {
            return canon.to_string_lossy().into_owned();
        }
        path.to_string_lossy().into_owned()
    }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    identity_key(a) == identity_key(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ignores_slash_and_case() {
        let a = Path::new(r"C:\Users\charm\.agents\skills");
        let b = Path::new(r"c:/users/CHARM/.agents/skills");
        assert_eq!(
            a.to_string_lossy().replace('/', "\\").to_ascii_lowercase(),
            b.to_string_lossy().replace('/', "\\").to_ascii_lowercase()
        );
    }
}
