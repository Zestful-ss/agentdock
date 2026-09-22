use std::path::PathBuf;

use crate::core::error::AppError;
use crate::core::installer;
use crate::core::mcp_inventory::{self, McpInventoryRow, SkillInventoryRow};
use crate::core::paths;
use crate::core::v1;

#[tauri::command]
pub async fn get_skill_inventory() -> Result<Vec<SkillInventoryRow>, AppError> {
    tauri::async_runtime::spawn_blocking(mcp_inventory::discover_skills)
        .await
        .map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command]
pub async fn get_project_skill_inventory(
    project_path: String,
) -> Result<Vec<SkillInventoryRow>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        mcp_inventory::discover_project_skills(std::path::Path::new(&project_path))
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))
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

#[tauri::command]
pub async fn adopt_skill_to_user(source_path: String) -> Result<String, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        if !source.is_dir() {
            return Err(AppError::not_found("Skill directory not found"));
        }
        let dest_root = paths::user_agents_skills_dir();
        std::fs::create_dir_all(&dest_root).map_err(AppError::io)?;
        let name = crate::core::skill_metadata::infer_skill_name(&source);
        let dest = dest_root.join(&name);
        installer::install_skill_dir_to_destination(&source, &name, &dest).map_err(AppError::io)?;
        Ok(dest.display().to_string())
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn adopt_skill_to_project(
    source_path: String,
    project_path: String,
) -> Result<String, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let source = PathBuf::from(&source_path);
        if !source.is_dir() {
            return Err(AppError::not_found("Skill directory not found"));
        }
        let dest_root = paths::project_agents_skills_dir(std::path::Path::new(&project_path));
        if !v1::is_canonical_agents_skills_path(&dest_root) {
            return Err(v1::blocked_write());
        }
        std::fs::create_dir_all(&dest_root).map_err(AppError::io)?;
        let name = crate::core::skill_metadata::infer_skill_name(&source);
        let dest = dest_root.join(&name);
        installer::install_skill_dir_to_destination(&source, &name, &dest).map_err(AppError::io)?;
        Ok(dest.display().to_string())
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub fn get_canonical_roots() -> Result<CanonicalRoots, AppError> {
    Ok(CanonicalRoots {
        user_skills: paths::user_agents_skills_dir().display().to_string(),
        native_consumers: v1::native_consumers()
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
