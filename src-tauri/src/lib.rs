pub mod agent;
mod app;
pub mod crowbot;
pub mod crowquant;
mod crowquant_memory;
pub mod evolution;
pub mod membership;
pub mod memory;
mod memory_export;
pub mod openrouter;
pub mod startup;
pub mod storage;
pub mod tools;

use app::{
    crowclaw_action_decide, crowclaw_app_bootstrap, crowclaw_chat_send,
    crowclaw_conversation_create, crowclaw_conversation_get, crowclaw_crowquant_list,
    crowclaw_crowquant_recall, crowclaw_crowquant_remember, crowclaw_evolution_cancel,
    crowclaw_evolution_decide, crowclaw_evolution_draft, crowclaw_evolution_evaluate,
    crowclaw_evolution_feedback, crowclaw_evolution_rate, crowclaw_evolution_reflect,
    crowclaw_evolution_restore, crowclaw_evolution_snapshot, crowclaw_folder_select,
    crowclaw_membership_acknowledge_welcome, crowclaw_membership_cancel_sign_in,
    crowclaw_membership_manage_usage, crowclaw_membership_refresh_models,
    crowclaw_membership_sign_in, crowclaw_membership_sign_out, crowclaw_membership_snapshot,
    crowclaw_membership_use_model, crowclaw_memory_admit_file, crowclaw_memory_configure,
    crowclaw_memory_export, crowclaw_memory_rebuild, crowclaw_memory_search,
    crowclaw_memory_semantic_sync, crowclaw_memory_status, crowclaw_memory_sync,
    crowclaw_memory_withdraw, crowclaw_model_connect, crowclaw_model_discover,
    crowclaw_model_set_default, crowclaw_model_test_connection, crowclaw_settings_save,
    crowclaw_task_cancel, AppState,
};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let options = match startup::LaunchOptions::parse(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            rfd::MessageDialog::new()
                .set_title("CrowClaw startup")
                .set_description(&error)
                .set_level(rfd::MessageLevel::Error)
                .show();
            return;
        }
    };
    let mut context = tauri::generate_context!();
    if let Some(profile) = &options.profile_directory {
        for window in &mut context.config_mut().app.windows {
            window.data_directory = Some(profile.join("webview"));
            window.title = format!("{} — separate profile", window.title);
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                if let Some(state) = window.try_state::<AppState>() {
                    if state.exit_when_window_destroyed() {
                        window.app_handle().exit(0);
                    }
                }
            }
        })
        .setup(move |app| {
            let app_data_directory = match &options.profile_directory {
                Some(directory) => directory.clone(),
                None => app.path().app_data_dir()?,
            };
            let state = AppState::open(app_data_directory)?;
            state.start_memory_indexer();
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crowclaw_membership_snapshot,
            crowclaw_membership_sign_in,
            crowclaw_membership_cancel_sign_in,
            crowclaw_membership_sign_out,
            crowclaw_membership_refresh_models,
            app::crowclaw_codex_image_auth_begin,
            app::crowclaw_codex_image_auth_poll,
            app::crowclaw_codex_image_auth_status,
            app::crowclaw_codex_image_auth_cancel,
            crowclaw_membership_use_model,
            crowclaw_membership_acknowledge_welcome,
            crowclaw_membership_manage_usage,
            crowclaw_app_bootstrap,
            app::composer::crowclaw_composer_get,
            app::composer::crowclaw_composer_save_draft,
            app::composer::crowclaw_composer_choose,
            app::composer::crowclaw_composer_refresh_models,
            app::composer::crowclaw_model_picker_visibility,
            app::attachments::crowclaw_attachments_select,
            app::attachments::crowclaw_attachment_remove,
            app::attachments::crowclaw_attachment_preview,
            app::openrouter::crowclaw_openrouter_catalog,
            app::openrouter::crowclaw_openrouter_connect,
            app::openrouter::crowclaw_openrouter_disconnect,
            crowclaw_model_discover,
            crowclaw_model_test_connection,
            crowclaw_model_connect,
            crowclaw_model_set_default,
            crowclaw_conversation_create,
            crowclaw_conversation_get,
            crowclaw_folder_select,
            crowclaw_crowquant_list,
            crowclaw_crowquant_remember,
            crowclaw_crowquant_recall,
            crowclaw_chat_send,
            crowclaw_task_cancel,
            crowclaw_action_decide,
            crowclaw_settings_save,
            crowclaw_memory_status,
            crowclaw_memory_configure,
            crowclaw_memory_search,
            crowclaw_memory_withdraw,
            crowclaw_memory_sync,
            crowclaw_memory_rebuild,
            crowclaw_memory_export,
            crowclaw_memory_admit_file,
            crowclaw_memory_semantic_sync,
            crowclaw_evolution_snapshot,
            crowclaw_evolution_feedback,
            crowclaw_evolution_draft,
            crowclaw_evolution_reflect,
            crowclaw_evolution_evaluate,
            crowclaw_evolution_rate,
            crowclaw_evolution_decide,
            crowclaw_evolution_restore,
            crowclaw_evolution_cancel,
        ])
        .run(context)
        .expect("error while running tauri application");
}
