use std::sync::Arc;
use std::time::Instant;

pub mod commands;
pub mod core;

/// Exit the desktop application.
///
/// AgentDock is an ordinary desktop app: closing the main window exits the
/// process. There is no tray icon, close-to-background mode, or restart hook.
pub fn quit_app(app: &tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let pre_builder_start = Instant::now();
    let (store, startup_timings) =
        core::app_state::initialize_store().expect("Failed to initialize app state");
    let pre_builder_ms = pre_builder_start.elapsed().as_millis();

    let cancel_registry = Arc::new(core::install_cancel::InstallCancelRegistry::new());

    let builder_start = Instant::now();
    tauri::Builder::default()
        .manage(store)
        .manage(cancel_registry)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            let builder_to_setup_ms = builder_start.elapsed().as_millis();
            let setup_start = Instant::now();

            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .level_for("tao", log::LevelFilter::Warn)
                    .level_for("wry", log::LevelFilter::Warn)
                    .level_for("hyper", log::LevelFilter::Warn)
                    .level_for("reqwest", log::LevelFilter::Warn)
                    .level_for("rustls", log::LevelFilter::Warn)
                    .max_file_size(5 * 1024 * 1024)
                    .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                    .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                    .format(|out, message, record| {
                        out.finish(format_args!(
                            "{} {:5} [{}] {}",
                            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
                            record.level(),
                            record.target(),
                            message
                        ))
                    })
                    .build(),
            )?;

            core::panic_log::install_panic_hook(app.handle().clone());
            log::info!(
                "app start: version={} os={} arch={}",
                app.config().version.clone().unwrap_or_default(),
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            log::info!(
                "startup: pre_builder {} ms, builder_to_setup {} ms",
                pre_builder_ms,
                builder_to_setup_ms
            );
            startup_timings.log();

            for detail in core::central_repo::take_startup_errors() {
                log::error!("{detail}");
            }

            log::info!(
                "startup: setup() body total {} ms",
                setup_start.elapsed().as_millis()
            );

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Tools
            commands::tools::get_tool_status,
            commands::tools::set_tool_enabled,
            commands::tools::set_all_tools_enabled,
            commands::tools::get_tool_order_cmd,
            commands::tools::set_tool_order_cmd,
            commands::tools::set_custom_tool_path,
            commands::tools::reset_custom_tool_path,
            commands::tools::set_custom_tool_project_path,
            commands::tools::reset_custom_tool_project_path,
            commands::tools::add_custom_tool,
            commands::tools::remove_custom_tool,
            // Skills
            commands::skills::get_managed_skills,
            commands::skills::get_skills_for_preset,
            commands::skills::get_skill_document,
            commands::skills::get_source_skill_document,
            commands::skills::get_skill_source_diff,
            commands::skills::delete_managed_skill,
            commands::skills::delete_managed_skills,
            commands::skills::install_local,
            commands::skills::preview_git_install,
            commands::skills::confirm_git_install,
            commands::skills::cancel_git_preview,
            commands::skills::check_skill_update,
            commands::skills::check_all_skill_updates,
            commands::skills::update_skill,
            commands::skills::batch_update_skills,
            commands::skills::reimport_local_skill,
            commands::skills::relink_local_skill_source,
            commands::skills::detach_local_skill_source,
            commands::skills::get_all_tags,
            commands::skills::set_skill_tags,
            commands::skills::rename_tag,
            commands::skills::delete_tag,
            commands::skills::cancel_install,
            // Unified resource inventory
            commands::inventory::get_skill_inventory,
            commands::inventory::get_custom_read_only_paths,
            commands::inventory::set_custom_read_only_paths,
            commands::inventory::get_project_skill_inventory,
            commands::inventory::get_mcp_inventory,
            commands::inventory::adopt_skill_to_user,
            commands::inventory::adopt_skill_to_project,
            commands::inventory::delete_canonical_skill,
            commands::inventory::read_canonical_skill_document,
            commands::inventory::save_canonical_skill_document,
            commands::inventory::run_legacy_migration,
            commands::inventory::get_canonical_roots,
            // Settings and diagnostics
            commands::settings::get_settings,
            commands::settings::set_settings,
            commands::settings::get_central_repo_path,
            commands::settings::get_central_repo_path_override,
            commands::settings::get_central_repo_warnings,
            commands::settings::set_central_repo_path,
            commands::settings::open_central_repo_folder,
            commands::settings::get_diagnostic_info,
            commands::settings::get_recent_log_excerpt,
            commands::settings::export_logs_zip,
            commands::settings::log_startup_event,
            commands::settings::check_last_panic,
            commands::settings::clear_last_panic,
            commands::settings::app_exit,
            // Projects
            commands::projects::get_projects,
            commands::projects::add_project,
            commands::projects::remove_project,
            commands::projects::scan_projects,
            commands::projects::get_project_skills,
            commands::projects::get_project_skill_document,
            commands::projects::import_project_skill_to_center,
            commands::projects::copy_skill_to_project,
            commands::projects::update_project_skill_to_center,
            commands::projects::update_project_skill_from_center,
            commands::projects::toggle_project_skill,
            commands::projects::delete_project_skill,
            commands::projects::slugify_skill_names,
            // Presets
            commands::presets::get_presets,
            commands::presets::get_active_preset,
            commands::presets::create_preset,
            commands::presets::update_preset,
            commands::presets::delete_preset,
            commands::presets::add_skill_to_preset,
            commands::presets::remove_skill_from_preset,
            commands::presets::reorder_presets,
            commands::projects::reorder_projects,
            commands::presets::get_preset_skill_order,
            commands::presets::reorder_preset_skills,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
