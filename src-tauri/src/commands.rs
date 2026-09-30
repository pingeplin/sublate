use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::file_name::OutputLocation;
use crate::languages::{find_target, Language, TARGET_LANGUAGES};
use crate::metadata::{SubtitleTrack, VideoMetadata};
use crate::services::AppState;
use crate::subtitle_job::{download_and_translate, SubtitleOutput};

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum JobEvent {
    VideoProgress { percent: f32 },
    TranslationProgress { done: usize, total: usize },
}

#[derive(Deserialize)]
pub struct VideoRef {
    pub url: String,
    pub id: String,
    pub title: String,
}

impl VideoRef {
    fn output(&self, out_dir: PathBuf) -> OutputLocation {
        OutputLocation::new(out_dir, &self.title, &self.id)
    }
}

#[tauri::command]
pub fn target_languages() -> Vec<Language> {
    TARGET_LANGUAGES.to_vec()
}

#[tauri::command]
pub async fn credential_source(state: State<'_, AppState>) -> AppResult<String> {
    Ok(state.services().await?.credential_source.clone())
}

#[tauri::command]
pub async fn fetch_metadata(state: State<'_, AppState>, url: String) -> AppResult<VideoMetadata> {
    state.services().await?.ytdlp.fetch_metadata(url.trim()).await
}

#[tauri::command]
pub async fn download_video(
    state: State<'_, AppState>,
    video: VideoRef,
    out_dir: PathBuf,
    on_event: Channel<JobEvent>,
) -> AppResult<PathBuf> {
    let services = state.services().await?;
    services
        .ytdlp
        .download_video(&video.url, &video.output(out_dir), |percent| {
            let _ = on_event.send(JobEvent::VideoProgress { percent });
        })
        .await
}

#[tauri::command]
pub async fn translate_subtitles(
    state: State<'_, AppState>,
    video: VideoRef,
    out_dir: PathBuf,
    track: SubtitleTrack,
    target: String,
    on_event: Channel<JobEvent>,
) -> AppResult<SubtitleOutput> {
    let target = find_target(&target).ok_or(AppError::UnknownLanguage(target))?;
    let services = state.services().await?;
    download_and_translate(
        &services.ytdlp,
        services.translator.as_ref(),
        &video.url,
        &video.output(out_dir),
        &track,
        target,
        |done, total| {
            let _ = on_event.send(JobEvent::TranslationProgress { done, total });
        },
    )
    .await
}
