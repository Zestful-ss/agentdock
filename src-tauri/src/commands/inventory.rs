use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::core::canonical::{self, MigrationEntry, SkillDocument};
use crate::core::error::AppError;
use crate::core::mcp_inventory::{self, McpInventoryRow, SkillInventoryRow};
use crate::core::skill_store::SkillStore;

#[tauri::command]
pub async fn get_skill_inventory() -> Result<Vec<SkillInventoryRow>, AppError> {
    tauri::async_runtime::spawn_blocking(mcp_inventory::discover_skills)
        .await
        .map_err(|e| AppError::internal(e.to_string()))
}

/// Project skills by project **id** (resolved backend-side via `SkillStore`).
/// The WebView never decides which directory is written to.
#[tauri::command]
pub async fn get_project_skill_inventory(
    project_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillInventoryRow>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_, resolved) = canonical::resolve_project_root(&store, &project_id)?;
        Ok(mcp_inventory::discover_project_skills_at(&resolved.root))
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn get_mcp_inventory() -> Result<Vec<McpInventoryRow>, AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        let entries = mcp_inventory::discover_mcp();
        mcp_inventory::inventory_rows(&entries)
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))
}

/// Adopt a discovered skill into User `~/.agents/skills`.
/// Existing name + `replace != true` → `target_conflict` (frontend: Replace/Cancel).
#[tauri::command]
pub async fn adopt_skill_to_user(
    source_path: String,
    replace: Option<bool>,
) -> Result<String, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        let resolved = canonical::resolve_user_root()?;
        let dest = canonical::install_skill_dir(&source, &resolved, replace.unwrap_or(false))?;
        Ok(dest.display().to_string())
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
            let project_id =
                project_id.ok_or_else(|| AppError::invalid_input("project_id is required"))?;
            let (_, resolved) = canonical::resolve_project_root(&store, &project_id)?;
            let removed = canonical::delete_skill(&resolved, &skill_name)?;
            Ok(removed.display().to_string())
        }
        "user" => {
            let resolved = canonical::resolve_user_root()?;
            let removed = canonical::delete_skill(&resolved, &skill_name)?;
            Ok(removed.display().to_string())
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
            let resolved = canonical::resolve_user_root()?;
            let saved = canonical::save_skill_document(&resolved, &skill_name, &content)?;
            Ok(saved.display().to_string())
        }
        _ => Err(AppError::invalid_input("scope must be 'user' or 'project'")),
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

/// One-shot V1 migration: legacy `~/.skills-manager/skills` → canonical user
/// root. Conflict-safe; never deletes the legacy directory.
#[tauri::command]
pub async fn run_legacy_migration(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<MigrationEntry>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || canonical::migrate_legacy_library(&store))
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
