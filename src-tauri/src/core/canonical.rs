//! Canonical skill store: the single trusted write boundary (V1).
//!
//! Product rule: `.agents/skills` (user + current project) is the only managed
//! area. Harness adapters are discovery sources and must never be written to.
//!
//! Every mutation of a managed skill — install/adopt, replace, delete, read and
//! save of `SKILL.md` — goes through [`ResolvedRoot`]. Callers never accept an
//! arbitrary destination path from the WebView:
//!
//! ```text
//! User                  -> ~/.agents/skills
//! Project(project_id)   -> SkillStore -> <project>/.agents/skills
//! ```
//!
//! Path safety is containment-based, not string-based: the root is
//! canonicalized once, skill names are restricted to a single directory
//! component, and existing directories are re-canonicalized and required to
//! stay inside the root (symlinks/junctions pointing out are refused).

use std::fs;
use std::path::{Path, PathBuf};

use super::content_hash;
use super::error::{AppError, TargetConflictDetail};
use super::paths;
use super::skill_metadata;
use super::skill_store::SkillStore;
use super::sync_engine;

const POLICY_NO_SYMLINK: &str =
    "Refusing to write through a symlink/junction. Remove it manually first.";
const MAX_SKILL_MD_BYTES: u64 = 1_048_576;

/// A canonical skills root with its canonicalized form precomputed.
#[derive(Debug, Clone)]
pub struct ResolvedRoot {
    /// Lexical root (`~/.agents/skills` or `<project>/.agents/skills`).
    pub root: PathBuf,
    /// Canonicalized root. All managed paths must resolve inside this.
    pub canonical: PathBuf,
}

impl ResolvedRoot {
    fn new(root: PathBuf) -> Result<Self, AppError> {
        fs::create_dir_all(&root).map_err(AppError::io)?;
        let canonical = root
            .canonicalize()
            .map_err(|e| AppError::io(format!("Cannot resolve skills root {}: {e}", root.display())))?;
        Ok(Self { root, canonical })
    }
}

/// Resolve the user-level canonical root (`~/.agents/skills`).
pub fn resolve_user_root() -> Result<ResolvedRoot, AppError> {
    ResolvedRoot::new(paths::user_agents_skills_dir())
}

/// Resolve a project-level canonical root from a `SkillStore` project id.
///
/// The frontend passes a project **id**, never a directory. The directory comes
/// from the backend's own project record.
pub fn resolve_project_root(
    store: &SkillStore,
    project_id: &str,
) -> Result<(String, ResolvedRoot), AppError> {
    let record = store
        .get_project_by_id(project_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Project not found"))?;
    let root = paths::project_agents_skills_dir(Path::new(&record.path));
    Ok((record.path, ResolvedRoot::new(root)?))
}

/// Legacy (pre-V1) skill library: `~/.skills-manager/skills` (or the configured
/// base). App metadata (DB, cache, logs) still lives under `base_dir()`; only
/// the skill *library* moved to the canonical location.
pub fn legacy_skills_root() -> PathBuf {
    super::central_repo::base_dir().join("skills")
}

/// Restrict a skill name to a single directory component.
///
/// Uses the shared sanitizer, then additionally rejects anything that is not
/// exactly one normal path component (no separators survive this).
pub fn sanitize_component(name: &str) -> Result<String, AppError> {
    // Reject separators on the raw input first: the shared sanitizer strips
    // leading components via `file_name()`, which would silently turn "a/b"
    // into "b". The canonical writer takes single directory names only.
    if name.contains('/') || name.contains('\\') {
        return Err(AppError::invalid_input(format!("Invalid skill name: '{name}'")));
    }
    let clean = skill_metadata::sanitize_skill_name(name)
        .ok_or_else(|| AppError::invalid_input(format!("Invalid skill name: '{name}'")))?;
    if Path::new(&clean).components().count() != 1 {
        return Err(AppError::invalid_input(format!("Invalid skill name: '{name}'")));
    }
    Ok(clean)
}

/// Lexical destination for a skill inside a resolved root.
///
/// Safe by construction: the name is a single component, so the result cannot
/// escape the root lexically. Canonical containment of *existing* paths is
/// checked separately by [`validate_existing_skill`].
pub fn skill_dir(resolved: &ResolvedRoot, skill_name: &str) -> Result<PathBuf, AppError> {
    let clean = sanitize_component(skill_name)?;
    Ok(resolved.root.join(clean))
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Verify an existing managed skill directory:
/// it must be a real directory (not a symlink/junction) directly inside the
/// root, resolving inside the canonical root.
pub fn validate_existing_skill(
    resolved: &ResolvedRoot,
    dir: &Path,
) -> Result<PathBuf, AppError> {
    if is_link(dir) {
        return Err(AppError::invalid_input(POLICY_NO_SYMLINK));
    }
    if !dir.is_dir() {
        return Err(AppError::not_found("Skill directory not found"));
    }
    let canonical = dir
        .canonicalize()
        .map_err(|e| AppError::io(format!("Cannot resolve {}: {e}", dir.display())))?;
    if !canonical.starts_with(&resolved.canonical) {
        return Err(AppError::invalid_input(
            "Skill directory is outside the managed skills root",
        ));
    }
    // Single-level containment: the skill must be a direct child of the root.
    match canonical.parent() {
        Some(parent) if parent == resolved.canonical => Ok(canonical),
        _ => Err(AppError::invalid_input(
            "Skill directory is outside the managed skills root",
        )),
    }
}

/// Verify a *new* destination path before creating it: it must be
/// `<root>/<single-component-name>` lexically, and must not already be a
/// symlink waiting underneath.
pub fn validate_new_destination(
    resolved: &ResolvedRoot,
    dest: &Path,
) -> Result<(), AppError> {
    let relative = dest.strip_prefix(&resolved.root).map_err(|_| {
        AppError::invalid_input("Destination is outside the managed skills root")
    })?;
    if relative.components().count() != 1 {
        return Err(AppError::invalid_input(
            "Destination is outside the managed skills root",
        ));
    }
    if is_link(dest) {
        return Err(AppError::invalid_input(POLICY_NO_SYMLINK));
    }
    Ok(())
}

fn already_managed_conflict(dest: &Path, skill_name: &str) -> AppError {
    AppError::target_conflict(
        format!("Skill \"{skill_name}\" is already managed. Replace it or cancel."),
        vec![TargetConflictDetail {
            path: dest.display().to_string(),
            reason: "already-exists".to_string(),
        }],
    )
}

fn copy_skill_tree(source: &Path, dest: &Path) -> Result<(), AppError> {
    super::installer::copy_skill_dir(source, dest).map_err(AppError::io)
}

/// Install a skill directory into a canonical root.
///
/// - `Missing` destination → install.
/// - `Existing` destination + `replace == false` (default) → [`AppError`] with
///   kind `target_conflict`; the frontend offers Replace/Cancel.
/// - `Existing` destination + `replace == true` → remove and reinstall.
///
/// This previously deleted silently; that behavior is intentionally gone.
pub fn install_skill_dir(
    source: &Path,
    resolved: &ResolvedRoot,
    replace: bool,
) -> Result<PathBuf, AppError> {
    if !source.is_dir() {
        return Err(AppError::not_found("Skill directory not found"));
    }
    if !skill_metadata::is_valid_skill_dir(source) {
        return Err(AppError::invalid_input(
            "Source directory does not contain SKILL.md",
        ));
    }
    let name = sanitize_component(&skill_metadata::infer_skill_name(source))?;
    install_skill_dir_as(source, resolved, &name, replace)
}

/// [`install_skill_dir`] with an explicit managed name (project update flows
/// where the target directory name is authoritative, not the source metadata).
pub fn install_skill_dir_as(
    source: &Path,
    resolved: &ResolvedRoot,
    name: &str,
    replace: bool,
) -> Result<PathBuf, AppError> {
    if !source.is_dir() {
        return Err(AppError::not_found("Skill directory not found"));
    }
    // Validate the source BEFORE touching the destination: a replace must
    // never delete a managed skill only to discover the source is unusable.
    if !skill_metadata::is_valid_skill_dir(source) {
        return Err(AppError::invalid_input(
            "Source directory does not contain SKILL.md",
        ));
    }
    let clean = sanitize_component(name)?;
    let dest = resolved.root.join(&clean);
    validate_new_destination(resolved, &dest)?;
    sync_engine::ensure_dst_not_inside_src(source, &dest)
        .map_err(|e| AppError::invalid_input(e.to_string()))?;

    if dest.exists() {
        // A non-directory (or link) squatting on the name can never be a
        // managed skill; refuse rather than follow or delete it blindly.
        if is_link(&dest) {
            return Err(AppError::invalid_input(POLICY_NO_SYMLINK));
        }
        if !dest.is_dir() {
            return Err(AppError::invalid_input(format!(
                "Cannot install \"{clean}\": a file with the same name exists"
            )));
        }
        if !replace {
            validate_existing_skill(resolved, &dest)?;
            return Err(already_managed_conflict(&dest, &clean));
        }
        validate_existing_skill(resolved, &dest)?;
        fs::remove_dir_all(&dest).map_err(AppError::io)?;
    }

    copy_skill_tree(source, &dest)?;
    // Post-condition: what we just wrote resolves inside the root.
    validate_existing_skill(resolved, &dest)?;
    Ok(dest)
}

/// Delete a managed skill by name from a canonical root.
pub fn delete_skill(resolved: &ResolvedRoot, skill_name: &str) -> Result<PathBuf, AppError> {
    let dest = skill_dir(resolved, skill_name)?;
    validate_existing_skill(resolved, &dest)?;
    fs::remove_dir_all(&dest).map_err(AppError::io)?;
    Ok(dest)
}

// ── SKILL.md read / save ──

/// A managed skill document.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillDocument {
    pub skill_name: String,
    pub filename: String,
    pub content: String,
    pub path: String,
}

fn skill_markdown_file(skill_dir: &Path) -> Option<PathBuf> {
    for candidate in ["SKILL.md", "skill.md"] {
        let path = skill_dir.join(candidate);
        if path.is_file() && !is_link(&path) {
            return Some(path);
        }
    }
    None
}

/// Read the `SKILL.md` of a managed skill.
pub fn read_skill_document(
    resolved: &ResolvedRoot,
    skill_name: &str,
) -> Result<SkillDocument, AppError> {
    let clean = sanitize_component(skill_name)?;
    let dir = resolved.root.join(&clean);
    let canonical_dir = validate_existing_skill(resolved, &dir)?;
    let file = skill_markdown_file(&canonical_dir)
        .ok_or_else(|| AppError::not_found("SKILL.md not found"))?;
    let metadata = fs::metadata(&file).map_err(AppError::io)?;
    if metadata.len() > MAX_SKILL_MD_BYTES {
        return Err(AppError::invalid_input("SKILL.md is too large to edit"));
    }
    let content = fs::read_to_string(&file).map_err(|_| {
        AppError::invalid_input("SKILL.md is not valid UTF-8 text and cannot be edited here")
    })?;
    Ok(SkillDocument {
        skill_name: clean,
        filename: file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "SKILL.md".to_string()),
        content,
        path: file.display().to_string(),
    })
}

/// Validate edited `SKILL.md` content before saving: non-empty, and if a YAML
/// frontmatter block is present it must parse.
pub fn validate_skill_content(content: &str) -> Result<(), AppError> {
    if content.trim().is_empty() {
        return Err(AppError::invalid_input("SKILL.md must not be empty"));
    }
    if content.as_bytes().len() as u64 > MAX_SKILL_MD_BYTES {
        return Err(AppError::invalid_input("SKILL.md is too large to save"));
    }
    let trimmed = content.trim_start();
    if trimmed.starts_with("---") {
        let rest = &trimmed[3..];
        match rest.find("---") {
            Some(end) => {
                let yaml_str = &rest[..end];
                if serde_yaml::from_str::<serde_yaml::Value>(yaml_str).is_err() {
                    return Err(AppError::invalid_input(
                        "SKILL.md frontmatter is not valid YAML. Fix it before saving.",
                    ));
                }
            }
            None => {
                return Err(AppError::invalid_input(
                    "SKILL.md frontmatter is missing its closing '---'. Fix it before saving.",
                ));
            }
        }
    }
    Ok(())
}

/// Save edited `SKILL.md` content for a managed skill (atomic write).
pub fn save_skill_document(
    resolved: &ResolvedRoot,
    skill_name: &str,
    content: &str,
) -> Result<PathBuf, AppError> {
    validate_skill_content(content)?;
    let clean = sanitize_component(skill_name)?;
    let dir = resolved.root.join(&clean);
    let canonical_dir = validate_existing_skill(resolved, &dir)?;
    // Keep the existing marker filename when the skill uses lowercase;
    // otherwise create the canonical `SKILL.md`.
    let target = skill_markdown_file(&canonical_dir).unwrap_or_else(|| canonical_dir.join("SKILL.md"));
    // The target must be directly inside the validated skill directory.
    match target.parent() {
        Some(parent) if parent == canonical_dir => {}
        _ => {
            return Err(AppError::invalid_input(
                "Skill document is outside the managed skill directory",
            ))
        }
    }
    // Atomic write: temp file in the same directory, then rename.
    let mut temp = tempfile::NamedTempFile::new_in(&canonical_dir).map_err(AppError::io)?;
    use std::io::Write as _;
    temp.write_all(content.as_bytes()).map_err(AppError::io)?;
    temp.flush().map_err(AppError::io)?;
    temp.persist(&target).map_err(|e| {
        AppError::io(format!("Failed to save {}: {e}", target.display()))
    })?;
    Ok(target)
}

// ── Legacy migration (P0.2) ──

/// Outcome of migrating one legacy skill.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationOutcome {
    /// Copied to canonical, DB path updated, hash verified.
    Migrated,
    /// Canonical copy already identical; only the DB path was updated.
    UpdatedDbOnly,
    /// Canonical copy exists with different content; left untouched.
    Conflict,
    /// No DB record pointed at the legacy path; copied (or already present).
    Adopted,
}

/// One row of the migration report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MigrationEntry {
    pub name: String,
    pub legacy_path: String,
    pub canonical_path: String,
    pub outcome: MigrationOutcome,
}

/// Migrate the pre-V1 library (`~/.skills-manager/skills`) into the canonical
/// user root (`~/.agents/skills`).
///
/// Conflict-safe: an existing canonical copy with different content is never
/// overwritten. The legacy directory is never deleted here (P2 cleanup).
pub fn migrate_legacy_library(store: &SkillStore) -> Result<Vec<MigrationEntry>, AppError> {
    let legacy_root = legacy_skills_root();
    if !legacy_root.exists() {
        return Ok(Vec::new());
    }
    // Same physical directory (e.g. tests / overrides): nothing to do.
    if paths::same_path(&legacy_root, &resolve_user_root()?.root) {
        return Ok(Vec::new());
    }
    let resolved = resolve_user_root()?;
    let records = store.get_all_skills().map_err(AppError::db)?;

    let mut report = Vec::new();
    let entries = fs::read_dir(&legacy_root).map_err(AppError::io)?;
    for entry in entries.flatten() {
        let legacy_dir = entry.path();
        if !legacy_dir.is_dir() || is_link(&legacy_dir) {
            continue;
        }
        if !skill_metadata::is_valid_skill_dir(&legacy_dir) {
            continue;
        }
        let name = sanitize_component(&skill_metadata::infer_skill_name(&legacy_dir))?;
        let legacy_hash = content_hash::hash_directory(&legacy_dir).map_err(AppError::io)?;
        let dest = resolved.root.join(&name);

        let matching: Vec<_> = records
            .iter()
            .filter(|r| paths::identity_key(Path::new(&r.central_path)) == paths::identity_key(&legacy_dir))
            .collect();

        if dest.exists() {
            validate_existing_skill(&resolved, &dest)?;
            let dest_hash = content_hash::hash_directory(&dest).map_err(AppError::io)?;
            if dest_hash == legacy_hash {
                for record in &matching {
                    store
                        .update_skill_central_path(&record.id, &dest.display().to_string(), Some(&dest_hash))
                        .map_err(AppError::db)?;
                }
                report.push(MigrationEntry {
                    name,
                    legacy_path: legacy_dir.display().to_string(),
                    canonical_path: dest.display().to_string(),
                    outcome: if matching.is_empty() {
                        MigrationOutcome::Adopted
                    } else {
                        MigrationOutcome::UpdatedDbOnly
                    },
                });
            } else {
                report.push(MigrationEntry {
                    name,
                    legacy_path: legacy_dir.display().to_string(),
                    canonical_path: dest.display().to_string(),
                    outcome: MigrationOutcome::Conflict,
                });
            }
            continue;
        }

        validate_new_destination(&resolved, &dest)?;
        copy_skill_tree(&legacy_dir, &dest)?;
        let copied_hash = content_hash::hash_directory(&dest).map_err(AppError::io)?;
        if copied_hash != legacy_hash {
            // Copy verification failed; remove the half-written copy.
            let _ = fs::remove_dir_all(&dest);
            return Err(AppError::internal(format!(
                "Migration copy verification failed for {name}"
            )));
        }
        for record in &matching {
            store
                .update_skill_central_path(&record.id, &dest.display().to_string(), Some(&copied_hash))
                .map_err(AppError::db)?;
        }
        report.push(MigrationEntry {
            name,
            legacy_path: legacy_dir.display().to_string(),
            canonical_path: dest.display().to_string(),
            outcome: if matching.is_empty() {
                MigrationOutcome::Adopted
            } else {
                MigrationOutcome::Migrated
            },
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_root() -> (tempfile::TempDir, ResolvedRoot) {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("skills");
        let resolved = ResolvedRoot::new(root).unwrap();
        (tmp, resolved)
    }

    fn make_source(parent: &Path, dir_name: &str, body: &str) -> PathBuf {
        let dir = parent.join(dir_name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("---\nname: {dir_name}\n---\n{body}")).unwrap();
        dir
    }

    #[test]
    fn traversal_names_are_rejected() {
        assert!(sanitize_component("../evil").is_err());
        assert!(sanitize_component("..").is_err());
        assert!(sanitize_component("a/b").is_err());
        assert!(sanitize_component("").is_err());
        assert!(sanitize_component("ok-name").is_ok());
    }

    #[test]
    fn existing_path_outside_root_is_refused() {
        let (_tmp, resolved) = test_root();
        let outside = std::env::temp_dir().join("canonical-test-outside-root-marker");
        // A path that is not under the root at all.
        assert!(validate_existing_skill(&resolved, &outside).is_err());
    }

    #[test]
    fn install_missing_then_conflict_then_replace() {
        let tmp = tempdir().unwrap();
        let (_t, resolved) = test_root();
        let source = make_source(tmp.path(), "demo", "v1");

        let dest = install_skill_dir(&source, &resolved, false).unwrap();
        assert!(dest.join("SKILL.md").exists());

        // Second install without replace must NOT silently overwrite.
        let source2 = make_source(tmp.path(), "demo", "v2");
        let err = install_skill_dir(&source2, &resolved, false).unwrap_err();
        assert!(matches!(err.kind, super::super::error::ErrorKind::TargetConflict));
        // Original content untouched.
        let kept = fs::read_to_string(dest.join("SKILL.md")).unwrap();
        assert!(kept.contains("v1"));

        // Explicit replace works.
        let dest2 = install_skill_dir(&source2, &resolved, true).unwrap();
        assert_eq!(dest, dest2);
        let replaced = fs::read_to_string(dest.join("SKILL.md")).unwrap();
        assert!(replaced.contains("v2"));
    }

    #[test]
    fn delete_removes_only_the_named_skill() {
        let tmp = tempdir().unwrap();
        let (_t, resolved) = test_root();
        let source = make_source(tmp.path(), "gone", "x");
        install_skill_dir(&source, &resolved, false).unwrap();
        let removed = delete_skill(&resolved, "gone").unwrap();
        assert!(!removed.exists());
        assert!(resolved.root.exists());
    }

    #[test]
    fn skill_content_validation_rejects_broken_frontmatter() {
        assert!(validate_skill_content("").is_err());
        assert!(validate_skill_content("   \n  ").is_err());
        assert!(validate_skill_content("# plain skill\nbody").is_ok());
        assert!(validate_skill_content("---\nname: ok\n---\nbody").is_ok());
        assert!(validate_skill_content("---\nname: ok\nbody without close").is_err());
        assert!(validate_skill_content("---\n: : broken\n---\n").is_err());
    }

    #[test]
    fn read_save_roundtrip() {
        let tmp = tempdir().unwrap();
        let (_t, resolved) = test_root();
        let source = make_source(tmp.path(), "editable", "hello");
        install_skill_dir(&source, &resolved, false).unwrap();

        let doc = read_skill_document(&resolved, "editable").unwrap();
        assert!(doc.content.contains("hello"));

        save_skill_document(&resolved, "editable", "---\nname: editable\n---\nupdated").unwrap();
        let doc2 = read_skill_document(&resolved, "editable").unwrap();
        assert!(doc2.content.contains("updated"));

        // Broken save is refused and the previous content survives.
        assert!(save_skill_document(&resolved, "editable", "---\nname: x\nno close").is_err());
        let doc3 = read_skill_document(&resolved, "editable").unwrap();
        assert!(doc3.content.contains("updated"));
    }

    #[test]
    fn project_root_resolves_from_store_not_webview() {
        use super::super::skill_store::{ProjectRecord, SkillStore};

        let tmp = tempdir().unwrap();
        let project_dir = tmp.path().join("myproj");
        fs::create_dir_all(&project_dir).unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        store
            .insert_project(&ProjectRecord {
                id: "p1".to_string(),
                name: "myproj".to_string(),
                path: project_dir.display().to_string(),
                workspace_type: "project".to_string(),
                linked_agent_key: None,
                linked_agent_name: None,
                disabled_path: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();

        let (project_path, resolved) = resolve_project_root(&store, "p1").unwrap();
        assert_eq!(PathBuf::from(project_path), project_dir);
        assert_eq!(resolved.root, project_dir.join(".agents").join("skills"));
        assert!(resolved.root.exists());

        // Unknown ids never resolve to a writable root.
        assert!(resolve_project_root(&store, "nope").is_err());
    }
}
