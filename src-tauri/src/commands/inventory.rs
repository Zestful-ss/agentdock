use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::from_str as json_from_str;
use tauri::State;

use crate::core::canonical::{self, MigrationEntry, SkillDocument, UserSkillRegistration};
use crate::core::content_hash;
use crate::core::error::AppError;
use crate::core::mcp_inventory::{self, McpInventoryRow, SkillInventoryRow};
use crate::core::paths;
use crate::core::skill_metadata;
use crate::core::skill_store::SkillStore;
use crate::core::sync_metadata;

const CUSTOM_READ_ONLY_PATHS_KEY: &str = "custom_read_only_skill_paths";
const RESOURCE_STATES_KEY: &str = "inventory_resource_states";

fn is_same_or_descendant(path: &std::path::Path, root: &std::path::Path) -> bool {
    let path_key = paths::identity_key(path);
    let root_key = paths::identity_key(root);
    if path_key == root_key {
        return true;
    }
    let separator = std::path::MAIN_SEPARATOR;
    let prefix = if root_key.ends_with(separator) {
        root_key
    } else {
        format!("{root_key}{separator}")
    };
    path_key.starts_with(&prefix)
}

fn is_canonical_custom_read_only_path(
    store: &SkillStore,
    path: &std::path::Path,
) -> Result<bool, AppError> {
    let mut canonical_roots = vec![
        crate::core::central_repo::skills_dir_override()
            .unwrap_or_else(paths::user_agents_skills_dir),
    ];
    for project in store.get_all_projects().map_err(AppError::db)? {
        canonical_roots.push(paths::project_agents_skills_dir(std::path::Path::new(
            &project.path,
        )));
    }
    Ok(canonical_roots
        .iter()
        .any(|root| is_same_or_descendant(path, root)))
}

fn validate_custom_read_only_path(
    store: &SkillStore,
    path: &std::path::Path,
) -> Result<(), AppError> {
    if is_canonical_custom_read_only_path(store, path)? {
        return Err(AppError::invalid_input(
            "Custom read-only paths cannot be the canonical user or project skills library",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceState {
    #[serde(default)]
    pub ignored: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Serialize)]
pub struct AdoptDiff {
    pub original: String,
    pub updated: String,
    pub source_path: String,
    pub target_path: String,
}

fn resource_key(kind: &str, identity: &str) -> String {
    if kind == "mcp" {
        // MCP identity includes harness, server name, and config identity.
        // A single settings.json can contain many independent MCP resources.
        format!("mcp:{}", identity)
    } else {
        format!(
            "skill:{}",
            paths::identity_key(std::path::Path::new(identity))
        )
    }
}

fn load_resource_states(store: &SkillStore) -> Result<BTreeMap<String, ResourceState>, AppError> {
    let Some(raw) = store
        .get_setting(RESOURCE_STATES_KEY)
        .map_err(AppError::db)?
    else {
        return Ok(BTreeMap::new());
    };
    serde_json::from_str(&raw).map_err(AppError::internal)
}

fn save_resource_states(
    store: &SkillStore,
    states: &BTreeMap<String, ResourceState>,
) -> Result<(), AppError> {
    let encoded = serde_json::to_string(states).map_err(AppError::internal)?;
    store
        .set_setting(RESOURCE_STATES_KEY, &encoded)
        .map_err(AppError::db)
}

fn decorate_skill_rows(
    rows: &mut [SkillInventoryRow],
    states: &BTreeMap<String, ResourceState>,
) {
    for row in rows {
        if let Some(state) = states.get(&resource_key("skill", &row.path)) {
            row.ignored = state.ignored;
            row.hidden = state.hidden;
            row.note = state.note.clone();
        }
    }
}

fn decorate_mcp_rows(rows: &mut [McpInventoryRow], states: &BTreeMap<String, ResourceState>) {
    for row in rows {
        if let Some(state) = states.get(&resource_key("mcp", &row.id)) {
            row.ignored = state.ignored;
            row.hidden = state.hidden;
            row.note = state.note.clone();
        }
    }
}

/// Update state for one Inventory row. Skills use their normalized path as the
/// identity; MCP rows use the full row id so servers sharing a config remain
/// independent.
pub fn update_inventory_resource_state(
    store: &SkillStore,
    resource_kind: &str,
    identity: &str,
    ignored: Option<bool>,
    hidden: Option<bool>,
    note: Option<String>,
) -> Result<ResourceState, AppError> {
    if resource_kind != "skill" && resource_kind != "mcp" {
        return Err(AppError::invalid_input("Unknown inventory resource kind"));
    }
    let mut states = load_resource_states(store)?;
    let key = resource_key(resource_kind, identity);
    let state = states.entry(key).or_default();
    if let Some(value) = ignored {
        state.ignored = value;
    }
    if let Some(value) = hidden {
        state.hidden = value;
    }
    if let Some(value) = note {
        state.note = value;
    }
    let result = state.clone();
    save_resource_states(store, &states)?;
    Ok(result)
}

pub fn custom_read_only_paths(store: &SkillStore) -> Result<Vec<PathBuf>, AppError> {
    let raw = store
        .get_setting(CUSTOM_READ_ONLY_PATHS_KEY)
        .map_err(AppError::db)?;
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let values: Vec<String> = json_from_str(&raw).map_err(AppError::internal)?;
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = paths::expand_windows_path(trimmed);
        // Drop stale entries written by older builds instead of showing the
        // canonical library twice in Inventory.
        if is_canonical_custom_read_only_path(store, &path)? {
            continue;
        }
        let key = paths::identity_key(&path);
        if seen.insert(key) {
            result.push(path);
        }
    }
    Ok(result)
}

pub fn validated_custom_read_only_paths(
    store: &SkillStore,
    values: Vec<String>,
) -> Result<Vec<String>, AppError> {
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = paths::expand_windows_path(trimmed);
        validate_custom_read_only_path(store, &path)?;
        let key = paths::identity_key(&path);
        if seen.insert(key) {
            result.push(path.to_string_lossy().to_string());
        }
    }
    Ok(result)
}

pub fn normalize_custom_read_only_paths(
    store: &SkillStore,
    values: Vec<String>,
) -> Result<Vec<String>, AppError> {
    let result = validated_custom_read_only_paths(store, values)?;
    let encoded = serde_json::to_string(&result).map_err(AppError::internal)?;
    store
        .set_setting(CUSTOM_READ_ONLY_PATHS_KEY, &encoded)
        .map_err(AppError::db)?;
    Ok(result)
}

pub fn build_skill_inventory(store: &SkillStore) -> Result<Vec<SkillInventoryRow>, AppError> {
    let custom_paths = custom_read_only_paths(store)?;
    let states = load_resource_states(store)?;
    let mut rows = mcp_inventory::discover_skills_with_custom_paths(&custom_paths);
    decorate_skill_rows(&mut rows, &states);
    Ok(rows)
}

pub fn build_mcp_inventory(store: &SkillStore) -> Result<Vec<McpInventoryRow>, AppError> {
    let custom_paths = custom_read_only_paths(store)?;
    let states = load_resource_states(store)?;
    let entries = mcp_inventory::discover_mcp_with_custom_paths(&custom_paths);
    let mut rows = mcp_inventory::inventory_rows(&entries);
    decorate_mcp_rows(&mut rows, &states);
    Ok(rows)
}

#[tauri::command]
pub async fn get_skill_inventory(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillInventoryRow>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || build_skill_inventory(&store))
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// Project skills by project **id** (resolved backend-side via `SkillStore`).
/// The WebView never decides which directory is written to.
#[tauri::command]
pub async fn get_custom_read_only_paths(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok(custom_read_only_paths(&store)?
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect())
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn set_custom_read_only_paths(
    paths: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || normalize_custom_read_only_paths(&store, paths))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn get_project_skill_inventory(
    project_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillInventoryRow>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Read-only: viewing a project must never create its `.agents/skills`.
        let (project_path, root) = canonical::resolve_project_root_for_read(&store, &project_id)?;
        let states = load_resource_states(&store)?;
        let mut rows = mcp_inventory::discover_project_skills_at(
            &root,
            std::path::Path::new(&project_path),
        );
        decorate_skill_rows(&mut rows, &states);
        Ok(rows)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn get_mcp_inventory(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<McpInventoryRow>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || build_mcp_inventory(&store))
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn set_inventory_resource_state(
    resource_kind: String,
    identity: String,
    ignored: Option<bool>,
    hidden: Option<bool>,
    note: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ResourceState, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        update_inventory_resource_state(
            &store,
            &resource_kind,
            &identity,
            ignored,
            hidden,
            note,
        )
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

fn read_skill_markdown(path: &std::path::Path) -> Result<String, AppError> {
    for filename in ["SKILL.md", "skill.md"] {
        let candidate = path.join(filename);
        if candidate.is_file() {
            return std::fs::read_to_string(&candidate).map_err(AppError::io);
        }
    }
    Err(AppError::not_found(format!(
        "No SKILL.md found in {}",
        path.display()
    )))
}

#[tauri::command]
pub async fn get_adopt_diff(
    source_path: String,
    skill_name: String,
    scope: String,
    project_id: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<AdoptDiff, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        if !source.is_dir() {
            return Err(AppError::not_found("Adopt source directory not found"));
        }
        let source_content = read_skill_markdown(&source)?;
        let resolved = match scope.as_str() {
            "user" => canonical::resolve_user_root()?,
            "project" => {
                let project_id = project_id.ok_or_else(|| {
                    AppError::invalid_input("Project id is required for project Adopt")
                })?;
                canonical::resolve_project_root(&store, &project_id)?.1
            }
            _ => return Err(AppError::invalid_input("Unknown Adopt scope")),
        };
        let target = canonical::skill_dir(&resolved, &skill_name)?;
        let original = if target.join("SKILL.md").is_file() {
            std::fs::read_to_string(target.join("SKILL.md")).map_err(AppError::io)?
        } else if target.join("skill.md").is_file() {
            std::fs::read_to_string(target.join("skill.md")).map_err(AppError::io)?
        } else {
            String::new()
        };
        Ok(AdoptDiff {
            original,
            updated: source_content,
            source_path: source.display().to_string(),
            target_path: target.display().to_string(),
        })
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// Adopt a discovered skill into User `~/.agents/skills`.
/// Existing name + `replace != true` → `target_conflict` (frontend: Replace/Cancel).
///
/// User scope reconciles the `SkillStore` under the repo lock, so MySkills and
/// the update system see exactly what the filesystem holds.
#[tauri::command]
pub async fn adopt_skill_to_user(
    source_path: String,
    replace: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        // The whole User operation runs under the repo lock
        // (RepoLock → fs → DB → metadata), so a concurrent git
        // update/reimport can neither interleave nor leave
        // "files changed, DB not changed" behind.
        sync_metadata::with_repo_lock("adopt skill", || {
            let resolved = canonical::resolve_user_root()?;
            if replace.unwrap_or(false) {
                let name = skill_metadata::infer_skill_name(&source);
                let candidate = resolved.root.join(canonical::sanitize_component(&name)?);
                let existing = store
                    .get_all_skills()?
                    .into_iter()
                    .find(|record| {
                        paths::same_path(
                            std::path::Path::new(&record.central_path),
                            &candidate,
                        )
                    });
                if let Some(record) = existing {
                    let Some(baseline) = record.content_hash.as_deref() else {
                        return Err(anyhow::Error::from(AppError::invalid_input(
                            "Managed skill has no recorded baseline; replacement was not applied",
                        )));
                    };
                    let live = content_hash::hash_directory_strict(&candidate)
                        .map_err(AppError::io)?;
                    if live != baseline {
                        return Err(anyhow::Error::from(AppError::invalid_input(
                            "Managed skill was modified locally; replacement was not applied",
                        )));
                    }
                }
            }
            let dest =
                canonical::install_skill_dir(&source, &resolved, replace.unwrap_or(false))?;
            let hash = content_hash::hash_directory(&dest).map_err(AppError::io)?;
            let meta = skill_metadata::parse_skill_md(&dest);
            let name = dest
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "skill".to_string());
            canonical::register_user_skill(
                &store,
                &UserSkillRegistration {
                    name,
                    description: meta.description,
                    central_path: dest.clone(),
                    content_hash: hash,
                    source_type: "adopted".to_string(),
                    source_ref: Some(source.display().to_string()),
                    ..Default::default()
                },
            )
            .map_err(anyhow::Error::from)?;
            Ok(dest.display().to_string())
        })
        .map_err(AppError::db)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// Adopt a discovered skill into `<project>/.agents/skills` by project id.
#[tauri::command]
pub async fn adopt_skill_to_project(
    source_path: String,
    project_id: String,
    replace: Option<bool>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        let (_, resolved) = canonical::resolve_project_root(&store, &project_id)?;
        let dest = canonical::install_skill_dir(&source, &resolved, replace.unwrap_or(false))?;
        Ok(dest.display().to_string())
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// Delete a managed skill by name. `scope` is `"user"` or `"project"`.
#[tauri::command]
pub async fn delete_canonical_skill(
    skill_name: String,
    scope: String,
    project_id: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || match scope.as_str() {
        "project" => {
            // Project scope is pure filesystem; it intentionally keeps no
            // SkillStore records.
            let project_id =
                project_id.ok_or_else(|| AppError::invalid_input("project_id is required"))?;
            let (_, resolved) = canonical::resolve_project_root(&store, &project_id)?;
            let removed = canonical::delete_skill(&resolved, &skill_name)?;
            Ok(removed.display().to_string())
        }
        "user" => {
            // Whole operation under the repo lock (see adopt above).
            sync_metadata::with_repo_lock("delete canonical skill", || {
                let resolved = canonical::resolve_user_root()?;
                let removed = canonical::delete_skill(&resolved, &skill_name)?;
                // Reconcile the index: drop records (and their stale
                // deploy-target rows) for the deleted directory. Harness
                // files are never touched.
                canonical::remove_user_skill_records(&store, &removed)
                    .map_err(anyhow::Error::from)?;
                Ok(removed.display().to_string())
            })
            .map_err(AppError::db)
        }
        _ => Err(AppError::invalid_input("scope must be 'user' or 'project'")),
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// Read a managed `SKILL.md` by skill name.
#[tauri::command]
pub async fn read_canonical_skill_document(
    skill_name: String,
    scope: String,
    project_id: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SkillDocument, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || match scope.as_str() {
        "project" => {
            let project_id =
                project_id.ok_or_else(|| AppError::invalid_input("project_id is required"))?;
            let (_, resolved) = canonical::resolve_project_root(&store, &project_id)?;
            canonical::read_skill_document(&resolved, &skill_name)
        }
        "user" => {
            let resolved = canonical::resolve_user_root()?;
            canonical::read_skill_document(&resolved, &skill_name)
        }
        _ => Err(AppError::invalid_input("scope must be 'user' or 'project'")),
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// Save a managed `SKILL.md` by skill name (validated, atomic write).
#[tauri::command]
pub async fn save_canonical_skill_document(
    skill_name: String,
    content: String,
    scope: String,
    project_id: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || match scope.as_str() {
        "project" => {
            let project_id =
                project_id.ok_or_else(|| AppError::invalid_input("project_id is required"))?;
            let (_, resolved) = canonical::resolve_project_root(&store, &project_id)?;
            let saved = canonical::save_skill_document(&resolved, &skill_name, &content)?;
            Ok(saved.display().to_string())
        }
        "user" => {
            // Whole operation under the repo lock (see adopt above).
            sync_metadata::with_repo_lock("save canonical skill", || {
                let resolved = canonical::resolve_user_root()?;
                let saved = canonical::save_skill_document(&resolved, &skill_name, &content)?;
                // Reconcile the index so `content_hash/updated_at` stop being stale.
                let skill_dir = saved.parent().map(|p| p.to_path_buf()).ok_or_else(|| {
                    anyhow::anyhow!("Saved skill has no parent directory")
                })?;
                let hash = content_hash::hash_directory(&skill_dir).map_err(AppError::io)?;
                let identity = crate::core::paths::identity_key(&skill_dir);
                for record in store.get_all_skills()? {
                    if crate::core::paths::identity_key(std::path::Path::new(
                        &record.central_path,
                    )) == identity
                    {
                        store.update_skill_central_path(
                            &record.id,
                            &skill_dir.display().to_string(),
                            Some(&hash),
                        )?;
                    }
                }
                sync_metadata::write_all_from_db_unlocked(&store)?;
                Ok(saved.display().to_string())
            })
            .map_err(AppError::db)
        }
        _ => Err(AppError::invalid_input("scope must be 'user' or 'project'")),
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// One-shot V1 migration: legacy `~/.skills-manager/skills` → canonical user
/// root. Conflict-safe; never deletes the legacy directory.
///
/// Runs under the repo lock; `Adopted` rows (no prior record) get fresh
/// `SkillStore` records so MySkills sees the same truth as the filesystem.
#[tauri::command]
pub async fn run_legacy_migration(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<MigrationEntry>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync_metadata::with_repo_lock("migrate legacy library", || {
            let report = canonical::migrate_legacy_library(&store)?;
            for entry in &report {
                if entry.outcome != canonical::MigrationOutcome::Adopted {
                    continue;
                }
                let hash = entry.content_hash.clone().ok_or_else(|| {
                    anyhow::anyhow!("Adopted skill {} has no content hash", entry.name)
                })?;
                let dest = PathBuf::from(&entry.canonical_path);
                let meta = skill_metadata::parse_skill_md(&dest);
                canonical::register_user_skill(
                    &store,
                    &UserSkillRegistration {
                        name: entry.name.clone(),
                        description: meta.description,
                        central_path: dest,
                        content_hash: hash,
                        source_type: "migrated".to_string(),
                        source_ref: Some(entry.legacy_path.clone()),
                        ..Default::default()
                    },
                )?;
            }
            sync_metadata::write_all_from_db_unlocked(&store)?;
            Ok(report)
        })
        .map_err(AppError::db)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub fn get_canonical_roots() -> Result<CanonicalRoots, AppError> {
    Ok(CanonicalRoots {
        user_skills: crate::core::paths::user_agents_skills_dir()
            .display()
            .to_string(),
        native_consumers: crate::core::v1::native_consumers()
            .iter()
            .map(|s| s.to_string())
            .collect(),
    })
}

#[derive(serde::Serialize)]
pub struct CanonicalRoots {
    pub user_skills: String,
    pub native_consumers: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::ErrorKind;
    use crate::core::mcp_inventory::{McpHarnessStatus, McpTransport};
    use crate::core::skill_store::ProjectRecord;
    use tempfile::tempdir;

    fn mcp_row(id: &str, name: &str) -> McpInventoryRow {
        McpInventoryRow {
            id: id.to_string(),
            name: name.to_string(),
            sources: vec![McpHarnessStatus {
                harness: "opencode".to_string(),
                display_name: "OpenCode".to_string(),
                transport: McpTransport::Stdio,
                source_enabled: Some(true),
                source_path: "C:/Users/test/AppData/Roaming/opencode/settings.json".to_string(),
                configured: true,
            }],
            ignored: false,
            hidden: false,
            note: String::new(),
        }
    }

    #[test]
    fn canonical_path_matching_includes_descendants_but_not_siblings() {
        let root = std::path::Path::new("/tmp/agentdock-skills");
        let child = root.join("one");
        let sibling = std::path::Path::new("/tmp/agentdock-other");
        assert!(is_same_or_descendant(child.as_path(), root));
        assert!(is_same_or_descendant(root, root));
        assert!(!is_same_or_descendant(sibling, root));
    }

    #[test]
    fn custom_read_only_paths_reject_a_project_canonical_library() {
        let tmp = tempdir().unwrap();
        let project_root = tmp.path().join("repo");
        let canonical = project_root.join(".agents").join("skills").join("one");
        std::fs::create_dir_all(&canonical).unwrap();
        let store = SkillStore::new(&tmp.path().join("inventory.db")).unwrap();
        store
            .insert_project(&ProjectRecord {
                id: "project-1".to_string(),
                name: "repo".to_string(),
                path: project_root.display().to_string(),
                workspace_type: "project".to_string(),
                linked_agent_key: None,
                linked_agent_name: None,
                disabled_path: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();

        let error = validated_custom_read_only_paths(
            &store,
            vec![canonical.display().to_string()],
        )
        .unwrap_err();
        assert!(matches!(error.kind, ErrorKind::InvalidInput));
    }

    #[test]
    fn mcp_state_uses_resource_id_not_config_path() {
        let mut rows = vec![
            mcp_row("opencode:exa:settings", "exa"),
            mcp_row("opencode:github:settings", "github"),
        ];
        let mut states = BTreeMap::new();
        states.insert(
            resource_key("mcp", &rows[0].id),
            ResourceState {
                ignored: true,
                ..ResourceState::default()
            },
        );

        decorate_mcp_rows(&mut rows, &states);

        assert!(rows[0].ignored);
        assert!(!rows[1].ignored);
    }
}
