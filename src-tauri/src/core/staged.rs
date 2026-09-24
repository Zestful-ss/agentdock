//! Staged directory installation mechanics (fail-safe writes).
//!
//! Nothing is ever copied directly into its final destination. Content is
//! built and validated in a `.name.staged-uuid` sibling, then moved into
//! place with backup + rollback. A failed copy therefore leaves neither a
//! half-written skill nor a deleted original behind.
//!
//! Policy decisions (canonical containment, replace-vs-cancel, SkillStore
//! bookkeeping) live with the callers; this module only moves directories.

use std::path::{Path, PathBuf};

use super::content_hash;
use super::error::{AppError, TargetConflictDetail};
use super::skill_metadata;

/// Sibling path used to stage content for `final_path`.
///
/// Lives in the same directory so the final move is a rename, and starts with
/// `.` so skill discovery (which requires a `SKILL.md` marker) is less likely
/// to mistake a half-written stage for a skill.
pub fn staged_sibling_for(final_path: &Path) -> PathBuf {
    let file_name = final_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    final_path.with_file_name(format!(".{file_name}.staged-{}", uuid::Uuid::new_v4()))
}

/// Remove a path whether it is a directory, a file, or a symlink.
/// Missing paths are a no-op. Never follows links.
pub fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.file_type().is_dir() && !meta.file_type().is_symlink() {
        std::fs::remove_dir_all(path).map_err(AppError::io)
    } else {
        std::fs::remove_file(path).map_err(AppError::io)
    }
}

/// Move a fully-built `staged` directory onto `current`.
///
/// - `current` missing → plain rename.
/// - `current` present → rename aside as backup, rename staged in, drop backup.
/// - Rename-in fails → backup restored, staged cleaned, error returned.
///   The pre-existing `current` always survives a failed swap.
pub fn swap_dir_staged(staged: &Path, current: &Path) -> Result<(), AppError> {
    let backup = current.with_file_name(format!(
        ".{}.backup-{}",
        current
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string()),
        uuid::Uuid::new_v4()
    ));

    if current.exists() || is_link(current) {
        // rename() never follows the link itself, so this is safe for
        // symlinks/junctions: the link is moved aside, not its target.
        std::fs::rename(current, &backup).map_err(AppError::io)?;
    }

    if let Err(err) = std::fs::rename(staged, current) {
        if backup.exists() || is_link(&backup) {
            let _ = std::fs::rename(&backup, current);
        }
        let _ = remove_path_if_exists(&staged);
        return Err(AppError::io(format!(
            "Failed to move {} into place: {err}",
            current.display()
        )));
    }

    // Commit point is the rename above: the new content is live. Backup
    // cleanup must never fail the operation afterwards (Windows AV/indexer
    // locks can refuse the delete) or callers would report failure while the
    // new skill is already in effect — and skip their DB reconcile.
    if let Err(err) = remove_path_if_exists(&backup) {
        log::warn!(
            "staged swap: new content is live at {}, but backup cleanup failed: {err}",
            current.display()
        );
    }
    Ok(())
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// A fully-built staged skill directory that has not touched the live
/// destination yet.
pub struct StagedSkillDir {
    pub path: PathBuf,
    pub hash: String,
}

/// Build a validated skill directory beside `final_path` without changing
/// `final_path`. Callers can inspect removals or seek approval before calling
/// [`swap_dir_staged`].
pub fn stage_skill_dir(source: &Path, final_path: &Path) -> Result<StagedSkillDir, AppError> {
    if !source.is_dir() {
        return Err(AppError::not_found("Skill directory not found"));
    }
    if !skill_metadata::is_valid_skill_dir(source) {
        return Err(AppError::invalid_input(
            "Source directory does not contain SKILL.md",
        ));
    }

    let staged = staged_sibling_for(final_path);
    let _ = remove_path_if_exists(&staged);
    let built = (|| {
        super::installer::copy_skill_dir(source, &staged).map_err(AppError::io)?;
        content_hash::hash_directory_strict(&staged).map_err(AppError::io)
    })();
    match built {
        Ok(hash) => Ok(StagedSkillDir { path: staged, hash }),
        Err(err) => {
            let _ = remove_path_if_exists(&staged);
            Err(err)
        }
    }
}

/// Copy `source` into `dest` via a staged sibling and return the staged hash.
///
/// - `dest` missing → staged is renamed into place; a failed rename cleans up.
/// - `dest` present + `replace == false` → staged cleaned, `target_conflict`.
/// - `dest` present + `replace == true` → [`swap_dir_staged`] (backup + rollback).
///
/// `dest` must already be validated by the caller (containment, single-level,
/// no squatting symlink): this function only guarantees the *mechanics* are
/// fail-safe, never the *policy*.
pub fn install_via_stage(source: &Path, dest: &Path, replace: bool) -> Result<String, AppError> {
    if !source.is_dir() {
        return Err(AppError::not_found("Skill directory not found"));
    }
    if !skill_metadata::is_valid_skill_dir(source) {
        return Err(AppError::invalid_input(
            "Source directory does not contain SKILL.md",
        ));
    }
    let skill_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());

    let staged = staged_sibling_for(dest);
    // Best effort: a stale stage from a crashed run must not block us.
    let _ = remove_path_if_exists(&staged);
    let built = (|| -> Result<String, AppError> {
        super::installer::copy_skill_dir(source, &staged).map_err(AppError::io)?;
        content_hash::hash_directory(&staged).map_err(AppError::io)
    })();
    let staged_hash = match built {
        Ok(hash) => hash,
        Err(err) => {
            let _ = remove_path_if_exists(&staged);
            return Err(err);
        }
    };

    if dest.exists() || is_link(dest) {
        if !replace {
            let _ = remove_path_if_exists(&staged);
            return Err(AppError::target_conflict(
                format!("Skill \"{skill_name}\" is already managed. Replace it or cancel."),
                vec![TargetConflictDetail {
                    path: dest.display().to_string(),
                    reason: "already-exists".to_string(),
                }],
            ));
        }
        if let Err(err) = swap_dir_staged(&staged, dest) {
            let _ = remove_path_if_exists(&staged);
            return Err(err);
        }
        return Ok(staged_hash);
    }

    if let Err(err) = std::fs::rename(&staged, dest) {
        let _ = remove_path_if_exists(&staged);
        return Err(AppError::io(format!(
            "Failed to move {} into place: {err}",
            dest.display()
        )));
    }
    Ok(staged_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn make_source(parent: &Path, dir_name: &str, body: &str) -> PathBuf {
        let dir = parent.join(dir_name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("---\nname: {dir_name}\n---\n{body}")).unwrap();
        dir
    }

    #[test]
    fn stage_skill_dir_builds_without_touching_the_live_destination() {
        let tmp = tempdir().unwrap();
        let source = make_source(tmp.path(), "src", "v2");
        let dest = tmp.path().join("final");
        install_via_stage(&source, &dest, false).unwrap();
        fs::write(dest.join("SKILL.md"), "---\nname: final\n---\nlive v1\n").unwrap();

        let staged = stage_skill_dir(&source, &dest).unwrap();
        assert!(staged.path.starts_with(tmp.path()));
        assert!(fs::read_to_string(dest.join("SKILL.md"))
            .unwrap()
            .contains("live v1"));
        assert!(fs::read_to_string(staged.path.join("SKILL.md"))
            .unwrap()
            .contains("v2"));
    }

    #[test]
    fn fresh_install_moves_staged_into_place() {
        let tmp = tempdir().unwrap();
        let source = make_source(tmp.path(), "src", "v1");
        let dest = tmp.path().join("final");
        let hash = install_via_stage(&source, &dest, false).unwrap();
        assert_eq!(hash, content_hash::hash_directory(&dest).unwrap());
        // No stage litter remains next to the destination.
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 2);
    }

    #[test]
    fn conflict_default_cancels_and_leaves_original() {
        let tmp = tempdir().unwrap();
        let source = make_source(tmp.path(), "src", "v1");
        let dest = tmp.path().join("final");
        install_via_stage(&source, &dest, false).unwrap();

        let source2 = make_source(tmp.path(), "src2", "v2");
        let err = install_via_stage(&source2, &dest, false).unwrap_err();
        assert!(matches!(
            err.kind,
            super::super::error::ErrorKind::TargetConflict
        ));
        assert!(fs::read_to_string(dest.join("SKILL.md")).unwrap().contains("v1"));
    }

    #[test]
    fn replace_swaps_without_losing_content() {
        let tmp = tempdir().unwrap();
        let source = make_source(tmp.path(), "src", "v1");
        let dest = tmp.path().join("final");
        install_via_stage(&source, &dest, false).unwrap();

        let source2 = make_source(tmp.path(), "src2", "v2");
        install_via_stage(&source2, &dest, true).unwrap();
        assert!(fs::read_to_string(dest.join("SKILL.md")).unwrap().contains("v2"));
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 3);
    }

    #[test]
    fn invalid_source_never_touches_destination() {
        let tmp = tempdir().unwrap();
        let bad = tmp.path().join("not-a-skill");
        fs::create_dir_all(&bad).unwrap();
        let dest = tmp.path().join("final");
        assert!(install_via_stage(&bad, &dest, true).is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn swap_succeeds_even_when_backup_cleanup_fails() {
        // The rename-in is the commit point: once the new content is live,
        // a failing backup cleanup (Windows AV/indexer locks, read-only
        // files) must degrade to a warning, never to an Err that would make
        // callers skip their DB reconcile.
        let tmp = tempdir().unwrap();
        let source = make_source(tmp.path(), "src", "v1");
        let dest = tmp.path().join("final");
        install_via_stage(&source, &dest, false).unwrap();

        let locked = dest.join("locked.txt");
        fs::write(&locked, "old").unwrap();
        let mut perms = fs::metadata(&locked).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&locked, perms).unwrap();

        let source2 = make_source(tmp.path(), "src2", "v2");
        install_via_stage(&source2, &dest, true).unwrap();
        assert!(fs::read_to_string(dest.join("SKILL.md"))
            .unwrap()
            .contains("v2"));
    }
}
