use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tauri::State;

use crate::core::error::AppError;
use crate::core::skill_store::SkillStore;
use crate::core::timing::should_log_first_or_slow;
use crate::core::tool_adapters::{self, CustomToolDef, ToolCategory};
use crate::core::tool_service::{
    self, get_custom_tool_paths, get_custom_tool_project_paths, get_custom_tools,
    get_disabled_tools, get_tool_order, normalize_project_relative_skills_dir_input,
    normalize_skills_dir_input, set_custom_tool_paths, set_custom_tool_project_paths,
    set_custom_tools, set_disabled_tools, set_tool_order, ToolInfo,
};

#[derive(Debug, Serialize)]
pub struct ToolInfoDto {
    pub key: String,
    pub display_name: String,
    pub installed: bool,
    pub skills_dir: String,
    pub enabled: bool,
    pub is_custom: bool,
    pub has_path_override: bool,
    pub project_relative_skills_dir: Option<String>,
    pub has_project_path_override: bool,
    pub category: ToolCategory,
}

static GET_TOOL_STATUS_FIRST_CALL: AtomicBool = AtomicBool::new(true);

#[tauri::command]
pub async fn get_tool_status(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<ToolInfoDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let infos = tool_service::list_tool_info(&store);
        let count = infos.len();
        let result: Vec<ToolInfoDto> = infos
            .into_iter()
            .map(|info: ToolInfo| ToolInfoDto {
                key: info.key,
                display_name: info.display_name,
                installed: info.installed,
                skills_dir: info.skills_dir,
                enabled: info.enabled,
                is_custom: info.is_custom,
                has_path_override: info.has_path_override,
                project_relative_skills_dir: info.project_relative_skills_dir,
                has_project_path_override: info.has_project_path_override,
                category: info.category,
            })
            .collect();
        let elapsed_ms = start.elapsed().as_millis();
        if should_log_first_or_slow(&GET_TOOL_STATUS_FIRST_CALL, elapsed_ms, 100) {
            log::info!("get_tool_status: {count} tools in {elapsed_ms} ms");
        }
        Ok(result)
    })
    .await?
}

#[tauri::command]
pub async fn set_tool_enabled(
    key: String,
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        set_tool_enabled_internal(&store, &key, enabled)
    })
    .await?;
    result
}

/// Shared GUI/CLI implementation for the discovery toggle.
///
/// In V1 a tool toggle only controls whether the adapter participates in
/// read-only inventory. It must never synchronize, copy, or remove files in a
/// harness directory.
pub fn set_tool_enabled_internal(
    store: &SkillStore,
    key: &str,
    enabled: bool,
) -> Result<(), AppError> {
    if tool_adapters::find_adapter_with_store(store, key).is_none() {
        return Err(AppError::not_found(format!("Unknown agent: {key}")));
    }

    let mut disabled = get_disabled_tools(store);
    if enabled {
        disabled.retain(|item| item != key);
    } else if !disabled.iter().any(|item| item == key) {
        disabled.push(key.to_string());
    }
    set_disabled_tools(store, &disabled)
}

#[tauri::command]
pub async fn set_all_tools_enabled(
    enabled: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            set_disabled_tools(&store, &[])
        } else {
            let adapters = tool_adapters::all_tool_adapters(&store);
            let all_keys: Vec<String> = adapters.iter().map(|a| a.key.clone()).collect();
            set_disabled_tools(&store, &all_keys)
        }
    })
    .await?;
    result
}

#[tauri::command]
pub async fn get_tool_order_cmd(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(get_tool_order(&store))).await?
}

#[tauri::command]
pub async fn set_tool_order_cmd(
    order: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_tool_order(&store, &order)).await?
}

/// Whether a built-in adapter claims this key. Resolution consults built-ins
/// before custom tools, so a key that answers `true` here must have its
/// overrides written to the side maps, never onto a custom tool definition.
fn is_builtin_key(key: &str) -> bool {
    tool_adapters::default_tool_adapters()
        .iter()
        .any(|a| a.key == key)
}

/// Store side of [`set_custom_tool_path`], separated so the write can be
/// tested against a real store without a Tauri runtime.
pub(crate) fn apply_tool_skills_dir(
    store: &SkillStore,
    key: &str,
    path: &str,
) -> Result<(), AppError> {
    let key = key.trim().to_string();
    let path = normalize_skills_dir_input(path)?;
    if key.is_empty() || path.is_empty() {
        return Err(AppError::invalid_input("Key and path are required"));
    }

    // Resolution prefers a built-in over a custom tool of the same key
    // (`find_adapter_with_store`), so the write has to agree. A custom agent
    // whose key later became a built-in one otherwise stores the new path on a
    // definition nothing reads, and the save reports success while the
    // displayed path never moves (#378).
    let mut customs = get_custom_tools(store);
    match customs.iter_mut().find(|c| c.key == key) {
        Some(custom) if !is_builtin_key(&key) => {
            custom.skills_dir = path;
            set_custom_tools(store, &customs)?;
        }
        _ => {
            let mut paths = get_custom_tool_paths(store);
            paths.insert(key.clone(), path);
            set_custom_tool_paths(store, &paths)?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn set_custom_tool_path(
    key: String,
    path: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || apply_tool_skills_dir(&store, &key, &path)).await?
}

#[tauri::command]
pub async fn reset_custom_tool_path(
    key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut paths = get_custom_tool_paths(&store);
        paths.remove(&key);
        set_custom_tool_paths(&store, &paths)
    })
    .await?
}

/// Store side of [`set_custom_tool_project_path`], separated so the write can
/// be tested against a real store without a Tauri runtime.
pub(crate) fn apply_tool_project_skills_dir(
    store: &SkillStore,
    key: &str,
    project_relative_skills_dir: Option<&str>,
) -> Result<(), AppError> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(AppError::invalid_input("Key is required"));
    }
    let normalized = normalize_project_relative_skills_dir_input(
        project_relative_skills_dir.unwrap_or_default(),
    )?;

    // Built-in tools keep overrides in a side map keyed by tool key. Resolve
    // the built-in default project path (no store overrides) to validate the
    // key and to detect no-op edits: an empty value, or one equal to the
    // default, removes the override and restores the default.
    //
    // Checked before custom tools, matching how resolution reads them back
    // (#378) — see `apply_tool_skills_dir`.
    let default_project_path = match tool_adapters::default_tool_adapters()
        .into_iter()
        .find(|a| a.key == key)
        .map(|a| a.project_relative_skills_dir().to_string())
    {
        Some(default_project_path) => default_project_path,
        // Custom tools store the project path on their definition; clearing it
        // (None) drops project-workspace support for that agent.
        None => {
            let mut customs = get_custom_tools(store);
            let custom = customs
                .iter_mut()
                .find(|c| c.key == key)
                .ok_or_else(|| AppError::not_found(format!("Unknown tool: {key}")))?;
            custom.project_relative_skills_dir = normalized;
            return set_custom_tools(store, &customs);
        }
    };
    let mut project_paths = get_custom_tool_project_paths(store);
    match normalized {
        Some(path) if path != default_project_path => {
            project_paths.insert(key, path);
        }
        _ => {
            project_paths.remove(&key);
        }
    }
    set_custom_tool_project_paths(store, &project_paths)
}

#[tauri::command]
pub async fn set_custom_tool_project_path(
    key: String,
    project_relative_skills_dir: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        apply_tool_project_skills_dir(&store, &key, project_relative_skills_dir.as_deref())
    })
    .await?
}

#[tauri::command]
pub async fn reset_custom_tool_project_path(
    key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(AppError::invalid_input("Key is required"));
        }
        if tool_adapters::find_adapter_with_store(&store, &key).is_none() {
            return Err(AppError::not_found(format!("Unknown tool: {key}")));
        }
        let mut project_paths = get_custom_tool_project_paths(&store);
        if project_paths.remove(&key).is_some() {
            set_custom_tool_project_paths(&store, &project_paths)?;
        }
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn add_custom_tool(
    key: String,
    display_name: String,
    skills_dir: String,
    project_relative_skills_dir: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let key = key.trim().to_string();
        let display_name = display_name.trim().to_string();
        let skills_dir = normalize_skills_dir_input(&skills_dir)?;
        let project_relative_skills_dir = normalize_project_relative_skills_dir_input(
            project_relative_skills_dir.as_deref().unwrap_or_default(),
        )?;
        if key.is_empty() || display_name.is_empty() || skills_dir.is_empty() {
            return Err(AppError::invalid_input(
                "Agent key, name and skills path are required",
            ));
        }

        // Validate key uniqueness
        let all = tool_adapters::all_tool_adapters(&store);
        if all.iter().any(|a| a.key == key) {
            return Err(AppError::invalid_input(format!(
                "Agent key \"{key}\" already exists"
            )));
        }
        let mut customs = get_custom_tools(&store);
        customs.push(CustomToolDef {
            key: key.clone(),
            display_name,
            skills_dir,
            project_relative_skills_dir,
            category: Default::default(),
        });
        set_custom_tools(&store, &customs)
    })
    .await?
}

#[tauri::command]
pub async fn remove_custom_tool(
    key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Target rows are legacy metadata only. Never remove a path from the
        // filesystem when a custom discovery source is removed.
        let targets = store.get_all_targets().unwrap_or_default();
        for target in targets.iter().filter(|t| t.tool == key) {
            store.delete_target(&target.skill_id, &key).ok();
        }
        // Remove from custom_tools list.
        let mut customs = get_custom_tools(&store);
        customs.retain(|c| c.key != key);
        set_custom_tools(&store, &customs)?;
        // Remove any stale override for this key.
        let mut custom_paths = get_custom_tool_paths(&store);
        custom_paths.remove(&key);
        set_custom_tool_paths(&store, &custom_paths)?;
        // Also remove from disabled_tools if present.
        let mut disabled = get_disabled_tools(&store);
        disabled.retain(|k| k != &key);
        set_disabled_tools(&store, &disabled)
    })
    .await?
}

pub fn migrate_legacy_tool_keys(store: &SkillStore) -> Result<(), AppError> {
    tool_service::migrate_legacy_tool_keys(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// #378: a user names a custom agent after one that later ships built-in.
    /// The key is derived from the display name, so "DeepSeek Harness" becomes
    /// `deepseek_harness` — and the collision check at creation time only knew
    /// the keys that existed then. Resolution now prefers the built-in, so a
    /// write that landed on the custom definition was stored where nothing
    /// reads it: the save reported success and the path never moved.
    fn store_with_colliding_custom_tool(dir: &std::path::Path, key: &str) -> SkillStore {
        let store = SkillStore::new(&dir.join("test.db")).unwrap();
        let customs = vec![CustomToolDef {
            key: key.to_string(),
            display_name: "DeepSeek Harness".to_string(),
            skills_dir: "/tmp/whatever-they-had".to_string(),
            project_relative_skills_dir: Some(".old/skills".to_string()),
            category: ToolCategory::Coding,
        }];
        store
            .set_setting("custom_tools", &serde_json::to_string(&customs).unwrap())
            .unwrap();
        store
    }

    #[test]
    fn a_path_edit_survives_a_custom_tool_shadowed_by_a_builtin() {
        let tmp = tempdir().unwrap();
        let store = store_with_colliding_custom_tool(tmp.path(), "deepseek_harness");
        let chosen = tmp.path().join("chosen");

        apply_tool_skills_dir(
            &store,
            "deepseek_harness",
            &chosen.to_string_lossy(),
        )
        .unwrap();

        let adapter = tool_adapters::find_adapter_with_store(&store, "deepseek_harness").unwrap();
        assert_eq!(adapter.skills_dir(), chosen, "the edit must be what resolves");
        assert!(adapter.has_path_override());
    }

    #[test]
    fn a_project_path_edit_survives_the_same_shadowing() {
        let tmp = tempdir().unwrap();
        let store = store_with_colliding_custom_tool(tmp.path(), "deepseek_harness");

        apply_tool_project_skills_dir(&store, "deepseek_harness", Some(".mine/skills")).unwrap();

        let adapter = tool_adapters::find_adapter_with_store(&store, "deepseek_harness").unwrap();
        assert_eq!(adapter.project_relative_skills_dir(), ".mine/skills");
    }

    /// A genuine custom agent — no built-in claims its key — still keeps both
    /// paths on its own definition, which is where its adapter reads them.
    #[test]
    fn a_real_custom_tool_still_stores_its_paths_on_its_definition() {
        let tmp = tempdir().unwrap();
        let store = store_with_colliding_custom_tool(tmp.path(), "my_own_agent");
        let chosen = tmp.path().join("chosen");

        apply_tool_skills_dir(&store, "my_own_agent", &chosen.to_string_lossy()).unwrap();
        apply_tool_project_skills_dir(&store, "my_own_agent", Some(".mine/skills")).unwrap();

        let customs = get_custom_tools(&store);
        let custom = customs.iter().find(|c| c.key == "my_own_agent").unwrap();
        assert_eq!(std::path::Path::new(&custom.skills_dir), chosen);
        assert_eq!(
            custom.project_relative_skills_dir.as_deref(),
            Some(".mine/skills")
        );

        let adapter = tool_adapters::find_adapter_with_store(&store, "my_own_agent").unwrap();
        assert_eq!(adapter.skills_dir(), chosen);
        assert_eq!(adapter.project_relative_skills_dir(), ".mine/skills");
    }

    #[test]
    fn discovery_toggle_never_writes_to_the_tool_directory() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let target_base = tmp.path().join("agent-skills");
        std::fs::create_dir_all(&target_base).unwrap();

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
        store
            .set_setting(
                "disabled_tools",
                &serde_json::to_string::<Vec<String>>(&vec![]).unwrap(),
            )
            .unwrap();

        set_tool_enabled_internal(&store, "test_agent", false).unwrap();
        set_tool_enabled_internal(&store, "test_agent", true).unwrap();

        assert!(target_base.read_dir().unwrap().next().is_none());
        assert!(store.get_all_targets().unwrap().is_empty());
    }
}
