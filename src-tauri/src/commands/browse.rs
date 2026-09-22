use std::sync::Arc;
use tauri::State;

use crate::core::{error::AppError, skill_store::SkillStore, skillssh_api::SkillsShSkill};

#[tauri::command]
pub async fn fetch_leaderboard(
    _board: String,
    _store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillsShSkill>, AppError> {
    Err(crate::core::v1::blocked_write())
}

#[tauri::command]
pub async fn search_skillssh(
    _query: String,
    _limit: Option<usize>,
    _store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillsShSkill>, AppError> {
    Err(crate::core::v1::blocked_write())
}
