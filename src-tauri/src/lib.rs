pub mod auth;
mod commands;
pub mod error;
pub mod file_name;
pub mod languages;
pub mod metadata;
pub mod process_env;
pub mod punctuation;
mod services;
pub mod subtitle;
pub mod subtitle_job;
pub mod translate;
pub mod ytdlp;

use services::AppState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::target_languages,
            commands::credential_source,
            commands::fetch_metadata,
            commands::download_video,
            commands::translate_subtitles,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
