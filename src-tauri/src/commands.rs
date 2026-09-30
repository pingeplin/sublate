use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::file_name::OutputLocation;
use crate::languages::{find_target, Language, TARGET_LANGUAGES};
use crate::metadata::{SubtitleTrack, VideoMetadata};
use crate::subtitle_job::translate_subtitle_file;
use crate::translate::{TranslationPlan, Translator};
use crate::ytdlp::YtDlp;

pub struct AppState {
    pub ytdlp: YtDlp,
    pub translator: Arc<dyn Translator>,
    pub credential_source: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum JobEvent {
    VideoProgress { percent: f32 },
    SubtitleDownloaded { path: PathBuf },
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleOutput {
    pub source_path: PathBuf,
    pub translated_path: PathBuf,
    pub cue_count: usize,
}

#[tauri::command]
pub fn target_languages() -> Vec<Language> {
    TARGET_LANGUAGES.to_vec()
}

#[tauri::command]
pub fn credential_source(state: State<'_, AppState>) -> String {
    state.credential_source.clone()
}

#[tauri::command]
pub async fn fetch_metadata(state: State<'_, AppState>, url: String) -> AppResult<VideoMetadata> {
    state.ytdlp.fetch_metadata(url.trim()).await
}

#[tauri::command]
pub async fn download_video(
    state: State<'_, AppState>,
    video: VideoRef,
    out_dir: PathBuf,
    on_event: Channel<JobEvent>,
) -> AppResult<PathBuf> {
    let out = video.output(out_dir);
    tokio::fs::create_dir_all(out.dir()).await?;
    state
        .ytdlp
        .download_video(&video.url, &out, |percent| {
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
    let out = video.output(out_dir);
    tokio::fs::create_dir_all(out.dir()).await?;

    let source_path = state
        .ytdlp
        .download_subtitle(&video.url, &out, &track)
        .await?;
    let _ = on_event.send(JobEvent::SubtitleDownloaded {
        path: source_path.clone(),
    });

    let translated_path = out.subtitle(target.code);
    let cue_count = translate_subtitle_file(
        state.translator.as_ref(),
        &source_path,
        &track,
        target,
        &translated_path,
        TranslationPlan::default(),
        |done, total| {
            let _ = on_event.send(JobEvent::TranslationProgress { done, total });
        },
    )
    .await?;

    Ok(SubtitleOutput {
        source_path,
        translated_path,
        cue_count,
    })
}
