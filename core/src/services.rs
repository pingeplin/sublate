use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::OnceCell;

use crate::auth::CredentialChain;
use crate::error::AppResult;
use crate::toolchain::Toolchain;
use crate::translate::claude::ClaudeTranslator;
use crate::translate::Translator;
use crate::update::{GitHubReleases, Installs, Updater};
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

pub struct Services {
    pub ytdlp: YtDlp,
    pub translator: Box<dyn Translator>,
    pub updater: Updater,
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

/// Services are built on first use, so a broken installation surfaces as an error message
/// in the window rather than at launch.
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

    pub async fn services(&self) -> AppResult<&Services> {
        let load = || async { Services::load(&self.locations, Arc::clone(&self.credentials)) };
        self.services.get_or_try_init(load).await
    }
}
