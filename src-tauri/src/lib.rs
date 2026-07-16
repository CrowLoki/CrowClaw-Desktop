pub mod agent;
mod app;
pub mod crowquant;
pub mod storage;
pub mod tools;

use app::{
    crowclaw_action_decide, crowclaw_app_bootstrap, crowclaw_chat_send,
    crowclaw_conversation_create, crowclaw_conversation_get, crowclaw_crowquant_list,
    crowclaw_crowquant_recall, crowclaw_crowquant_remember, crowclaw_folder_select,
    crowclaw_model_connect, crowclaw_model_discover, crowclaw_model_test_connection,
    crowclaw_settings_save, crowclaw_task_cancel, AppState,
};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_directory = app.path().app_data_dir()?;
            app.manage(AppState::open(app_data_directory)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crowclaw_app_bootstrap,
            crowclaw_model_discover,
            crowclaw_model_test_connection,
            crowclaw_model_connect,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
