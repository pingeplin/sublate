use std::path::Path;
use std::sync::Arc;

use crate::error::AppError;
use crate::file_name::OutputLocation;
use crate::languages::{find_target, Language, TARGET_LANGUAGES};
use crate::metadata::{SubtitleTrack, VideoMetadata};
use crate::services::AppState;
use crate::subtitle_job::{download_and_translate, SubtitleOutput};

/// Flattened to its message, so the UI handles one case whatever the cause.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum BackendError {
    #[error(transparent)]
    Failed(#[from] AppError),
}

type BackendResult<T> = Result<T, BackendError>;

#[derive(Debug, PartialEq, Eq, uniffi::Record)]
pub struct TargetLanguage {
    pub code: String,
    pub native: String,
}

impl From<&Language> for TargetLanguage {
    fn from(language: &Language) -> Self {
        Self {
            code: language.code.into(),
            native: language.native.into(),
        }
    }
}

#[derive(uniffi::Record)]
pub struct VideoRef {
    pub url: String,
    pub id: String,
    pub title: String,
}

impl VideoRef {
    fn output(&self, out_dir: String) -> OutputLocation {
        OutputLocation::new(out_dir, &self.title, &self.id)
    }
}

#[derive(Debug, PartialEq, Eq, uniffi::Record)]
pub struct SubtitleFiles {
    pub source_path: String,
    pub translated_path: String,
    pub cue_count: u64,
}

impl From<SubtitleOutput> for SubtitleFiles {
    fn from(output: SubtitleOutput) -> Self {
        Self {
            source_path: path_string(&output.source_path),
            translated_path: path_string(&output.translated_path),
            cue_count: output.cue_count as u64,
        }
    }
}

/// Called from the runtime thread, never the UI thread.
#[uniffi::export(foreign)]
pub trait VideoProgressListener: Send + Sync {
    fn on_video_progress(&self, percent: f32);
}

/// Called from the runtime thread, never the UI thread.
#[uniffi::export(foreign)]
pub trait TranslationProgressListener: Send + Sync {
    fn on_translation_progress(&self, done: u64, total: u64);
}

#[derive(Default, uniffi::Object)]
pub struct Backend {
    state: AppState,
}

#[uniffi::export(async_runtime = "tokio")]
impl Backend {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn target_languages(&self) -> Vec<TargetLanguage> {
        TARGET_LANGUAGES.iter().map(TargetLanguage::from).collect()
    }

    pub async fn credential_source(&self) -> BackendResult<String> {
        Ok(self.state.services().await?.credential_source.clone())
    }

    pub async fn fetch_metadata(&self, url: String) -> BackendResult<VideoMetadata> {
        let services = self.state.services().await?;
        Ok(services.ytdlp.fetch_metadata(url.trim()).await?)
    }

    pub async fn download_video(
        &self,
        video: VideoRef,
        out_dir: String,
        listener: Arc<dyn VideoProgressListener>,
    ) -> BackendResult<String> {
        let services = self.state.services().await?;
        let path = services
            .ytdlp
            .download_video(&video.url, &video.output(out_dir), |percent| {
                listener.on_video_progress(percent)
            })
            .await?;
        Ok(path_string(&path))
    }

    pub async fn translate_subtitles(
        &self,
        video: VideoRef,
        out_dir: String,
        track: SubtitleTrack,
        target: String,
        listener: Arc<dyn TranslationProgressListener>,
    ) -> BackendResult<SubtitleFiles> {
        let target = find_target(&target).ok_or(AppError::UnknownLanguage(target))?;
        let services = self.state.services().await?;
        let output = download_and_translate(
            &services.ytdlp,
            services.translator.as_ref(),
            &video.url,
            &video.output(out_dir),
            &track,
            target,
            |done, total| listener.on_translation_progress(done as u64, total as u64),
        )
        .await?;
        Ok(output.into())
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::metadata::TrackKind;

    struct Silent;

    impl TranslationProgressListener for Silent {
        fn on_translation_progress(&self, _done: u64, _total: u64) {}
    }

    #[test]
    fn offers_every_supported_target_language() {
        let offered = Backend::new().target_languages();
        let codes: Vec<&str> = offered.iter().map(|l| l.code.as_str()).collect();
        let supported: Vec<&str> = TARGET_LANGUAGES.iter().map(|l| l.code).collect();
        assert_eq!(codes, supported);
        assert_eq!(offered[0].native, "繁體中文（台灣）");
    }

    #[tokio::test]
    async fn unknown_target_is_rejected_with_the_domain_message() {
        let video = VideoRef {
            url: "https://example.com/v".into(),
            id: "id".into(),
            title: "Title".into(),
        };
        let track = SubtitleTrack {
            code: "en".into(),
            name: "English".into(),
            kind: TrackKind::Manual,
        };
        let err = Backend::new()
            .translate_subtitles(video, "/nonexistent".into(), track, "xx".into(), Arc::new(Silent))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "unsupported target language: xx");
    }

    #[test]
    fn subtitle_files_carry_paths_as_strings() {
        let files = SubtitleFiles::from(SubtitleOutput {
            source_path: PathBuf::from("/out/T [id].en.srt"),
            translated_path: PathBuf::from("/out/T [id].zh-TW.srt"),
            cue_count: 3,
        });
        assert_eq!(
            files,
            SubtitleFiles {
                source_path: "/out/T [id].en.srt".into(),
                translated_path: "/out/T [id].zh-TW.srt".into(),
                cue_count: 3,
            }
        );
    }
}
