use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tauri::State;

use crate::core::{
    error::AppError,
    scenario_service,
    skill_store::{ScenarioRecord, SkillStore},
    sync_metadata,
    timing::should_log_first_or_slow,
};

#[derive(Debug, Serialize)]
pub struct PresetDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub skill_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

static GET_PRESETS_FIRST_CALL: AtomicBool = AtomicBool::new(true);

fn preset_dto(store: &SkillStore, scenario: ScenarioRecord) -> PresetDto {
    let skill_count = store.count_skills_for_scenario(&scenario.id).unwrap_or(0);
    PresetDto {
        id: scenario.id,
        name: scenario.name,
        description: scenario.description,
        icon: scenario.icon,
        sort_order: scenario.sort_order,
        skill_count,
        created_at: scenario.created_at,
        updated_at: scenario.updated_at,
    }
}

#[tauri::command]
pub async fn get_presets(store: State<'_, Arc<SkillStore>>) -> Result<Vec<PresetDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
        let count = scenarios.len();
        let mut result = Vec::new();
        for s in scenarios {
            result.push(preset_dto(&store, s));
        }
        let elapsed_ms = start.elapsed().as_millis();
        if should_log_first_or_slow(&GET_PRESETS_FIRST_CALL, elapsed_ms, 100) {
            log::info!("get_presets: {count} presets in {elapsed_ms} ms");
        }
        Ok(result)
    })
    .await?
}

#[tauri::command]
pub async fn get_active_preset(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<PresetDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let active_id = store.get_active_scenario_id().map_err(AppError::db)?;

        if let Some(id) = active_id {
            let scenarios = store.get_all_scenarios().map_err(AppError::db)?;
            if let Some(s) = scenarios.into_iter().find(|s| s.id == id) {
                return Ok(Some(preset_dto(&store, s)));
            }
        }
        Ok(None)
    })
    .await?
}

#[tauri::command]
pub async fn create_preset(
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<PresetDto, AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        create_and_activate_preset_internal(&store, &name, description.as_deref(), icon.as_deref())
            .map(|scenario| preset_dto(&store, scenario))
    })
    .await?;
    result
}

pub fn create_preset_internal(
    store: &SkillStore,
    name: &str,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<ScenarioRecord, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::invalid_input("Preset name cannot be empty"));
    }

    let now = chrono::Utc::now().timestamp_millis();
    let id = uuid::Uuid::new_v4().to_string();
    let record = ScenarioRecord {
        id: id.clone(),
        name: name.to_string(),
        description: description
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        icon: icon
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        sort_order: 999,
        created_at: now,
        updated_at: now,
    };

    sync_metadata::with_repo_lock("create preset", || {
        store.insert_scenario(&record)?;
        sync_metadata::write_all_from_db_unlocked(store)
    })
    .map_err(AppError::db)?;
    Ok(record)
}

/// Create a preset and make it the active *view* without touching any harness
/// files. Presets are organization metadata only in V1.
fn create_and_activate_preset_internal(
    store: &SkillStore,
    name: &str,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<ScenarioRecord, AppError> {
    let record = create_preset_internal(store, name, description, icon)?;
    store
        .set_active_scenario(&record.id)
        .map_err(AppError::db)?;
    Ok(record)
}

#[tauri::command]
pub async fn update_preset(
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        update_preset_internal(&store, &id, &name, description.as_deref(), icon.as_deref())
    })
    .await?;
    result
}

pub fn update_preset_internal(
    store: &SkillStore,
    id: &str,
    name: &str,
    description: Option<&str>,
    icon: Option<&str>,
) -> Result<(), AppError> {
    scenario_service::ensure_scenario_exists(store, id)?;
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::invalid_input("Preset name cannot be empty"));
    }
    let description = description.map(str::trim).filter(|value| !value.is_empty());
    let icon = icon.map(str::trim).filter(|value| !value.is_empty());
    sync_metadata::with_repo_lock("update preset", || {
        store.update_scenario(id, name, description, icon)?;
        sync_metadata::write_all_from_db_unlocked(store)
    })
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn delete_preset(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        delete_preset_internal(&store, &id)
    })
    .await?;
    result
}

pub fn delete_preset_internal(store: &SkillStore, id: &str) -> Result<(), AppError> {
    scenario_service::ensure_scenario_exists(store, id)?;
    let was_active = store
        .get_active_scenario_id()
        .map_err(AppError::db)?
        .as_deref()
        == Some(id);

    sync_metadata::with_repo_lock("delete preset", || {
        if was_active {
            store.clear_active_scenario()?;
        }
        store.delete_scenario(id)?;
        sync_metadata::write_all_from_db_unlocked(store)
    })
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn add_skill_to_preset(
    skill_id: String,
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        set_preset_skills_internal(&store, &preset_id, &[skill_id], true)?;
        // Membership-only edit. We intentionally do NOT sync to disk here,
        // even when this preset happens to be the legacy `active_scenario_id`,
        // because presets are curation labels, not implicit deployment
        // switches. Users deploy explicitly via the CLI / per-skill commands.
        Ok(())
    })
    .await?;
    result
}

#[tauri::command]
pub async fn remove_skill_from_preset(
    skill_id: String,
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        set_preset_skills_internal(&store, &preset_id, &[skill_id], false)?;
        // Same rationale as add_skill_to_preset: editing preset membership
        // never wipes on-disk skill targets. To remove a skill from a coding
        // agent the caller uses the CLI / the explicit per-skill unsync path.
        Ok(())
    })
    .await?;
    result
}

/// Add or remove a pre-resolved set of skills from one preset under the repo
/// lock. Membership edits are curation-only and deliberately do not deploy.
pub fn set_preset_skills_internal(
    store: &SkillStore,
    preset_id: &str,
    skill_ids: &[String],
    add: bool,
) -> Result<(), AppError> {
    scenario_service::ensure_scenario_exists(store, preset_id)?;
    sync_metadata::with_repo_lock(
        if add {
            "add skills to preset"
        } else {
            "remove skills from preset"
        },
        || {
            for skill_id in skill_ids {
                if store.get_skill_by_id(skill_id)?.is_none() {
                    return Err(anyhow::anyhow!("Skill not found: {skill_id}"));
                }
                if add {
                    store.add_skill_to_scenario(preset_id, skill_id)?;
                } else {
                    store.remove_skill_from_scenario(preset_id, skill_id)?;
                }
            }
            sync_metadata::write_all_from_db_unlocked(store)
        },
    )
    .map_err(AppError::db)
}

#[tauri::command]
pub async fn reorder_presets(
    ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        sync_metadata::with_repo_lock("reorder scenarios", || {
            store.reorder_scenarios(&ids)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)
    })
    .await?;
    result
}

#[tauri::command]
pub async fn get_preset_skill_order(
    preset_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_skill_ids_for_scenario(&preset_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn reorder_preset_skills(
    preset_id: String,
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync_metadata::with_repo_lock("reorder scenario skills", || {
            store.reorder_scenario_skills(&preset_id, &skill_ids)?;
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)
    })
    .await?
}

// ── Internal helpers ──

#[cfg(test)]
pub(crate) fn sync_scenario_skills(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<Vec<scenario_service::TargetConflict>, AppError> {
    scenario_service::sync_scenario_skills(store, scenario_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::core::scenario_service::{
        collect_scenario_sync_targets, sync_desired_targets, unsync_obsolete_scenario_targets,
    };
    use crate::core::skill_store::SkillRecord;
    use crate::core::tool_adapters::{self, CustomToolDef};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;
    use std::sync::MutexGuard;
    use tempfile::tempdir;
    use tempfile::TempDir;

    struct MetadataTestRepo {
        _lock: MutexGuard<'static, ()>,
        _tmp: TempDir,
        store: SkillStore,
    }

    impl Drop for MetadataTestRepo {
        fn drop(&mut self) {
            crate::core::central_repo::set_test_base_dir_override(None);
        }
    }

    fn metadata_test_repo() -> MetadataTestRepo {
        let lock = crate::core::central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        let base = tmp.path().join("repo");
        crate::core::central_repo::set_test_base_dir_override(Some(base.clone()));
        fs::create_dir_all(crate::core::central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();
        MetadataTestRepo {
            _lock: lock,
            _tmp: tmp,
            store,
        }
    }

    fn sample_skill(id: &str, name: &str, central_path: &std::path::Path) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: Some(central_path.to_string_lossy().to_string()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: central_path.to_string_lossy().to_string(),
            content_hash: None,
            enabled: true,
            created_at: 1,
            updated_at: 1,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    fn sample_scenario(id: &str, name: &str) -> ScenarioRecord {
        ScenarioRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            icon: None,
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn write_skill_dir(base: &std::path::Path, name: &str) -> PathBuf {
        let dir = base.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
        dir
    }

    fn configure_single_custom_tool(store: &SkillStore, target_base: &std::path::Path) {
        let custom_tools = vec![CustomToolDef {
            key: "test_agent".to_string(),
            display_name: "Test Agent".to_string(),
            skills_dir: target_base.to_string_lossy().to_string(),
            project_relative_skills_dir: None,
            category: Default::default(),
        }];
        store
            .set_setting(
                "custom_tools",
                &serde_json::to_string(&custom_tools).unwrap(),
            )
            .unwrap();
        let disabled_builtin_tools: Vec<String> = tool_adapters::default_tool_adapters()
            .into_iter()
            .map(|adapter| adapter.key)
            .collect();
        store
            .set_setting(
                "disabled_tools",
                &serde_json::to_string(&disabled_builtin_tools).unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn organization_crud_does_not_switch_or_deploy_presets() {
        let repo = metadata_test_repo();
        repo.store
            .insert_scenario(&sample_scenario("current", "Current"))
            .unwrap();
        repo.store.set_active_scenario("current").unwrap();

        let created =
            create_preset_internal(&repo.store, "  Web Dev  ", Some(" Frontend work "), None)
                .unwrap();
        assert_eq!(created.name, "Web Dev");
        assert_eq!(created.description.as_deref(), Some("Frontend work"));
        assert_eq!(
            repo.store.get_active_scenario_id().unwrap().as_deref(),
            Some("current")
        );
        assert!(repo.store.get_all_targets().unwrap().is_empty());

        delete_preset_internal(&repo.store, &created.id).unwrap();
        assert_eq!(
            repo.store.get_active_scenario_id().unwrap().as_deref(),
            Some("current")
        );

        delete_preset_internal(&repo.store, "current").unwrap();
        assert_eq!(repo.store.get_active_scenario_id().unwrap(), None);
        assert!(repo.store.get_all_targets().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn switching_scenarios_keeps_overlapping_skill_target() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&target_base).unwrap();

        configure_single_custom_tool(&store, &target_base);

        store
            .insert_scenario(&sample_scenario("old", "Old"))
            .unwrap();
        store
            .insert_scenario(&sample_scenario("new", "New"))
            .unwrap();

        let shared_dir = write_skill_dir(&source_base, "shared");
        let old_only_dir = write_skill_dir(&source_base, "old-only");
        let new_only_dir = write_skill_dir(&source_base, "new-only");
        store
            .insert_skill(&sample_skill("shared", "shared", &shared_dir))
            .unwrap();
        store
            .insert_skill(&sample_skill("old-only", "old-only", &old_only_dir))
            .unwrap();
        store
            .insert_skill(&sample_skill("new-only", "new-only", &new_only_dir))
            .unwrap();

        store.add_skill_to_scenario("old", "shared").unwrap();
        store.add_skill_to_scenario("old", "old-only").unwrap();
        store.add_skill_to_scenario("new", "shared").unwrap();
        store.add_skill_to_scenario("new", "new-only").unwrap();

        store.set_active_scenario("old").unwrap();
        sync_scenario_skills(&store, "old").unwrap();

        let shared_target = target_base.join("shared");
        let old_only_target = target_base.join("old-only");
        let new_only_target = target_base.join("new-only");
        assert_eq!(fs::read_link(&shared_target).unwrap(), shared_dir);
        assert!(old_only_target.is_symlink());
        let shared_inode_before = fs::symlink_metadata(&shared_target).unwrap().ino();

        let desired_targets = collect_scenario_sync_targets(&store, "new").unwrap();
        unsync_obsolete_scenario_targets(&store, "old", &desired_targets).unwrap();
        store.set_active_scenario("new").unwrap();
        sync_desired_targets(&store, &desired_targets).unwrap();

        assert_eq!(fs::read_link(&shared_target).unwrap(), shared_dir);
        assert_eq!(
            fs::symlink_metadata(&shared_target).unwrap().ino(),
            shared_inode_before
        );
        assert!(!old_only_target.exists());
        assert_eq!(fs::read_link(&new_only_target).unwrap(), new_only_dir);

        let targets = store.get_all_targets().unwrap();
        assert_eq!(targets.len(), 2);
        assert!(targets
            .iter()
            .any(|target| target.skill_id == "shared" && target.tool == "test_agent"));
        assert!(targets
            .iter()
            .any(|target| target.skill_id == "new-only" && target.tool == "test_agent"));
    }

    #[test]
    fn scenario_sync_keeps_duplicate_skill_names_separate() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let source_base = tmp.path().join("central");
        let target_base = tmp.path().join("agent-skills");
        fs::create_dir_all(&source_base).unwrap();
        fs::create_dir_all(&target_base).unwrap();
        configure_single_custom_tool(&store, &target_base);
        store.set_setting("sync_mode", "copy").unwrap();

        store
            .insert_scenario(&sample_scenario("active", "Active"))
            .unwrap();

        let first_dir = write_skill_dir(&source_base, "skill123");
        let second_dir = write_skill_dir(&source_base, "skill123-2");
        fs::write(first_dir.join("unique.txt"), "first").unwrap();
        fs::write(second_dir.join("unique.txt"), "second").unwrap();

        store
            .insert_skill(&sample_skill("first", "skill123", &first_dir))
            .unwrap();
        store
            .insert_skill(&sample_skill("second", "skill123", &second_dir))
            .unwrap();
        store.add_skill_to_scenario("active", "first").unwrap();
        store.add_skill_to_scenario("active", "second").unwrap();

        sync_scenario_skills(&store, "active").unwrap();

        assert_eq!(
            fs::read_to_string(target_base.join("skill123/unique.txt")).unwrap(),
            "first"
        );
        assert_eq!(
            fs::read_to_string(target_base.join("skill123-2/unique.txt")).unwrap(),
            "second"
        );
        let targets = store.get_all_targets().unwrap();
        assert!(targets.iter().any(|target| {
            target.skill_id == "first" && target.target_path.ends_with("skill123")
        }));
        assert!(targets.iter().any(|target| {
            target.skill_id == "second" && target.target_path.ends_with("skill123-2")
        }));
    }
}
