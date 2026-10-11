use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::OnceCell;

use crate::auth::CredentialChain;
use crate::error::{AppError, AppResult};
use crate::file_name::OutputLocation;
use crate::languages::find_target;
use crate::metadata::{SubtitleTrack, VideoMetadata};
use crate::subtitle_job::{download_and_translate, SubtitleOutput};
use crate::toolchain::Toolchain;
use crate::translate::claude::ClaudeTranslator;
use crate::translate::Translator;
use crate::update::{newer_release, AppRelease, GitHubAppReleases, GitHubReleases, Installs, Updater, Version};
use crate::ytdlp::YtDlp;

const INSTALLS_DIR: &str = "yt-dlp";
const UPDATE_MARKER: &str = "yt-dlp.last-check";

/// Where the app keeps what it runs and what it writes.
pub struct Locations {
    /// The bundled deno, ffmpeg and ffprobe.
    pub tools: PathBuf,
    /// The yt-dlp releases the app downloads.
    pub support: PathBuf,
    pub cache: PathBuf,
}

struct Services {
    ytdlp: YtDlp,
    translator: Box<dyn Translator>,
    updater: Updater,
}

impl Services {
    fn load(locations: &Locations, credentials: Arc<CredentialChain>) -> AppResult<Self> {
        let tools = Toolchain::bundled(&locations.tools)?;
        let installs = Installs::new(locations.support.join(INSTALLS_DIR));
        let newest = installs.newest();
        installs.retain(newest.as_ref().map(|install| &install.version));
        Ok(Self {
            ytdlp: YtDlp::new(tools, newest, &locations.cache),
            translator: Box::new(ClaudeTranslator::new(Box::new(credentials))?),
            updater: Updater::new(
                Box::new(GitHubReleases::new()?),
                installs,
                locations.support.join(UPDATE_MARKER),
            ),
        })
    }
}

/// What the app does for its user. Services are built on first use, so a broken installation
/// surfaces as an error message in the window rather than at launch.
pub struct AppState {
    locations: Locations,
    credentials: Arc<CredentialChain>,
    services: OnceCell<Services>,
}

impl AppState {
    pub fn new(locations: Locations) -> Self {
        Self {
            locations,
            credentials: Arc::default(),
            services: OnceCell::new(),
        }
    }

    pub fn credentials(&self) -> &CredentialChain {
        &self.credentials
    }

    /// The yt-dlp release in use, or `None` until one has been downloaded.
    pub async fn ytdlp_version(&self) -> AppResult<Option<Version>> {
        Ok(self.services().await?.ytdlp.version())
    }

    /// Switches to the latest yt-dlp release when it is newer, and returns the version now in
    /// use. Unless forced, the first release is not downloaded and the release feed is asked
    /// at most once a day.
    pub async fn update_ytdlp(&self, force: bool) -> AppResult<Option<Version>> {
        let services = self.services().await?;
        let current = services.ytdlp.version();
        if let Some(install) = services.updater.refresh(current.as_ref(), force).await? {
            services.ytdlp.switch_to(install);
        }
        Ok(services.ytdlp.version())
    }

    /// The Sublate release to move to, or `None` when `current`, the version that is running,
    /// is the latest. It needs none of the services, so a broken installation can still ask.
    pub async fn check_app_update(&self, current: &str) -> AppResult<Option<AppRelease>> {
        newer_release(&GitHubAppReleases::new()?, current).await
    }

    /// Deletes what the app wrote under the user's Library, the downloaded yt-dlp and the
    /// caches, leaving it as it was before its first launch.
    pub async fn clear_data(&self) -> AppResult<()> {
        let services = self.services().await?;
        services.ytdlp.uninstall();
        services.updater.clear().await?;
        services.ytdlp.clear_cache().await?;
        // Deleted only when empty: the system keeps files of its own beside the app's.
        for dir in [&self.locations.support, &self.locations.cache] {
            let _ = tokio::fs::remove_dir(dir).await;
        }
        Ok(())
    }

    pub async fn fetch_metadata(&self, url: &str) -> AppResult<VideoMetadata> {
        self.services().await?.ytdlp.fetch_metadata(url.trim()).await
    }

    pub async fn download_video(
        &self,
        url: &str,
        out: &OutputLocation,
        on_progress: impl Fn(f32),
    ) -> AppResult<PathBuf> {
        self.services().await?.ytdlp.download_video(url, out, on_progress).await
    }

    /// Downloads a subtitle track and translates it into the language with code `target`. The
    /// code is checked first, so a wrong one is reported even when the installation is broken.
    pub async fn translate_subtitles(
        &self,
        url: &str,
        out: &OutputLocation,
        track: &SubtitleTrack,
        target: &str,
        on_progress: impl Fn(usize, usize) + Sync,
    ) -> AppResult<SubtitleOutput> {
        let target = find_target(target).ok_or_else(|| AppError::UnknownLanguage(target.into()))?;
        let services = self.services().await?;
        let translator = services.translator.as_ref();
        download_and_translate(&services.ytdlp, translator, url, out, track, target, on_progress).await
    }

    async fn services(&self) -> AppResult<&Services> {
        let load = || async { Services::load(&self.locations, Arc::clone(&self.credentials)) };
        self.services.get_or_try_init(load).await
    }
}
