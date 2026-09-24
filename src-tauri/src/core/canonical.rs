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
        if fs::symlink_metadata(&root)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(AppError::invalid_input(
                "Managed skills root must not be a symlink or junction",
            ));
        }
        fs::create_dir_all(&root).map_err(AppError::io)?;
        let canonical = root
            .canonicalize()
            .map_err(|e| AppError::io(format!("Cannot resolve skills root {}: {e}", root.display())))?;
        if paths::identity_key(&root) != paths::identity_key(&canonical) {
            return Err(AppError::invalid_input(
                "Managed skills root must not be a symlink or junction",
            ));
        }
        Ok(Self { root, canonical })
    }
}

/// Resolve the user-level canonical root (`~/.agents/skills`).
///
/// Honors the skills-dir override when one is set (CLI `--skills-root`
/// external checkouts, test isolation); otherwise the canonical location.
pub fn resolve_user_root() -> Result<ResolvedRoot, AppError> {
    if let Some(path) = super::central_repo::skills_dir_override() {
        return ResolvedRoot::new(path);
    }
    ResolvedRoot::new(paths::user_agents_skills_dir())
}

pub(crate) fn ensure_project_workspace(
    record: &super::skill_store::ProjectRecord,
) -> Result<(), AppError> {
    if record.workspace_type != "project" {
        return Err(AppError::invalid_input(
            "Linked workspaces are no longer supported; register the repository as a project",
        ));
    }
    Ok(())
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
    ensure_project_workspace(&record)?;
    let root = paths::project_agents_skills_dir(Path::new(&record.path));
    Ok((record.path, ResolvedRoot::new(root)?))
}

/// Project root for **read** paths (inventory/discovery): returns the lexical
/// root without creating anything. Viewing a project must never create
/// `<repo>/.agents/skills` as a side effect.
pub fn resolve_project_root_for_read(
    store: &SkillStore,
    project_id: &str,
) -> Result<(String, PathBuf), AppError> {
    let record = store
        .get_project_by_id(project_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Project not found"))?;
    ensure_project_workspace(&record)?;
    let root = paths::project_agents_skills_dir(Path::new(&record.path));
    Ok((record.path, root))
}

/// Resolve and validate one existing canonical project skill without creating
/// the project root. This is the read-side counterpart to
/// [`resolve_project_root`].
pub fn resolve_existing_project_skill(
    store: &SkillStore,
    project_id: &str,
    skill_name: &str,
) -> Result<PathBuf, AppError> {
    let record = store
        .get_project_by_id(project_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Project not found"))?;
    ensure_project_workspace(&record)?;
    let root = paths::project_agents_skills_dir(Path::new(&record.path));
    if !root.is_dir() {
        return Err(AppError::not_found("Project skills directory not found"));
    }
    let resolved = ResolvedRoot::new(root)?;
    let skill_dir = skill_dir(&resolved, skill_name)?;
    validate_existing_skill(&resolved, &skill_dir)?;
    Ok(skill_dir)
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

    if dest.exists() || is_link(&dest) {
        if !replace {
            // Conflict check first so a cancel never deletes anything.
            if is_link(&dest) {
                return Err(AppError::invalid_input(POLICY_NO_SYMLINK));
            }
            if !dest.is_dir() {
                return Err(AppError::invalid_input(format!(
                    "Cannot install \"{clean}\": a file with the same name exists"
                )));
            }
            validate_existing_skill(resolved, &dest)?;
            return Err(already_managed_conflict(&dest, &clean));
        }
        // Replace path: the staged build below validates the source fully
        // before `swap_dir_staged` touches the existing destination, so a
        // failed copy can never lose the managed skill. Symlinks/junctions
        // squatting on the name are still refused outright.
        if is_link(&dest) {
            return Err(AppError::invalid_input(POLICY_NO_SYMLINK));
        }
        if dest.is_dir() {
            validate_existing_skill(resolved, &dest)?;
        } else {
            return Err(AppError::invalid_input(format!(
                "Cannot install \"{clean}\": a file with the same name exists"
            )));
        }
    }

    // Fail-safe mechanics: build + hash in a staged sibling, then move into
    // place (fresh rename, or backup + rollback swap for replace).
    let hash = super::staged::install_via_stage(source, &dest, replace)?;
    // Post-condition: what we just wrote resolves inside the root.
    validate_existing_skill(resolved, &dest)?;
    debug_assert_eq!(
        hash,
        content_hash::hash_directory(&dest).map_err(AppError::io)?,
        "staged install must land byte-identical content"
    );
    Ok(dest)
}

/// Build a canonical user skill in a sibling stage without replacing the live
/// directory yet. This is the safe preflight primitive for update/reimport
/// flows that must inspect removals or obtain approval before the swap.
pub fn stage_skill_dir_as(
    source: &Path,
    resolved: &ResolvedRoot,
    name: &str,
) -> Result<super::staged::StagedSkillDir, AppError> {
    if !source.is_dir() {
        return Err(AppError::not_found("Skill directory not found"));
    }
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
    if dest.exists() || is_link(&dest) {
        validate_existing_skill(resolved, &dest)?;
    }
    super::staged::stage_skill_dir(source, &dest)
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
    /// This item failed; every other item was still attempted.
    Failed,
}

/// One row of the migration report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MigrationEntry {
    pub name: String,
    pub legacy_path: String,
    pub canonical_path: String,
    pub outcome: MigrationOutcome,
    /// Hash of the canonical copy for adopted/migrated/updated rows.
    pub content_hash: Option<String>,
    /// Set only for `Failed` rows.
    pub error: Option<String>,
}

fn failed_entry(name: String, legacy_path: String, canonical_path: String, err: AppError) -> MigrationEntry {
    MigrationEntry {
        name,
        legacy_path,
        canonical_path,
        outcome: MigrationOutcome::Failed,
        content_hash: None,
        error: Some(err.to_string()),
    }
}

/// Migrate the pre-V1 library (`~/.skills-manager/skills`) into the canonical
/// user root (`~/.agents/skills`).
///
/// Per-item isolated: one skill failing (IO, hash, DB) is reported as a
/// `Failed` row and never stops the remaining items. Copies land via a staged
/// sibling and are hash-verified, so a failed copy leaves no half-written
/// skill behind (and a later run sees `missing`, not `conflict`).
/// Conflict-safe: an existing canonical copy with different content is never
/// overwritten. The legacy directory is never deleted here (P2 cleanup).
///
/// DB writes (relink adopted/migrated rows) go through the caller-supplied
/// `SkillStore` but are **not** finalized here: the command layer wraps this
/// in the repo lock and persists `sync_metadata` once at the end.
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
    let read_dir = fs::read_dir(&legacy_root).map_err(AppError::io)?;
    for dir_entry in read_dir {
        let legacy_dir = match dir_entry {
            Ok(entry) => entry.path(),
            Err(err) => {
                report.push(MigrationEntry {
                    name: "<unreadable-entry>".to_string(),
                    legacy_path: legacy_root.display().to_string(),
                    canonical_path: String::new(),
                    outcome: MigrationOutcome::Failed,
                    content_hash: None,
                    error: Some(format!("Cannot list legacy library entry: {err}")),
                });
                continue;
            }
        };
        if !legacy_dir.is_dir() || is_link(&legacy_dir) {
            continue;
        }
        if !skill_metadata::is_valid_skill_dir(&legacy_dir) {
            continue;
        }
        report.push(migrate_one_legacy_skill(
            store,
            &resolved,
            &records,
            &legacy_dir,
        ));
    }
    Ok(report)
}

fn migrate_one_legacy_skill(
    store: &SkillStore,
    resolved: &ResolvedRoot,
    records: &[super::skill_store::SkillRecord],
    legacy_dir: &Path,
) -> MigrationEntry {
    let failed = |name: String, canonical_path: String, err: AppError| {
        failed_entry(name, legacy_dir.display().to_string(), canonical_path, err)
    };
    let name = match sanitize_component(&skill_metadata::infer_skill_name(legacy_dir)) {
        Ok(name) => name,
        Err(err) => {
            return failed(
                legacy_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "<unknown>".to_string()),
                String::new(),
                err,
            )
        }
    };
    let dest = resolved.root.join(&name);
    let legacy_hash = match content_hash::hash_directory(legacy_dir).map_err(AppError::io) {
        Ok(hash) => hash,
        Err(err) => return failed(name, dest.display().to_string(), err),
    };

    let matching: Vec<_> = records
        .iter()
        .filter(|r| {
            paths::identity_key(Path::new(&r.central_path)) == paths::identity_key(legacy_dir)
        })
        .collect();
    let relink = |hash: &str| -> Result<(), AppError> {
        for record in &matching {
            store
                .update_skill_central_path(
                    &record.id,
                    &dest.display().to_string(),
                    Some(hash),
                )
                .map_err(AppError::db)?;
        }
        Ok(())
    };

    if dest.exists() || is_link(&dest) {
        if is_link(&dest) {
            return failed(
                name,
                dest.display().to_string(),
                AppError::invalid_input(POLICY_NO_SYMLINK),
            );
        }
        if let Err(err) = validate_existing_skill(resolved, &dest) {
            return failed(name, dest.display().to_string(), err);
        }
        let dest_hash = match content_hash::hash_directory(&dest).map_err(AppError::io) {
            Ok(hash) => hash,
            Err(err) => return failed(name, dest.display().to_string(), err),
        };
        if dest_hash == legacy_hash {
            if let Err(err) = relink(&dest_hash) {
                return failed(name, dest.display().to_string(), err);
            }
            return MigrationEntry {
                name,
                legacy_path: legacy_dir.display().to_string(),
                canonical_path: dest.display().to_string(),
                outcome: if matching.is_empty() {
                    MigrationOutcome::Adopted
                } else {
                    MigrationOutcome::UpdatedDbOnly
                },
                content_hash: Some(dest_hash),
                error: None,
            };
        }
        return MigrationEntry {
            name,
            legacy_path: legacy_dir.display().to_string(),
            canonical_path: dest.display().to_string(),
            outcome: MigrationOutcome::Conflict,
            content_hash: Some(dest_hash),
            error: None,
        };
    }

    if let Err(err) = validate_new_destination(resolved, &dest) {
        return failed(name, dest.display().to_string(), err);
    }
    // Staged, hash-verified copy: `dest` appears only when complete.
    let staged_hash = match super::staged::install_via_stage(legacy_dir, &dest, false) {
        Ok(hash) => hash,
        Err(err) => return failed(name, dest.display().to_string(), err),
    };
    if staged_hash != legacy_hash {
        let _ = super::staged::remove_path_if_exists(&dest);
        let message = format!("Migration copy verification failed for {name}");
        return failed(name, dest.display().to_string(), AppError::internal(message));
    }
    if let Err(err) = relink(&staged_hash) {
        return failed(name, dest.display().to_string(), err);
    }
    MigrationEntry {
        name,
        legacy_path: legacy_dir.display().to_string(),
        canonical_path: dest.display().to_string(),
        outcome: if matching.is_empty() {
            MigrationOutcome::Adopted
        } else {
            MigrationOutcome::Migrated
        },
        content_hash: Some(staged_hash),
        error: None,
    }
}

// ── SkillStore reconcile (user scope) ──
//
// The filesystem is authoritative for *what* is managed; the `SkillStore` is
// the index MySkills / update-checks read. Every user-scope canonical write
// must reconcile both under the repo lock, or the two drift apart:
//
// ```text
// RepoLock → filesystem mutation → SkillStore reconcile → sync_metadata
// ```
//
// Project scope intentionally has no records: it is pure filesystem.

/// Registration data for a user-scope canonical skill.
#[derive(Default)]
pub struct UserSkillRegistration {
    pub name: String,
    pub description: Option<String>,
    pub central_path: PathBuf,
    pub content_hash: String,
    /// e.g. `"adopted"`, `"migrated"`, `"skillssh"`.
    pub source_type: String,
    /// Where it came from (harness path, legacy path); shown in MySkills.
    pub source_ref: Option<String>,
    pub source_ref_resolved: Option<String>,
    pub source_subpath: Option<String>,
    pub source_branch: Option<String>,
    pub source_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub update_status: Option<String>,
}

/// Insert (or refresh) the `SkillStore` record for a canonical user skill.
///
/// Must be called with the repo lock held. Finalizes with `sync_metadata`.
/// Records whose `central_path` already points here are refreshed in place
/// (hash + source), never duplicated.
pub fn register_user_skill(
    store: &SkillStore,
    reg: &UserSkillRegistration,
) -> Result<String, AppError> {
    let central = reg.central_path.display().to_string();
    let update_status = reg.update_status.as_deref().unwrap_or("local_only");
    if let Some(existing) = store
        .get_skill_by_central_path(&central)
        .map_err(AppError::db)?
    {
        // Refresh the existing record in place (hash + source metadata);
        // never duplicate it.
        store
            .update_skill_after_reinstall(
                &existing.id,
                &reg.name,
                reg.description.as_deref(),
                &reg.source_type,
                reg.source_ref.as_deref(),
                reg.source_ref_resolved.as_deref(),
                reg.source_subpath.as_deref(),
                reg.source_branch.as_deref(),
                reg.source_revision.as_deref(),
                reg.remote_revision.as_deref(),
                Some(&reg.content_hash),
                update_status,
            )
            .map_err(AppError::db)?;
        super::sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        return Ok(existing.id);
    }
    // Fall back to identity match: an older record may spell the same
    // directory differently (case, separators) on this machine.
    let identity = super::paths::identity_key(&reg.central_path);
    let same_dir = store
        .get_all_skills()
        .map_err(AppError::db)?
        .into_iter()
        .find(|r| super::paths::identity_key(Path::new(&r.central_path)) == identity);
    if let Some(record) = same_dir {
        store
            .update_skill_after_reinstall(
                &record.id,
                &reg.name,
                reg.description.as_deref(),
                &reg.source_type,
                reg.source_ref.as_deref(),
                reg.source_ref_resolved.as_deref(),
                reg.source_subpath.as_deref(),
                reg.source_branch.as_deref(),
                reg.source_revision.as_deref(),
                reg.remote_revision.as_deref(),
                Some(&reg.content_hash),
                update_status,
            )
            .map_err(AppError::db)?;
        store
            .update_skill_central_path(&record.id, &central, Some(&reg.content_hash))
            .map_err(AppError::db)?;
        super::sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
        return Ok(record.id);
    }

    let now = chrono::Utc::now().timestamp_millis();
    let id = uuid::Uuid::new_v4().to_string();
    store
        .insert_skill(&super::skill_store::SkillRecord {
            id: id.clone(),
            name: reg.name.clone(),
            description: reg.description.clone(),
            source_type: reg.source_type.clone(),
            source_ref: reg.source_ref.clone(),
            source_ref_resolved: reg.source_ref_resolved.clone(),
            source_subpath: reg.source_subpath.clone(),
            source_branch: reg.source_branch.clone(),
            source_revision: reg.source_revision.clone(),
            remote_revision: reg.remote_revision.clone(),
            central_path: central,
            content_hash: Some(reg.content_hash.clone()),
            enabled: true,
            created_at: now,
            updated_at: now,
            status: "ok".to_string(),
            update_status: update_status.to_string(),
            last_checked_at: Some(now),
            last_check_error: None,
        })
        .map_err(AppError::db)?;
    store.log_audit(
        super::audit_log::AuditDraft::new("install")
            .detail(reg.source_type.clone())
            .skill(id.clone(), reg.name.clone())
            .ok(),
    );
    super::sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
    Ok(id)
}

/// Remove `SkillStore` records (and their deploy-target rows) for a canonical
/// directory that was just deleted. Returns the removed record count.
///
/// Must be called with the repo lock held. Touches the database only: harness
/// directories are observe-only in V1 and are never modified here, even when
/// stale `skill_targets` rows point at them.
pub fn remove_user_skill_records(
    store: &SkillStore,
    canonical_dir: &Path,
) -> Result<usize, AppError> {
    let identity = super::paths::identity_key(canonical_dir);
    let matching: Vec<_> = store
        .get_all_skills()
        .map_err(AppError::db)?
        .into_iter()
        .filter(|r| super::paths::identity_key(Path::new(&r.central_path)) == identity)
        .collect();
    if matching.is_empty() {
        return Ok(0);
    }
    for record in &matching {
        for target in store
            .get_targets_for_skill(&record.id)
            .map_err(AppError::db)?
        {
            store
                .delete_target(&record.id, &target.tool)
                .map_err(AppError::db)?;
        }
        store.delete_skill(&record.id).map_err(AppError::db)?;
        store.log_audit(
            super::audit_log::AuditDraft::new("remove")
                .skill(record.id.clone(), record.name.clone())
                .ok(),
        );
    }
    super::sync_metadata::write_all_from_db_unlocked(store).map_err(AppError::db)?;
    Ok(matching.len())
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
    fn replace_with_invalid_source_keeps_original() {
        // Source validation happens before the destination is touched: a
        // replace against an unusable source must fail with the managed skill
        // byte-identical to before.
        let tmp = tempdir().unwrap();
        let (_t, resolved) = test_root();
        let source = make_source(tmp.path(), "demo", "v1");
        let dest = install_skill_dir(&source, &resolved, false).unwrap();

        let bad = tmp.path().join("not-a-skill");
        fs::create_dir_all(&bad).unwrap();
        let err = install_skill_dir(&bad, &resolved, true).unwrap_err();
        assert!(matches!(
            err.kind,
            super::super::error::ErrorKind::InvalidInput
        ));
        assert!(fs::read_to_string(dest.join("SKILL.md"))
            .unwrap()
            .contains("v1"));
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

    #[test]
    fn linked_workspace_records_cannot_resolve_a_project_root() {
        use super::super::skill_store::{ProjectRecord, SkillStore};

        let tmp = tempdir().unwrap();
        let project_dir = tmp.path().join("legacy-linked");
        fs::create_dir_all(&project_dir).unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        store
            .insert_project(&ProjectRecord {
                id: "linked-1".to_string(),
                name: "legacy-linked".to_string(),
                path: project_dir.display().to_string(),
                workspace_type: "linked".to_string(),
                linked_agent_key: Some("claude_code".to_string()),
                linked_agent_name: Some("Claude Code".to_string()),
                disabled_path: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();

        let error = resolve_project_root(&store, "linked-1").unwrap_err();
        assert!(matches!(error.kind, super::super::error::ErrorKind::InvalidInput));
        assert!(resolve_project_root_for_read(&store, "linked-1").is_err());
    }

    struct IsolatedBase {
        _guard: std::sync::MutexGuard<'static, ()>,
        _tmp: tempfile::TempDir,
    }

    impl Drop for IsolatedBase {
        fn drop(&mut self) {
            super::super::central_repo::set_test_base_dir_override(None);
        }
    }

    struct SkillsDirGuard;
    impl Drop for SkillsDirGuard {
        fn drop(&mut self) {
            super::super::central_repo::set_runtime_skills_dir_override(None);
        }
    }

    /// Redirect metadata writes into a temp dir so reconcile tests never touch
    /// the real `~/.skills-manager`.
    fn isolated_base() -> IsolatedBase {
        let guard = super::super::central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        super::super::central_repo::set_test_base_dir_override(Some(
            tmp.path().join("base"),
        ));
        IsolatedBase { _guard: guard, _tmp: tmp }
    }

    #[test]
    fn register_then_remove_reconciles_records() {
        use super::super::skill_store::SkillStore;

        let _iso = isolated_base();
        let tmp = tempdir().unwrap();
        // Metadata validation requires record paths inside the skills root.
        let _skills_guard = SkillsDirGuard;
        super::super::central_repo::set_runtime_skills_dir_override(Some(
            tmp.path().to_path_buf(),
        ));
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let dir = make_source(tmp.path(), "reconciled", "body");
        let hash = super::super::content_hash::hash_directory(&dir).unwrap();

        let id = register_user_skill(
            &store,
            &UserSkillRegistration {
                name: "reconciled".to_string(),
                description: Some("desc".to_string()),
                central_path: dir.clone(),
                content_hash: hash.clone(),
                source_type: "skillssh".to_string(),
                source_ref: Some("owner/repo/skill".to_string()),
                source_ref_resolved: Some("https://example.test/repo.git".to_string()),
                source_subpath: Some("skills/skill".to_string()),
                source_branch: Some("main".to_string()),
                source_revision: Some("rev-1".to_string()),
                remote_revision: Some("rev-1".to_string()),
                update_status: Some("up_to_date".to_string()),
            },
        )
        .unwrap();
        // Re-registering the same directory refreshes in place, never duplicates.
        let id2 = register_user_skill(
            &store,
            &UserSkillRegistration {
                name: "reconciled".to_string(),
                description: Some("desc".to_string()),
                central_path: dir.clone(),
                content_hash: hash.clone(),
                source_type: "skillssh".to_string(),
                source_ref: Some("owner/repo/skill".to_string()),
                source_ref_resolved: Some("https://example.test/repo.git".to_string()),
                source_subpath: Some("skills/skill".to_string()),
                source_branch: Some("main".to_string()),
                source_revision: Some("rev-1".to_string()),
                remote_revision: Some("rev-1".to_string()),
                update_status: Some("up_to_date".to_string()),
            },
        )
        .unwrap();
        assert_eq!(id, id2);
        assert_eq!(store.get_all_skills().unwrap().len(), 1);
        let record = store.get_skill_by_id(&id).unwrap().unwrap();
        assert_eq!(record.source_type, "skillssh");
        assert_eq!(record.source_subpath.as_deref(), Some("skills/skill"));
        assert_eq!(record.remote_revision.as_deref(), Some("rev-1"));
        assert_eq!(record.update_status, "up_to_date");

        let removed = remove_user_skill_records(&store, &dir).unwrap();
        assert_eq!(removed, 1);
        assert!(store.get_all_skills().unwrap().is_empty());
        // Nothing left to remove is a no-op, not an error.
        assert_eq!(remove_user_skill_records(&store, &dir).unwrap(), 0);
    }

    #[test]
    fn migration_item_failure_does_not_stop_others() {
        use super::super::skill_store::SkillStore;

        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let legacy = tmp.path().join("legacy");
        let (_t, resolved) = test_root();
        let good = make_source(&legacy, "good", "body");
        let records = store.get_all_skills().unwrap();

        let ok_entry = migrate_one_legacy_skill(&store, &resolved, &records, &good);
        assert_eq!(ok_entry.outcome, MigrationOutcome::Adopted);
        assert!(ok_entry.content_hash.is_some());

        // A legacy dir whose destination is blocked produces Failed, and the
        // previously migrated skill is untouched by that failure.
        let blocker = resolved.root.join("blocked");
        fs::write(&blocker, "squatting file").unwrap();
        let legacy_blocked = make_source(&legacy, "blocked", "body");
        let failed_entry =
            migrate_one_legacy_skill(&store, &resolved, &records, &legacy_blocked);
        assert_eq!(failed_entry.outcome, MigrationOutcome::Failed);
        assert!(failed_entry.error.is_some());
        assert!(resolved.root.join("good").join("SKILL.md").exists());
    }
}
