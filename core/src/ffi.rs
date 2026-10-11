use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::AppError;
use crate::file_name::OutputLocation;
use crate::languages::{Language, TARGET_LANGUAGES};
use crate::metadata::{SubtitleTrack, VideoMetadata};
use crate::services::{AppState, Locations};
use crate::subtitle_job::SubtitleOutput;
use crate::update::{AppRelease, Version};

/// Flattened to its message, so the UI handles one case whatever the cause.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum BackendError {
    #[error(transparent)]
    Failed(#[from] AppError),
}

type BackendResult<T> = Result<T, BackendError>;

#[derive(uniffi::Record)]
pub struct BackendConfig {
    /// Directory holding the bundled deno, ffmpeg and ffprobe.
    pub tools_dir: String,
    /// Writable directory for the yt-dlp releases the app downloads.
    pub support_dir: String,
    pub cache_dir: String,
}

impl From<BackendConfig> for Locations {
    fn from(config: BackendConfig) -> Self {
        Self {
            tools: PathBuf::from(config.tools_dir),
            support: PathBuf::from(config.support_dir),
            cache: PathBuf::from(config.cache_dir),
        }
    }
}

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

/// A newer Sublate and the page it is downloaded from.
#[derive(Debug, PartialEq, Eq, uniffi::Record)]
pub struct AppUpdate {
    pub version: String,
    pub page_url: String,
}

impl From<AppRelease> for AppUpdate {
    fn from(release: AppRelease) -> Self {
        Self {
            version: release.version.to_string(),
            page_url: release.page,
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

#[derive(uniffi::Object)]
pub struct Backend {
    state: AppState,
}

#[uniffi::export(async_runtime = "tokio")]
impl Backend {
    #[uniffi::constructor]
    pub fn new(config: BackendConfig) -> Self {
        Self {
            state: AppState::new(config.into()),
        }
    }

    pub fn target_languages(&self) -> Vec<TargetLanguage> {
        TARGET_LANGUAGES.iter().map(TargetLanguage::from).collect()
    }

    /// The Anthropic API key the user saved in the app; `None` forgets it.
    pub fn set_api_key(&self, key: Option<String>) {
        self.state.credentials().save_key(key);
    }

    /// Where the Claude credential comes from, or `None` while the user has yet to provide one.
    pub async fn credential_source(&self) -> Option<String> {
        self.state.credentials().source().await
    }

    /// The yt-dlp release in use, or `None` until one has been downloaded.
    pub async fn ytdlp_version(&self) -> BackendResult<Option<String>> {
        Ok(version_text(self.state.ytdlp_version().await?))
    }

    /// Switches to the latest yt-dlp release when it is newer, and returns the version now in
    /// use. Unless forced, the first release is not downloaded and the release feed is asked
    /// at most once a day.
    pub async fn update_ytdlp(&self, force: bool) -> BackendResult<Option<String>> {
        Ok(version_text(self.state.update_ytdlp(force).await?))
    }

    /// The Sublate release to move to, or `None` when `current_version`, the version that is
    /// running, is the latest. Nothing is downloaded.
    pub async fn check_app_update(&self, current_version: String) -> BackendResult<Option<AppUpdate>> {
        let release = self.state.check_app_update(&current_version).await?;
        Ok(release.map(AppUpdate::from))
    }

    /// Deletes the downloaded yt-dlp and the caches; yt-dlp has to be downloaded again.
    pub async fn clear_data(&self) -> BackendResult<()> {
        Ok(self.state.clear_data().await?)
    }

    pub async fn fetch_metadata(&self, url: String) -> BackendResult<VideoMetadata> {
        Ok(self.state.fetch_metadata(&url).await?)
    }

    pub async fn download_video(
        &self,
        video: VideoRef,
        out_dir: String,
        listener: Arc<dyn VideoProgressListener>,
    ) -> BackendResult<String> {
        let path = self
            .state
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
        let output = self
            .state
            .translate_subtitles(&video.url, &video.output(out_dir), &track, &target, |done, total| {
                listener.on_translation_progress(done as u64, total as u64)
            })
            .await?;
        Ok(output.into())
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn version_text(version: Option<Version>) -> Option<String> {
    version.map(|version| version.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::metadata::TrackKind;

    struct Silent;

    fn backend() -> Backend {
        Backend::new(BackendConfig {
            tools_dir: "/nonexistent/tools".into(),
            support_dir: "/nonexistent/support".into(),
            cache_dir: "/nonexistent/cache".into(),
        })
    }

    impl TranslationProgressListener for Silent {
        fn on_translation_progress(&self, _done: u64, _total: u64) {}
    }

    #[test]
    fn offers_every_supported_target_language() {
        let offered = backend().target_languages();
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
        let err = backend()
            .translate_subtitles(video, "/nonexistent".into(), track, "xx".into(), Arc::new(Silent))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "unsupported target language: xx");
    }

    #[tokio::test]
    async fn an_incomplete_installation_is_reported_not_fatal() {
        let err = backend().fetch_metadata("https://example.com/v".into()).await.unwrap_err();
        assert!(err.to_string().starts_with("the app's bundled tools are unusable: "), "{err}");
    }

    /// As the app is right after installing: the bundled tools and nothing downloaded yet.
    fn fresh_installation(dir: &Path) -> Backend {
        let tools = dir.join("tools");
        std::fs::create_dir(&tools).unwrap();
        for tool in ["deno", "ffmpeg", "ffprobe"] {
            std::fs::write(tools.join(tool), "").unwrap();
        }
        Backend::new(BackendConfig {
            tools_dir: path_string(&tools),
            support_dir: path_string(&dir.join("support")),
            cache_dir: path_string(&dir.join("cache")),
        })
    }

    #[tokio::test]
    async fn a_fresh_installation_has_no_ytdlp_until_the_user_downloads_it() {
        let dir = tempfile::tempdir().unwrap();
        let backend = fresh_installation(dir.path());

        assert_eq!(backend.ytdlp_version().await.unwrap(), None);
        assert_eq!(backend.update_ytdlp(false).await.unwrap(), None);
        let err = backend.fetch_metadata("https://example.com/v".into()).await.unwrap_err();
        assert_eq!(err.to_string(), "yt-dlp is not installed; download it in Settings (⌘,)");
    }

    #[tokio::test]
    async fn clearing_data_deletes_what_the_app_wrote_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let release = dir.path().join("support/yt-dlp/2026.08.19");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(release.join(crate::update::PROGRAM), "").unwrap();
        std::fs::write(dir.path().join("support/yt-dlp.last-check"), "2026.08.19").unwrap();
        for cache in ["cache/yt-dlp", "cache/deno"] {
            std::fs::create_dir_all(dir.path().join(cache)).unwrap();
        }
        std::fs::write(dir.path().join("cache/Cache.db"), "the system's").unwrap();
        let backend = fresh_installation(dir.path());
        assert_eq!(backend.ytdlp_version().await.unwrap().as_deref(), Some("2026.08.19"));

        backend.clear_data().await.unwrap();

        assert_eq!(backend.ytdlp_version().await.unwrap(), None);
        assert!(!dir.path().join("support").exists());
        let cached: Vec<_> = std::fs::read_dir(dir.path().join("cache")).unwrap().flatten().collect();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].file_name(), "Cache.db");
    }

    #[tokio::test]
    async fn a_saved_api_key_becomes_the_credential_source() {
        let backend = backend();
        backend.set_api_key(Some("sk-ant-test".into()));
        assert_eq!(backend.credential_source().await.as_deref(), Some("your saved API key"));
    }

    #[tokio::test]
    async fn an_update_check_works_on_a_broken_installation_and_rejects_a_bad_version() {
        let err = backend().check_app_update("dev".into()).await.unwrap_err();
        assert_eq!(err.to_string(), "Sublate update check failed: 'dev' is not a Sublate version");
    }

    #[test]
    fn an_app_update_carries_its_version_and_page_as_strings() {
        let update = AppUpdate::from(AppRelease {
            version: Version::parse("0.2.0").unwrap(),
            page: "https://example.com/v0.2.0".into(),
        });
        assert_eq!(
            update,
            AppUpdate {
                version: "0.2.0".into(),
                page_url: "https://example.com/v0.2.0".into(),
            }
        );
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
