pub mod auth;
mod commands;
pub mod error;
pub mod file_name;
pub mod languages;
pub mod metadata;
pub mod process_env;
pub mod punctuation;
pub mod subtitle;
pub mod subtitle_job;
pub mod translate;
pub mod ytdlp;

use std::sync::Arc;

use tauri::Manager;

use commands::AppState;
use process_env::ProcessEnv;
use translate::claude::{ClaudeTranslator, DEFAULT_MODEL};
use ytdlp::YtDlp;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let env = ProcessEnv::from_login_shell();
            let credentials = auth::resolve_provider(&env);
            let credential_source = credentials.describe();
            let translator = ClaudeTranslator::new(credentials, DEFAULT_MODEL)?;
            app.manage(AppState {
                ytdlp: YtDlp::new(env),
                translator: Arc::new(translator),
                credential_source,
            });
            Ok(())
        })
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
