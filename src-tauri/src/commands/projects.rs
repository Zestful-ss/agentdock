use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde::Serialize;
use tauri::State;

use crate::core::skill_store::{ProjectRecord, SkillRecord, SkillStore};
use crate::core::timing::should_log_first_or_slow;
use crate::core::{canonical, content_hash, error::AppError, project_scanner, skill_metadata, sync_metadata};
#[cfg(test)]
use crate::core::sync_engine;

#[derive(Serialize, Default)]
pub struct SyncHealthDto {
    pub in_sync: usize,
    pub project_newer: usize,
    pub center_newer: usize,
    pub diverged: usize,
    pub project_only: usize,
}

#[derive(Serialize)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub workspace_type: String,
    pub linked_agent_name: Option<String>,
    pub supports_skill_toggle: bool,
    pub sort_order: i32,
    pub skill_count: usize,
    pub sync_health: SyncHealthDto,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
pub struct ProjectSkillDocumentDto {
    pub skill_name: String,
    pub filename: String,
    pub content: String,
}

/// V1 project skills root is always `<repo>/.agents/skills`.
/// Harness adapter dirs are never listed or written for project workspaces.
fn canonical_skill_config() -> project_scanner::AgentSkillConfig {
    project_scanner::AgentSkillConfig {
        key: "agents".to_string(),
        display_name: "Project".to_string(),
        relative_skills_dir: ".agents/skills".to_string(),
    }
}

fn read_workspace_skills(rec: &ProjectRecord) -> Vec<project_scanner::ProjectSkillInfo> {
    // V1: one canonical root only (`<repo>/.agents/skills`). Linked workspaces
    // resolve the same way as writes (`canonical::resolve_project_root`), so
    // read and document/update/delete always hit the same row.
    project_scanner::read_project_skills(Path::new(&rec.path), &[canonical_skill_config()])
}

/// Resolve the canonical skills root for a workspace (V1: `<repo>/.agents/skills`).
fn resolve_canonical_skills_roots(rec: &ProjectRecord) -> (PathBuf, Option<PathBuf>) {
    let skills_root = crate::core::paths::project_agents_skills_dir(Path::new(&rec.path));
    // Canonical layout has no harness `-disabled` sibling; kept for call-site shape.
    (skills_root, None)
}

/// Convert a project record into its DTO, folding the copies of one logical
/// skill across agents together by relative path.
fn project_to_dto(rec: &ProjectRecord, all_managed: &[SkillRecord]) -> ProjectDto {
    let skills = read_workspace_skills(rec);
    let mut grouped_statuses: HashMap<String, String> = HashMap::new();

    for skill in &skills {
        let matched = find_best_center_match(skill, all_managed);
        let status = classify_sync_status(skill, matched);
        let key = skill.relative_path.to_lowercase();
        let existing = grouped_statuses
            .entry(key)
            .or_insert_with(|| status.clone());
        if sync_status_priority(&status) > sync_status_priority(existing) {
            *existing = status;
        }
    }

    let skill_count = grouped_statuses.len();
    let mut health = SyncHealthDto::default();
    for status in grouped_statuses.values() {
        match status.as_str() {
            "in_sync" => health.in_sync += 1,
            "project_newer" => health.project_newer += 1,
            "center_newer" => health.center_newer += 1,
            "diverged" => health.diverged += 1,
            _ => health.project_only += 1,
        }
    }

    ProjectDto {
        id: rec.id.clone(),
        name: rec.name.clone(),
        path: rec.path.clone(),
        workspace_type: rec.workspace_type.clone(),
        linked_agent_name: rec.linked_agent_name.clone(),
        supports_skill_toggle: rec.workspace_type != "linked" || rec.disabled_path.is_some(),
        sort_order: rec.sort_order,
        skill_count,
        sync_health: health,
        created_at: rec.created_at,
        updated_at: rec.updated_at,
    }
}

/// Severity of a sync status, used to reduce one logical skill's per-agent
/// copies to a single verdict: the worst one the group carries.
fn sync_status_priority(status: &str) -> u8 {
    match status {
        "diverged" => 5,
        "project_newer" => 4,
        "center_newer" => 3,
        "project_only" => 2,
        "in_sync" => 1,
        _ => 0,
    }
}

pub(crate) fn ensure_safe_skill_relative_path(skill_relative_path: &str) -> Result<(), AppError> {
    if skill_relative_path.trim().is_empty() {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    let mut saw_component = false;
    for component in Path::new(skill_relative_path).components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(AppError::invalid_input("Invalid skill directory path"));
        }
        saw_component = true;
    }
    if !saw_component {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    Ok(())
}

pub(crate) fn ensure_dir_within_root(path: &Path, root: &Path) -> Result<(), AppError> {
    // First check that the lexical path (before symlink resolution) is under root.
    // This ensures the link itself lives where expected.
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let abs_root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };
    if !abs_path.starts_with(&abs_root) {
        return Err(AppError::invalid_input("Invalid skill directory path"));
    }
    Ok(())
}

#[cfg(test)]
fn remove_workspace_skill_target(path: &Path) -> Result<(), AppError> {
    sync_engine::remove_target(path).map_err(AppError::io)
}

// Walks upward from `start`, removing each empty directory until reaching
// (and including) `root`. Stops at the first non-empty directory or any
// other error. `fs::remove_dir` only succeeds on empty directories, so this
// will never delete a directory that still holds skills.
#[cfg(test)]
fn cleanup_empty_dirs_up_to(start: &Path, root: &Path) {
    let Ok(root_canonical) = std::fs::canonicalize(root) else {
        return;
    };
    let mut current = start.to_path_buf();
    loop {
        let Ok(current_canonical) = std::fs::canonicalize(&current) else {
            return;
        };
        if !current_canonical.starts_with(&root_canonical) {
            return;
        }
        if std::fs::remove_dir(&current).is_err() {
            return;
        }
        if current_canonical == root_canonical {
            return;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => return,
        }
    }
}

#[cfg(test)]
fn remove_symlink_entry(path: &Path) -> Result<(), AppError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(AppError::io(err)),
    };
    if !metadata.file_type().is_symlink() {
        return Err(AppError::invalid_input(
            "Duplicate skill entry is not a symlink — resolve manually",
        ));
    }
    sync_engine::remove_target(path).map_err(AppError::io)
}

#[cfg(test)]
fn set_project_skill_enabled_state(
    skills_dir: &Path,
    disabled_dir: &Path,
    skill_relative_path: &str,
    enabled: bool,
) -> Result<(), AppError> {
    ensure_safe_skill_relative_path(skill_relative_path)?;

    let enabled_path = skills_dir.join(skill_relative_path);
    let disabled_path = disabled_dir.join(skill_relative_path);

    if enabled {
        if enabled_path.is_dir() {
            ensure_dir_within_root(&enabled_path, skills_dir)?;
            if disabled_path.exists() {
                ensure_dir_within_root(&disabled_path, disabled_dir)?;
                remove_symlink_entry(&disabled_path)?;
                if let Some(parent) = disabled_path.parent() {
                    cleanup_empty_dirs_up_to(parent, disabled_dir);
                }
            }
            return Ok(());
        }

        if !disabled_path.is_dir() {
            return Err(AppError::not_found(
                "Skill directory not found in skills-disabled",
            ));
        }
        ensure_dir_within_root(&disabled_path, disabled_dir)?;
        if let Some(parent) = enabled_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if enabled_path.exists() {
            return Err(AppError::invalid_input(
                "Skill already exists in skills directory",
            ));
        }
        std::fs::rename(&disabled_path, &enabled_path)?;
        if let Some(parent) = disabled_path.parent() {
            cleanup_empty_dirs_up_to(parent, disabled_dir);
        }
        return Ok(());
    }

    if disabled_path.is_dir() {
        ensure_dir_within_root(&disabled_path, disabled_dir)?;
        if enabled_path.exists() {
            ensure_dir_within_root(&enabled_path, skills_dir)?;
            remove_symlink_entry(&enabled_path)?;
        }
        return Ok(());
    }

    if !enabled_path.is_dir() {
        return Err(AppError::not_found("Skill directory not found"));
    }
    ensure_dir_within_root(&enabled_path, skills_dir)?;
    if let Some(parent) = disabled_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if disabled_path.exists() {
        return Err(AppError::invalid_input(
            "Skill already exists in skills-disabled directory",
        ));
    }
    std::fs::rename(&enabled_path, &disabled_path)?;
    Ok(())
}

#[cfg(test)]
fn ensure_distinct_linked_workspace_roots(
    skills_root: &Path,
    disabled_root: &Path,
) -> Result<(), AppError> {
    let skills_canonical = std::fs::canonicalize(skills_root)?;
    let disabled_canonical = std::fs::canonicalize(disabled_root)?;

    if skills_canonical == disabled_canonical
        || skills_canonical.starts_with(&disabled_canonical)
        || disabled_canonical.starts_with(&skills_canonical)
    {
        return Err(AppError::invalid_input(
            "Skills directory and disabled skills directory must not overlap",
        ));
    }

    Ok(())
}

pub(crate) fn slugify_skill_dir_name(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.chars().flat_map(|c| c.to_lowercase()) {
        let valid = ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.';
        if valid {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '_' || c == '.');
    if trimmed.is_empty() {
        "skill".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn source_ref_matches_skill_path(
    skill_path: &str,
    skill_canonical: Option<&PathBuf>,
    managed: &SkillRecord,
) -> bool {
    let Some(source_ref) = managed.source_ref.as_deref() else {
        return false;
    };
    if source_ref == skill_path {
        return true;
    }
    let Some(skill_canonical) = skill_canonical else {
        return false;
    };
    let Ok(source_canonical) = std::fs::canonicalize(source_ref) else {
        return false;
    };
    source_canonical == *skill_canonical
}

pub(crate) fn find_best_center_match<'a>(
    skill: &project_scanner::ProjectSkillInfo,
    all_managed: &'a [SkillRecord],
) -> Option<&'a SkillRecord> {
    let skill_hash = skill.content_hash.as_deref();
    let canonical_skill_path = std::fs::canonicalize(&skill.path).ok();

    // source_ref is the strongest direct link there is.
    if let Some(managed) = all_managed.iter().find(|managed| {
        source_ref_matches_skill_path(&skill.path, canonical_skill_path.as_ref(), managed)
    }) {
        return Some(managed);
    }

    // A content hash that names exactly one library skill outranks any
    // directory or name evidence: it says the two directories hold the same
    // bytes, while a directory name only says they were once called the same
    // thing. An export written under a slugified name can land on a directory
    // that now reads as a *different* skill (a library holding both
    // "Code Review" and "code-review" exports the first as `code-review`),
    // and this match also decides where an import writes back — binding the
    // wrong row there overwrites the other skill.
    //
    // Only a unique hash qualifies. Several skills sharing one hash is the
    // arbitrary-pick this fix exists to remove, and those fall through to the
    // directory and name layers below.
    if let Some(hash) = skill_hash {
        let mut by_hash = all_managed
            .iter()
            .filter(|managed| managed.content_hash.as_deref() == Some(hash));
        if let Some(first) = by_hash.next() {
            if by_hash.next().is_none() {
                return Some(first);
            }
        }
    }

    // The central directory name is steadier than the frontmatter name:
    // several skills shipped from one repo can share the latter.
    let by_central_dir: Vec<&SkillRecord> = all_managed
        .iter()
        .filter(|managed| {
            Path::new(&managed.central_path)
                .file_name()
                .map(|name| name.to_string_lossy().eq_ignore_ascii_case(&skill.dir_name))
                .unwrap_or(false)
        })
        .collect();
    if let Some(managed) = unique_center_match(&by_central_dir, skill_hash) {
        return Some(managed);
    }

    // Covers the ordinary skill whose name and directory agree.
    let by_name: Vec<&SkillRecord> = all_managed
        .iter()
        .filter(|managed| {
            slugify_skill_dir_name(&managed.name).eq_ignore_ascii_case(&skill.dir_name)
        })
        .collect();
    if let Some(managed) = unique_center_match(&by_name, skill_hash) {
        return Some(managed);
    }

    None
}

/// Pick the one skill among candidates sharing an identity signal,
/// disambiguating by content hash when the signal alone leaves several.
fn unique_center_match<'a>(
    candidates: &[&'a SkillRecord],
    skill_hash: Option<&str>,
) -> Option<&'a SkillRecord> {
    match candidates.len() {
        0 => None,
        1 => Some(candidates[0]),
        _ => {
            let hash = skill_hash?;
            let mut filtered = candidates
                .iter()
                .copied()
                .filter(|managed| managed.content_hash.as_deref() == Some(hash));
            let first = filtered.next()?;
            filtered.next().is_none().then_some(first)
        }
    }
}

pub(crate) fn classify_sync_status(
    skill: &project_scanner::ProjectSkillInfo,
    managed: Option<&SkillRecord>,
) -> String {
    let Some(managed) = managed else {
        return "project_only".to_string();
    };

    // Fast path: compare project hash against DB-stored center hash
    if skill.content_hash.is_some()
        && managed.content_hash.as_deref() == skill.content_hash.as_deref()
    {
        return "in_sync".to_string();
    }

    // The DB hash may be stale, and `updated_at` is the wrong clock for the
    // comparison further down, so read the center from disk once and answer
    // both questions from the same walk.
    let center_entries =
        crate::core::content_hash::list_content_files(Path::new(&managed.central_path));

    if let Some(project_hash) = skill.content_hash.as_deref() {
        if project_hash == crate::core::content_hash::hash_entries(&center_entries) {
            return "in_sync".to_string();
        }
    }

    let Some(project_modified_at) = skill.last_modified_at else {
        return "diverged".to_string();
    };

    // The project side is a filesystem mtime, so the center has to be one too.
    // `updated_at` is a database column stamped when the row was written:
    // editing files in the library does not move it, and a metadata-only write
    // moves it while no content changed. Comparing the two rulers reported
    // "center is newer" for a project copy the user had just edited, and that
    // status invites a pull, which overwrites the edit — the diagnosis behind
    // #328.
    let Some(center_modified_at) = crate::core::content_hash::latest_modified_ms(&center_entries)
    else {
        return "diverged".to_string();
    };
    let threshold_ms = 1_000;
    if project_modified_at > center_modified_at + threshold_ms {
        "project_newer".to_string()
    } else if center_modified_at > project_modified_at + threshold_ms {
        "center_newer".to_string()
    } else {
        "diverged".to_string()
    }
}

static GET_PROJECTS_FIRST_CALL: AtomicBool = AtomicBool::new(true);

#[tauri::command]
pub async fn get_projects(store: State<'_, Arc<SkillStore>>) -> Result<Vec<ProjectDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let records = store.get_all_projects().map_err(AppError::db)?;
        let all_managed = store.get_all_skills().map_err(AppError::db)?;
        let count = records.len();
        let dtos: Vec<ProjectDto> = records
            .iter()
            .map(|r| project_to_dto(r, &all_managed))
            .collect();
        let elapsed_ms = start.elapsed().as_millis();
        if should_log_first_or_slow(&GET_PROJECTS_FIRST_CALL, elapsed_ms, 100) {
            log::info!("get_projects: {count} projects in {elapsed_ms} ms");
        }
        Ok(dtos)
    })
    .await?
}

fn initialize_canonical_project_root(path: &Path) -> Result<PathBuf, AppError> {
    let project_path = std::fs::canonicalize(path)
        .map_err(|_| AppError::invalid_input("Directory does not exist"))?;
    let skills_dir = crate::core::paths::project_agents_skills_dir(&project_path);
    std::fs::create_dir_all(&skills_dir)?;
    Ok(project_path)
}

#[tauri::command]
pub async fn add_project(
    store: State<'_, Arc<SkillStore>>,
    path: String,
) -> Result<ProjectDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let project_path = initialize_canonical_project_root(Path::new(&path))?;

        let name = project_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let now = chrono::Utc::now().timestamp_millis();
        let record = ProjectRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            path: project_path.to_string_lossy().to_string(),
            workspace_type: "project".to_string(),
            linked_agent_key: None,
            linked_agent_name: None,
            disabled_path: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };

        store.insert_project(&record).map_err(AppError::db)?;
        let all_managed = store.get_all_skills().map_err(AppError::db)?;
        Ok(project_to_dto(&record, &all_managed))
    })
    .await?
}

#[tauri::command]
pub async fn remove_project(store: State<'_, Arc<SkillStore>>, id: String) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.delete_project(&id).map_err(AppError::db))
        .await?
}

#[tauri::command]
pub async fn reorder_projects(
    ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.reorder_projects(&ids).map_err(AppError::db))
        .await?
}

#[tauri::command]
pub async fn scan_projects(root: String) -> Result<Vec<String>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let root_path = Path::new(&root);
        if !root_path.is_dir() {
            return Err(AppError::invalid_input("Directory does not exist"));
        }
        // Project discovery is canonical-only. Harness-specific project paths
        // are observation sources, not project membership criteria.
        Ok(project_scanner::scan_projects_in_dir(
            root_path,
            4,
            &[canonical_skill_config()],
        ))
    })
    .await?
}

#[tauri::command]
pub async fn get_project_skills(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
) -> Result<Vec<project_scanner::ProjectSkillInfo>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let mut skills = read_workspace_skills(&record);

        let all_managed = store.get_all_skills().unwrap_or_default();
        let tags_map = store.get_tags_map().unwrap_or_default();
        for skill in &mut skills {
            let matched = find_best_center_match(skill, &all_managed);
            skill.in_center = matched.is_some();
            skill.center_skill_id = matched.map(|m| m.id.clone());
            skill.tags = skill
                .center_skill_id
                .as_ref()
                .and_then(|skill_id| tags_map.get(skill_id).cloned())
                .unwrap_or_default();
            skill.sync_status = classify_sync_status(skill, matched);
        }

        Ok(skills)
    })
    .await?
}

#[tauri::command]
pub async fn get_project_skill_document(
    project_id: String,
    skill_relative_path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ProjectSkillDocumentDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_relative_path(&skill_relative_path)?;

        // V1: one canonical root; validate the exact existing directory with
        // the same path guard used by managed writes.
        let skill_dir = canonical::resolve_existing_project_skill(
            &store,
            &project_id,
            &skill_relative_path,
        )?;

        let candidates = ["SKILL.md", "skill.md", "CLAUDE.md", "README.md"];
        for candidate in &candidates {
            let file_path = skill_dir.join(candidate);
            if !file_path.exists() {
                continue;
            }
            if file_path.is_file() {
                let content = std::fs::read_to_string(&file_path)?;
                return Ok(ProjectSkillDocumentDto {
                    skill_name: skill_relative_path,
                    filename: candidate.to_string(),
                    content,
                });
            }
        }

        Err(AppError::not_found(
            "No document file found in skill directory",
        ))
    })
    .await?
}

#[tauri::command]
pub async fn import_project_skill_to_center(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        import_project_skill_to_center_blocking(&store, &project_id, &skill_relative_path)
    })
    .await?
}

fn import_project_skill_to_center_blocking(
    store: &SkillStore,
    project_id: &str,
    skill_relative_path: &str,
) -> Result<(), AppError> {
    ensure_safe_skill_relative_path(skill_relative_path)?;

    let record = store
        .get_project_by_id(project_id)
        .map_err(AppError::db)?
        .ok_or_else(|| AppError::not_found("Workspace not found"))?;

    let skills = read_workspace_skills(&record);
    let skill = skills
        .iter()
        .find(|s| s.relative_path == skill_relative_path)
        .ok_or_else(|| AppError::not_found("Skill not found in workspace"))?;

    import_project_skill_info(store, skill)
}

/// RepoLock → fs → SkillStore → sync_metadata, matching every other
/// Project → User entry (Git / Local / Adopt / Migration).
fn import_project_skill_info(
    store: &SkillStore,
    skill: &project_scanner::ProjectSkillInfo,
) -> Result<(), AppError> {
    sync_metadata::with_repo_lock("import project skill to center", || {
        let root = canonical::resolve_user_root()?;
        let source_path = PathBuf::from(&skill.path);
        let all_managed = store.get_all_skills()?;

        if let Some(existing) = find_best_center_match(skill, &all_managed) {
            // Replace in place under the user root. Prefer the directory
            // component already stored when it lives under the root so the
            // record's central_path stays stable; otherwise target under
            // root by the project directory name (V1: all managed writes
            // land in `~/.agents/skills`).
            let central = Path::new(&existing.central_path);
            let dir_name = if central.starts_with(&root.root) {
                central
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| existing.name.clone())
            } else {
                skill.dir_name.clone()
            };
            let installed =
                canonical::install_skill_dir_as(&source_path, &root, &dir_name, true)?;
            let hash = content_hash::hash_directory(&installed).map_err(AppError::io)?;
            let meta = skill_metadata::parse_skill_md(&installed);
            store.update_skill_after_install(
                &existing.id,
                &existing.name,
                meta.description.as_deref(),
                existing.source_revision.as_deref(),
                existing.remote_revision.as_deref(),
                Some(&hash),
                "local_only",
            )?;
            let new_path = installed.display().to_string();
            if existing.central_path != new_path {
                store.update_skill_central_path(&existing.id, &new_path, Some(&hash))?;
            }
            // Only update source_ref when the match was already by source_ref
            // path (not by hash or name). This avoids permanently rebinding
            // unrelated center skills that merely share a name or content.
            let already_matched_by_ref = source_ref_matches_skill_path(
                &skill.path,
                std::fs::canonicalize(&skill.path).ok().as_ref(),
                existing,
            );
            if existing.source_type == "local" && already_matched_by_ref {
                store.update_skill_source_ref(&existing.id, &skill.path)?;
            }
            sync_metadata::write_all_from_db_unlocked(store)?;
            return Ok(());
        }

        let clean = canonical::sanitize_component(&skill.dir_name)?;
        let installed = canonical::install_skill_dir_as(&source_path, &root, &clean, false)?;
        let hash = content_hash::hash_directory(&installed).map_err(AppError::io)?;
        let meta = skill_metadata::parse_skill_md(&installed);
        let name = installed
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| clean.clone());
        canonical::register_user_skill(
            store,
            &canonical::UserSkillRegistration {
                name,
                description: meta.description,
                central_path: installed,
                content_hash: hash,
                source_type: "local".to_string(),
                source_ref: Some(skill.path.clone()),
                ..Default::default()
            },
        )
        .map_err(anyhow::Error::from)?;
        Ok(())
    })
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn update_project_skill_to_center(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
) -> Result<(), AppError> {
    import_project_skill_to_center(store, project_id, skill_relative_path).await
}

#[tauri::command]
pub fn slugify_skill_names(names: Vec<String>) -> Vec<String> {
    names.iter().map(|n| slugify_skill_dir_name(n)).collect()
}

#[tauri::command]
pub async fn export_skill_to_project(
    store: State<'_, Arc<SkillStore>>,
    skill_id: String,
    project_id: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let skill = store
            .get_skill_by_id(&skill_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Skill not found"))?;
        let source = PathBuf::from(&skill.central_path);
        // Backend-resolved canonical project root; harness dirs are never targets.
        let (_, resolved) = crate::core::canonical::resolve_project_root(&store, &project_id)?;
        // Missing → install; existing → target_conflict (frontend: Replace/Cancel).
        crate::core::canonical::install_skill_dir(&source, &resolved, false)?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn update_project_skill_from_center(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_safe_skill_relative_path(&skill_relative_path)?;

        let record = store
            .get_project_by_id(&project_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found("Workspace not found"))?;

        let skills = read_workspace_skills(&record);
        let skill = skills
            .iter()
            .find(|s| s.relative_path == skill_relative_path)
            .ok_or_else(|| AppError::not_found("Skill not found in workspace"))?;

        let all_managed = store.get_all_skills().unwrap_or_default();
        let managed = find_best_center_match(skill, &all_managed)
            .ok_or_else(|| AppError::not_found("No matching skill in center"))?;

        // Mirror the global-workspace protection (agent_workspace.rs): never
        // overwrite a project copy that has unsynced local edits (#225 review).
        if classify_sync_status(skill, Some(managed)) == "project_newer" {
            return Err(AppError::invalid_input(
                "Project skill is newer than the Skills Center version",
            ));
        }

        let (skills_root, _) = resolve_canonical_skills_roots(&record);
        let target_path = PathBuf::from(&skill.path);
        if target_path.starts_with(&skills_root) {
            ensure_dir_within_root(&target_path, &skills_root)?;
        } else {
            return Err(AppError::invalid_input("Invalid skill directory path"));
        }

        let source = PathBuf::from(&managed.central_path);
        // V1: the target must already be a managed skill inside the canonical
        // project root. Resolve backend-side; harness dirs are never targets.
        let (_, resolved) = crate::core::canonical::resolve_project_root(&store, &project_id)?;
        let target_name = crate::core::canonical::sanitize_component(&skill.dir_name)?;
        let target_path = resolved.root.join(&target_name);
        crate::core::canonical::validate_existing_skill(&resolved, &target_path)?;
        // UserConfirmed update: explicitly replace the canonical copy.
        crate::core::canonical::install_skill_dir_as(
            &source,
            &resolved,
            &target_name,
            true,
        )?;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn toggle_project_skill(
    _store: State<'_, Arc<SkillStore>>,
    _project_id: String,
    _skill_relative_path: String,
    _enabled: bool,
) -> Result<(), AppError> {
    Err(crate::core::v1::blocked_write())
}

#[tauri::command]
pub async fn delete_project_skill(
    store: State<'_, Arc<SkillStore>>,
    project_id: String,
    skill_relative_path: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Backend-resolved canonical project root; the relative path is
        // re-sanitized to a single component inside it.
        let (_, resolved) = crate::core::canonical::resolve_project_root(&store, &project_id)?;
        let name = crate::core::canonical::sanitize_component(&skill_relative_path)?;
        crate::core::canonical::delete_skill(&resolved, &name)?;
        Ok(())
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::{
        classify_sync_status, ensure_distinct_linked_workspace_roots, find_best_center_match,
        import_project_skill_info, import_project_skill_to_center_blocking,
        initialize_canonical_project_root, project_to_dto, remove_workspace_skill_target,
        set_project_skill_enabled_state,
    };
    use crate::core::content_hash;
    use crate::core::error::ErrorKind;
    use crate::core::project_scanner::ProjectSkillInfo;
    use crate::core::skill_store::{ProjectRecord, SkillRecord, SkillStore};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::MutexGuard;
    use tempfile::{TempDir, tempdir};

    #[test]
    fn adding_a_project_initializes_only_the_canonical_agents_root() {
        let tmp = tempdir().unwrap();
        let project = tmp.path().join("repo");
        fs::create_dir_all(&project).unwrap();

        let canonical = initialize_canonical_project_root(&project).unwrap();

        assert!(canonical.join(".agents").join("skills").is_dir());
        assert!(!canonical.join(".claude").exists());
        assert!(!canonical.join(".claude").join("skills").exists());
        assert!(!canonical.join(".claude").join("skills-disabled").exists());
    }

    fn sample_managed_skill(
        central_path: String,
        content_hash: Option<String>,
        updated_at: i64,
    ) -> SkillRecord {
        SkillRecord {
            id: "skill-1".to_string(),
            name: "Example Skill".to_string(),
            description: None,
            source_type: "local".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path,
            content_hash,
            enabled: true,
            created_at: 0,
            updated_at,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    /// Build a library skill with the given identity fields, so a test can
    /// assert the order the layers resolve in.
    fn managed_skill_with_identity(
        id: &str,
        name: &str,
        central_path: String,
        content_hash: Option<String>,
    ) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "skillssh".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path,
            content_hash,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "unknown".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    fn sample_project_skill(
        path: String,
        content_hash: Option<String>,
        last_modified_at: Option<i64>,
    ) -> ProjectSkillInfo {
        ProjectSkillInfo {
            name: "Example Skill".to_string(),
            dir_name: "example-skill".to_string(),
            relative_path: "example-skill".to_string(),
            description: None,
            path,
            files: vec!["SKILL.md".to_string()],
            enabled: true,
            agent: "claude_code".to_string(),
            agent_display_name: "Claude Code".to_string(),
            tags: Vec::new(),
            in_center: true,
            sync_status: "project_only".to_string(),
            center_skill_id: Some("skill-1".to_string()),
            last_modified_at,
            content_hash,
        }
    }

    /// Build a project skill with the given directory name and agent, to
    /// stand in for one per-agent copy.
    fn project_skill_with_dir(
        dir_name: &str,
        path: String,
        content_hash: Option<String>,
        agent: &str,
    ) -> ProjectSkillInfo {
        ProjectSkillInfo {
            name: dir_name.to_string(),
            dir_name: dir_name.to_string(),
            relative_path: dir_name.to_string(),
            description: None,
            path,
            files: vec!["SKILL.md".to_string()],
            enabled: true,
            agent: agent.to_string(),
            agent_display_name: agent.to_string(),
            tags: Vec::new(),
            in_center: false,
            sync_status: "project_only".to_string(),
            center_skill_id: None,
            last_modified_at: Some(1_000),
            content_hash,
        }
    }

    /// Directory identity must win over a content hash several skills share.
    #[test]
    fn find_best_center_match_prefers_directory_identity_over_shared_hash() {
        let shared_hash = Some("same-content-hash".to_string());
        let project = project_skill_with_dir(
            "adapt",
            "/tmp/project/.claude/skills/adapt".to_string(),
            shared_hash.clone(),
            "claude_code",
        );
        let all_managed = vec![
            managed_skill_with_identity(
                "adapt-id",
                "adapt",
                "/tmp/center/adapt".to_string(),
                shared_hash.clone(),
            ),
            managed_skill_with_identity(
                "polish-id",
                "polish",
                "/tmp/center/polish".to_string(),
                shared_hash,
            ),
        ];

        let matched = find_best_center_match(&project, &all_managed).unwrap();

        assert_eq!(matched.id, "adapt-id");
    }

    /// The central directory name must win over a frontmatter name that repeats.
    #[test]
    fn find_best_center_match_uses_central_directory_before_frontmatter_name() {
        let project = project_skill_with_dir(
            "adapt",
            "/tmp/project/.claude/skills/adapt".to_string(),
            None,
            "claude_code",
        );
        let all_managed = vec![
            managed_skill_with_identity(
                "adapt-id",
                "impeccable",
                "/tmp/center/adapt".to_string(),
                None,
            ),
            managed_skill_with_identity(
                "layout-id",
                "impeccable",
                "/tmp/center/layout".to_string(),
                None,
            ),
        ];

        let matched = find_best_center_match(&project, &all_managed).unwrap();

        assert_eq!(matched.id, "adapt-id");
    }

    /// A unique content hash outranks a directory name owned by a different
    /// skill. A library holding both "Code Review" and "code-review" exports
    /// the first under the slug `code-review`, which is the second one's
    /// library directory — matching on the directory binds the copy to the
    /// wrong row, and that row is also where an import writes back.
    #[test]
    fn find_best_center_match_prefers_a_unique_hash_over_another_skills_directory() {
        let exported_hash = Some("code-review-content".to_string());
        let project = project_skill_with_dir(
            "code-review",
            "/tmp/project/.claude/skills/code-review".to_string(),
            exported_hash.clone(),
            "claude_code",
        );
        let all_managed = vec![
            managed_skill_with_identity(
                "spaced-id",
                "Code Review",
                "/tmp/center/Code Review".to_string(),
                exported_hash,
            ),
            managed_skill_with_identity(
                "slug-id",
                "code-review",
                "/tmp/center/code-review".to_string(),
                Some("unrelated-content".to_string()),
            ),
        ];

        let matched = find_best_center_match(&project, &all_managed).unwrap();

        assert_eq!(matched.id, "spaced-id");
    }

    /// The sidebar project count reads only the canonical project root.
    #[test]
    fn project_to_dto_counts_logical_skills_not_agent_copies() {
        let tmp = tempdir().unwrap();
        let project_path = tmp.path().join("project");
        let canonical_skill = project_path.join(".agents/skills/shared-skill");
        fs::create_dir_all(&canonical_skill).unwrap();
        fs::write(canonical_skill.join("SKILL.md"), "# Shared\n").unwrap();

        let record = ProjectRecord {
            id: "project-1".to_string(),
            name: "Project".to_string(),
            path: project_path.to_string_lossy().to_string(),
            workspace_type: "project".to_string(),
            linked_agent_key: None,
            linked_agent_name: None,
            disabled_path: None,
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        };
        let dto = project_to_dto(&record, &[]);

        assert_eq!(dto.skill_count, 1);
        assert_eq!(dto.sync_health.project_only, 1);
    }

    #[test]
    fn classify_sync_status_uses_live_center_hash_when_db_hash_is_stale() {
        let center_dir = tempdir().unwrap();
        fs::write(center_dir.path().join("SKILL.md"), "# Example\n").unwrap();
        let live_hash = content_hash::hash_directory(center_dir.path()).unwrap();

        let managed = sample_managed_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some("stale-db-hash".to_string()),
            1_000,
        );
        let project = sample_project_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some(live_hash),
            Some(5_000),
        );

        assert_eq!(classify_sync_status(&project, Some(&managed)), "in_sync");
    }

    /// Newest content mtime of a directory, the same figure the project side
    /// is built from, so both sides of the comparison use one ruler.
    fn center_mtime_ms(dir: &std::path::Path) -> i64 {
        content_hash::latest_modified_ms(&content_hash::list_content_files(dir)).unwrap()
    }

    /// `updated_at` is a database column, not a filesystem mtime. With the
    /// project copy genuinely newer on disk, a much later `updated_at` must not
    /// flip the answer to "center_newer" — that reading invited a pull and
    /// overwrote the edit the user had just made (#328).
    #[test]
    fn classify_sync_status_ignores_the_db_column_when_the_project_is_newer_on_disk() {
        let center_dir = tempdir().unwrap();
        fs::write(center_dir.path().join("SKILL.md"), "# Center\n").unwrap();
        let center_mtime = center_mtime_ms(center_dir.path());

        let project_dir = tempdir().unwrap();
        fs::write(project_dir.path().join("SKILL.md"), "# Project changed\n").unwrap();
        let project_hash = content_hash::hash_directory(project_dir.path()).unwrap();

        let managed = sample_managed_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some("stale-db-hash".to_string()),
            center_mtime + 60_000,
        );
        let project = sample_project_skill(
            project_dir.path().to_string_lossy().to_string(),
            Some(project_hash),
            Some(center_mtime + 5_000),
        );

        assert_eq!(
            classify_sync_status(&project, Some(&managed)),
            "project_newer"
        );
    }

    /// The other direction, and the reason the fix is not simply "always say
    /// project_newer": a center that really is ahead still reports so, with an
    /// `updated_at` old enough that only the real mtime can produce it.
    #[test]
    fn classify_sync_status_reports_a_center_that_is_newer_on_disk() {
        let center_dir = tempdir().unwrap();
        fs::write(center_dir.path().join("SKILL.md"), "# Center\n").unwrap();
        let center_mtime = center_mtime_ms(center_dir.path());

        let project_dir = tempdir().unwrap();
        fs::write(project_dir.path().join("SKILL.md"), "# Project older\n").unwrap();
        let project_hash = content_hash::hash_directory(project_dir.path()).unwrap();

        let managed = sample_managed_skill(
            center_dir.path().to_string_lossy().to_string(),
            Some("stale-db-hash".to_string()),
            0,
        );
        let project = sample_project_skill(
            project_dir.path().to_string_lossy().to_string(),
            Some(project_hash),
            Some(center_mtime - 5_000),
        );

        assert_eq!(
            classify_sync_status(&project, Some(&managed)),
            "center_newer"
        );
    }

    #[test]
    fn linked_workspace_roots_reject_same_directory() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("skills");
        fs::create_dir_all(&root).unwrap();

        let err = ensure_distinct_linked_workspace_roots(&root, &root).unwrap_err();
        assert!(
            err.to_string().contains("must not overlap"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn linked_workspace_roots_reject_nested_directory() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("skills");
        let nested = root.join("disabled");
        fs::create_dir_all(&nested).unwrap();

        let err = ensure_distinct_linked_workspace_roots(&root, &nested).unwrap_err();
        assert!(
            err.to_string().contains("must not overlap"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn linked_workspace_roots_allow_distinct_directories() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("skills");
        let disabled = tmp.path().join("skills-disabled");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&disabled).unwrap();

        ensure_distinct_linked_workspace_roots(&root, &disabled).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn remove_workspace_skill_target_removes_symlink_without_touching_target() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real-skill");
        let link = tmp.path().join("linked-skill");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        remove_workspace_skill_target(&link).unwrap();

        assert!(!link.exists());
        assert!(real.exists());
        assert!(real.join("SKILL.md").exists());
    }

    #[cfg(windows)]
    #[test]
    fn remove_workspace_skill_target_removes_directory_symlink_without_touching_target() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real-skill");
        let link = tmp.path().join("linked-skill");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();

        remove_workspace_skill_target(&link).unwrap();

        assert!(!link.exists());
        assert!(real.exists());
        assert!(real.join("SKILL.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn set_project_skill_enabled_state_disabling_cleans_duplicate_symlink_without_touching_target()
    {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let central_skill = tmp.path().join("central").join("understand-diff");
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "understand-diff";

        fs::create_dir_all(&central_skill).unwrap();
        fs::write(
            central_skill.join("SKILL.md"),
            "---\nname: understand-diff\n---\n",
        )
        .unwrap();
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&disabled_root).unwrap();

        symlink(&central_skill, skills_root.join(relative_path)).unwrap();
        symlink(&central_skill, disabled_root.join(relative_path)).unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, false)
            .unwrap();

        assert!(!skills_root.join(relative_path).exists());
        assert!(disabled_root.join(relative_path).exists());
        assert!(central_skill.exists());
        assert!(central_skill.join("SKILL.md").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn set_project_skill_enabled_state_enabling_cleans_duplicate_symlink_without_touching_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().unwrap();
        let central_skill = tmp.path().join("central").join("understand-diff");
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "understand-diff";

        fs::create_dir_all(&central_skill).unwrap();
        fs::write(
            central_skill.join("SKILL.md"),
            "---\nname: understand-diff\n---\n",
        )
        .unwrap();
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&disabled_root).unwrap();

        symlink(&central_skill, skills_root.join(relative_path)).unwrap();
        symlink(&central_skill, disabled_root.join(relative_path)).unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true).unwrap();

        assert!(skills_root.join(relative_path).exists());
        assert!(!disabled_root.join(relative_path).exists());
        assert!(central_skill.exists());
        assert!(central_skill.join("SKILL.md").is_file());
    }

    #[test]
    fn set_project_skill_enabled_state_enabling_removes_emptied_disabled_dir() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "my-skill";

        let real_disabled = disabled_root.join(relative_path);
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&real_disabled).unwrap();
        fs::write(real_disabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true).unwrap();

        assert!(skills_root.join(relative_path).join("SKILL.md").is_file());
        assert!(!disabled_root.exists());
    }

    #[test]
    fn set_project_skill_enabled_state_enabling_keeps_disabled_dir_when_other_skills_remain() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "skill-a";

        let real_disabled_a = disabled_root.join(relative_path);
        let real_disabled_b = disabled_root.join("skill-b");
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&real_disabled_a).unwrap();
        fs::create_dir_all(&real_disabled_b).unwrap();
        fs::write(
            real_disabled_a.join("SKILL.md"),
            "---\nname: skill-a\n---\n",
        )
        .unwrap();
        fs::write(
            real_disabled_b.join("SKILL.md"),
            "---\nname: skill-b\n---\n",
        )
        .unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true).unwrap();

        assert!(skills_root.join(relative_path).join("SKILL.md").is_file());
        assert!(disabled_root.is_dir());
        assert!(real_disabled_b.join("SKILL.md").is_file());
    }

    #[test]
    fn set_project_skill_enabled_state_enabling_removes_empty_nested_disabled_dirs() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "category/sub/skill-a";

        let real_disabled = disabled_root.join(relative_path);
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&real_disabled).unwrap();
        fs::write(real_disabled.join("SKILL.md"), "---\nname: skill-a\n---\n").unwrap();

        set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true).unwrap();

        assert!(skills_root.join(relative_path).join("SKILL.md").is_file());
        assert!(!disabled_root.exists());
    }

    #[test]
    fn set_project_skill_enabled_state_rejects_real_dir_duplicate_on_enable() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "my-skill";

        let real_enabled = skills_root.join(relative_path);
        let real_disabled = disabled_root.join(relative_path);
        fs::create_dir_all(&real_enabled).unwrap();
        fs::write(real_enabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();
        fs::create_dir_all(&real_disabled).unwrap();
        fs::write(real_disabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();

        let err =
            set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, true)
                .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        // Both real dirs must still exist
        assert!(real_enabled.join("SKILL.md").exists());
        assert!(real_disabled.join("SKILL.md").exists());
    }

    #[test]
    fn set_project_skill_enabled_state_rejects_real_dir_duplicate_on_disable() {
        let tmp = tempdir().unwrap();
        let skills_root = tmp.path().join("skills");
        let disabled_root = tmp.path().join("skills-disabled");
        let relative_path = "my-skill";

        let real_enabled = skills_root.join(relative_path);
        let real_disabled = disabled_root.join(relative_path);
        fs::create_dir_all(&real_enabled).unwrap();
        fs::write(real_enabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();
        fs::create_dir_all(&real_disabled).unwrap();
        fs::write(real_disabled.join("SKILL.md"), "---\nname: my-skill\n---\n").unwrap();

        let err =
            set_project_skill_enabled_state(&skills_root, &disabled_root, relative_path, false)
                .unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert!(real_enabled.join("SKILL.md").exists());
        assert!(real_disabled.join("SKILL.md").exists());
    }

    /// Redirects `base_dir` + `skills_dir` into a temp tree so import tests
    /// never touch the real `~/.agents/skills` or `~/.skills-manager`.
    struct IsolatedImportBase {
        _guard: MutexGuard<'static, ()>,
        _tmp: TempDir,
        base: PathBuf,
        user_skills: PathBuf,
    }

    impl Drop for IsolatedImportBase {
        fn drop(&mut self) {
            crate::core::central_repo::set_runtime_skills_dir_override(None);
            crate::core::central_repo::set_test_base_dir_override(None);
        }
    }

    fn isolated_import_base() -> IsolatedImportBase {
        let guard = crate::core::central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        // Base first: `set_test_base_dir_override` clears the skills override.
        crate::core::central_repo::set_test_base_dir_override(Some(base.clone()));
        let user_skills = tmp.path().join("user-skills");
        crate::core::central_repo::set_runtime_skills_dir_override(Some(user_skills.clone()));
        // SkillStore opens the DB under `base`; parent must exist.
        std::fs::create_dir_all(&base).unwrap();
        IsolatedImportBase {
            _guard: guard,
            _tmp: tmp,
            base,
            user_skills,
        }
    }

    fn project_skill_info(
        path: &Path,
        dir_name: &str,
        content_hash: Option<String>,
    ) -> ProjectSkillInfo {
        ProjectSkillInfo {
            name: dir_name.to_string(),
            dir_name: dir_name.to_string(),
            relative_path: dir_name.to_string(),
            description: None,
            path: path.to_string_lossy().to_string(),
            files: Vec::new(),
            enabled: true,
            agent: "agents".to_string(),
            agent_display_name: "Project".to_string(),
            tags: Vec::new(),
            in_center: false,
            sync_status: "project_only".to_string(),
            center_skill_id: None,
            last_modified_at: None,
            content_hash,
        }
    }

    fn insert_local_skill(store: &SkillStore, central_path: &Path, source_ref: &str) -> SkillRecord {
        let hash = content_hash::hash_directory(central_path).unwrap();
        let skill = SkillRecord {
            id: "existing-1".to_string(),
            name: "demo".to_string(),
            description: Some("original".to_string()),
            source_type: "local".to_string(),
            source_ref: Some(source_ref.to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: central_path.to_string_lossy().to_string(),
            content_hash: Some(hash),
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&skill).unwrap();
        skill
    }

    /// A failed replace must leave the existing User skill byte-identical:
    /// `install_skill_dir_as` validates the source before touching dest.
    #[test]
    fn import_existing_replace_failure_keeps_original_user_skill() {
        let iso = isolated_import_base();
        let store = SkillStore::new(&iso.base.join("test.db")).unwrap();

        // Seed the managed User skill under the skills override root.
        let original = iso.user_skills.join("demo");
        fs::create_dir_all(&original).unwrap();
        fs::write(
            original.join("SKILL.md"),
            "---\nname: demo\n---\noriginal\n",
        )
        .unwrap();

        // Project-side source matches by source_ref but has no SKILL.md.
        let project_src = iso.base.join("project-src").join("demo");
        fs::create_dir_all(&project_src).unwrap();
        // Intentionally no SKILL.md — install must fail before replacing.

        let seed = insert_local_skill(&store, &original, &project_src.to_string_lossy());
        let skill = project_skill_info(&project_src, "demo", None);

        // Match is by source_ref, so the existing row is the replace target.
        assert!(find_best_center_match(&skill, std::slice::from_ref(&seed)).is_some());

        let err = import_project_skill_info(&store, &skill).unwrap_err();
        // `with_repo_lock` flattens AppError → anyhow → AppError::db; assert
        // the install was refused, not the mapped kind.
        assert!(
            err.message.contains("SKILL.md") || matches!(err.kind, ErrorKind::InvalidInput | ErrorKind::Database),
            "unexpected error: {err:?}"
        );

        // Original directory and DB row are untouched.
        let kept = fs::read_to_string(original.join("SKILL.md")).unwrap();
        assert!(kept.contains("original"));
        let after = store.get_skill_by_id(&seed.id).unwrap().unwrap();
        assert_eq!(after.content_hash, seed.content_hash);
        assert_eq!(after.central_path, seed.central_path);
        assert_eq!(after.name, seed.name);
        assert_eq!(store.get_all_skills().unwrap().len(), 1);
    }

    /// New project skill imports only under the user skills root and create
    /// exactly one SkillStore record — never the legacy `base/skills` path.
    #[test]
    fn import_new_project_skill_lands_only_under_user_root() {
        let iso = isolated_import_base();
        let store = SkillStore::new(&iso.base.join("test.db")).unwrap();

        let project_path = iso.base.join("project");
        let skill_dir = project_path.join(".agents").join("skills").join("foo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: foo\n---\nbody\n",
        )
        .unwrap();

        store
            .insert_project(&ProjectRecord {
                id: "p1".to_string(),
                name: "Project".to_string(),
                path: project_path.to_string_lossy().to_string(),
                workspace_type: "project".to_string(),
                linked_agent_key: None,
                linked_agent_name: None,
                disabled_path: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();

        import_project_skill_to_center_blocking(&store, "p1", "foo").unwrap();

        let dest = iso.user_skills.join("foo");
        assert!(
            dest.join("SKILL.md").is_file(),
            "skill must land under the skills-dir override root"
        );
        assert!(
            !iso.base.join("skills").join("foo").exists(),
            "legacy base/skills must not receive the import"
        );

        let skills = store.get_all_skills().unwrap();
        assert_eq!(skills.len(), 1, "exactly one SkillStore record");
        let record = &skills[0];
        assert_eq!(record.name, "foo");
        assert_eq!(
            Path::new(&record.central_path),
            dest.as_path(),
            "central_path must point at the user root copy"
        );
        assert_eq!(record.source_type, "local");
        let source_ref = record.source_ref.as_deref().unwrap_or_default();
        assert_eq!(
            crate::core::paths::identity_key(Path::new(source_ref)),
            crate::core::paths::identity_key(&skill_dir),
            "source_ref must point at the project skill"
        );
        assert!(record.content_hash.is_some());
    }
}
