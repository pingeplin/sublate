mod active;
mod app;
mod github;
mod installs;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

pub use active::ActiveRelease;
pub use app::{newer_release, AppArchive, AppDownloader, AppRelease, AppReleaseFeed};
pub use github::{GitHubAppReleases, GitHubReleases};
pub use installs::{Install, Installs, Version, PROGRAM};

use crate::disk;
use crate::error::{AppError, AppResult};

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const ARCHIVE: &str = ".download.zip";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub url: String,
    /// Lowercase hex digest of the archive at `url`.
    pub sha256: String,
}

#[async_trait]
pub trait ReleaseFeed: Send + Sync {
    async fn latest(&self) -> AppResult<Release>;
    async fn download(&self, release: &Release, dest: &Path) -> AppResult<()>;
}

/// Installs yt-dlp and keeps it current. It is downloaded rather than shipped: sites change
/// faster than the app ships, and a copy sealed inside the signed bundle could not be replaced.
pub struct Updater {
    feed: Box<dyn ReleaseFeed>,
    installs: Installs,
    /// Changed only here, and only while `running` is held.
    active: Arc<ActiveRelease>,
    /// Touched after each completed check; its age throttles the next one.
    marker: PathBuf,
    running: tokio::sync::Mutex<()>,
}

impl Updater {
    pub fn new(feed: Box<dyn ReleaseFeed>, installs: Installs, active: Arc<ActiveRelease>, marker: PathBuf) -> Self {
        Self {
            feed,
            installs,
            active,
            marker,
            running: tokio::sync::Mutex::new(()),
        }
    }

    /// The release in use, or `None` until one has been downloaded.
    pub fn version(&self) -> Option<Version> {
        self.active.version()
    }

    /// Switches to the latest release when it is newer than the one in use, and returns the
    /// version now in use. Unless forced, the first release is left for the user to ask for
    /// and the feed is asked at most once a day; a failed check is retried next time.
    pub async fn refresh(&self, force: bool) -> AppResult<Option<Version>> {
        let _running = self.running.lock().await;
        let current = self.active.version();
        if !force && (current.is_none() || !self.is_due()) {
            return Ok(current);
        }
        let release = self.feed.latest().await?;
        let install = if current.as_ref().is_none_or(|current| release.version > *current) {
            Some(self.install(&release).await?)
        } else {
            None
        };
        self.mark_checked(&release.version).await?;
        if let Some(install) = install {
            self.active.set(Some(install));
        }
        Ok(self.active.version())
    }

    /// Stops using any release, then deletes every installed one and the record of the last
    /// check, once any download in progress has finished.
    pub async fn clear(&self) -> AppResult<()> {
        let _running = self.running.lock().await;
        self.active.set(None);
        disk::delete(self.installs.root()).await?;
        Ok(disk::delete(&self.marker).await?)
    }

    async fn mark_checked(&self, latest: &Version) -> AppResult<()> {
        if let Some(dir) = self.marker.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        Ok(tokio::fs::write(&self.marker, latest.to_string()).await?)
    }

    async fn install(&self, release: &Release) -> AppResult<Install> {
        if let Some(installed) = self.installs.find(&release.version) {
            return Ok(installed);
        }
        tokio::fs::create_dir_all(self.installs.root()).await?;
        let archive = self.installs.root().join(ARCHIVE);
        let installed = self.fetch(release, &archive).await;
        let _ = tokio::fs::remove_file(&archive).await;
        installed
    }

    async fn fetch(&self, release: &Release, archive: &Path) -> AppResult<Install> {
        self.feed.download(release, archive).await?;
        let digest = sha256_hex(archive).await?;
        if !digest.eq_ignore_ascii_case(&release.sha256) {
            return Err(AppError::Update(format!(
                "release {} does not match its checksum",
                release.version
            )));
        }
        self.installs.add(&release.version, archive).await
    }

    fn is_due(&self) -> bool {
        let checked = std::fs::metadata(&self.marker).and_then(|meta| meta.modified());
        match checked.map(|at| SystemTime::now().duration_since(at)) {
            Ok(Ok(age)) => age >= CHECK_INTERVAL,
            _ => true,
        }
    }
}

pub(super) async fn sha256_hex(path: &Path) -> AppResult<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::installs::test_support::fake_archive;
    use super::*;

    #[derive(Default)]
    struct Calls {
        latest: AtomicUsize,
        downloads: AtomicUsize,
    }

    struct LocalFeed {
        release: AppResult<Release>,
        archive: PathBuf,
        calls: Arc<Calls>,
    }

    #[async_trait]
    impl ReleaseFeed for LocalFeed {
        async fn latest(&self) -> AppResult<Release> {
            self.calls.latest.fetch_add(1, Ordering::SeqCst);
            match &self.release {
                Ok(release) => Ok(release.clone()),
                Err(e) => Err(AppError::Update(e.to_string())),
            }
        }

        async fn download(&self, _release: &Release, dest: &Path) -> AppResult<()> {
            self.calls.downloads.fetch_add(1, Ordering::SeqCst);
            tokio::fs::copy(&self.archive, dest).await?;
            Ok(())
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        calls: Arc<Calls>,
        active: Arc<ActiveRelease>,
        updater: Updater,
    }

    impl Fixture {
        /// A feed offering `offered`, whose archive reports itself as that version.
        async fn offering(offered: &str) -> Self {
            Self::build(offered, |_| {}).await
        }

        async fn build(offered: &str, adjust: impl FnOnce(&mut LocalFeed)) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let archive = fake_archive(dir.path(), offered);
            let calls = Arc::new(Calls::default());
            let mut feed = LocalFeed {
                release: Ok(Release {
                    version: version(offered),
                    url: "https://example.com/release.zip".into(),
                    sha256: sha256_hex(&archive).await.unwrap().to_uppercase(),
                }),
                archive,
                calls: Arc::clone(&calls),
            };
            adjust(&mut feed);
            let active = Arc::new(ActiveRelease::default());
            let updater = Updater::new(
                Box::new(feed),
                Installs::new(dir.path().join("yt-dlp")),
                Arc::clone(&active),
                dir.path().join("support/last-check"),
            );
            Self {
                dir,
                calls,
                active,
                updater,
            }
        }

        /// As if `text` were installed earlier and is in use now.
        fn in_use(self, text: &str) -> Self {
            self.active.set(Some(Install::at(version(text), &self.dir.path().join("earlier"))));
            self
        }

        fn installed(&self) -> Option<Install> {
            Installs::new(self.dir.path().join("yt-dlp")).newest()
        }
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[tokio::test]
    async fn switches_to_a_newer_release_and_cleans_up_the_download() {
        let fixture = Fixture::offering("2026.09.02").await.in_use("2026.08.19");

        let now = fixture.updater.refresh(false).await.unwrap();

        assert_eq!(now, Some(version("2026.09.02")));
        assert_eq!(fixture.active.get(), fixture.installed());
        assert_eq!(fixture.updater.version(), now);
        assert!(!fixture.dir.path().join("yt-dlp").join(ARCHIVE).exists());
    }

    #[tokio::test]
    async fn the_first_release_is_installed_only_when_asked_for() {
        let fixture = Fixture::offering("2026.09.02").await;

        assert_eq!(fixture.updater.refresh(false).await.unwrap(), None);
        assert_eq!(fixture.calls.latest.load(Ordering::SeqCst), 0);

        assert_eq!(fixture.updater.refresh(true).await.unwrap(), Some(version("2026.09.02")));
        assert!(fixture.installed().is_some());
        assert_eq!(fixture.active.get(), fixture.installed());
    }

    #[tokio::test]
    async fn clearing_stops_using_the_release_deletes_the_installs_and_forgets_the_last_check() {
        let fixture = Fixture::offering("2026.09.02").await;
        fixture.updater.refresh(true).await.unwrap();

        fixture.updater.clear().await.unwrap();
        fixture.updater.clear().await.unwrap();

        assert_eq!(fixture.updater.version(), None);
        assert_eq!(fixture.installed(), None);
        assert!(!fixture.dir.path().join("yt-dlp").exists());
        assert!(!fixture.dir.path().join("support").join("last-check").exists());
    }

    #[tokio::test]
    async fn leaves_a_current_or_newer_release_in_use() {
        let current = Fixture::offering("2026.08.19").await.in_use("2026.08.19");
        assert_eq!(current.updater.refresh(false).await.unwrap(), Some(version("2026.08.19")));

        let newer = Fixture::offering("2026.08.19").await.in_use("2026.10.01");
        assert_eq!(newer.updater.refresh(true).await.unwrap(), Some(version("2026.10.01")));

        for fixture in [current, newer] {
            assert_eq!(fixture.calls.downloads.load(Ordering::SeqCst), 0);
            assert_eq!(fixture.installed(), None);
        }
    }

    #[tokio::test]
    async fn asks_the_feed_once_a_day_unless_forced_and_always_reports_the_release_in_use() {
        let fixture = Fixture::offering("2026.08.19").await.in_use("2026.08.19");
        let current = Some(version("2026.08.19"));

        assert_eq!(fixture.updater.refresh(false).await.unwrap(), current);
        assert_eq!(fixture.updater.refresh(false).await.unwrap(), current);
        assert_eq!(fixture.calls.latest.load(Ordering::SeqCst), 1);

        assert_eq!(fixture.updater.refresh(true).await.unwrap(), current);
        assert_eq!(fixture.calls.latest.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn reuses_a_release_that_is_already_installed() {
        let fixture = Fixture::offering("2026.09.02").await.in_use("2026.08.19");
        let first = fixture.updater.refresh(true).await.unwrap();
        let fixture = fixture.in_use("2026.08.19");

        let second = fixture.updater.refresh(true).await.unwrap();

        assert_eq!(first, second);
        assert_eq!(fixture.active.get(), fixture.installed());
        assert_eq!(fixture.calls.downloads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rejects_a_download_that_does_not_match_its_checksum() {
        let fixture = Fixture::build("2026.09.02", |feed| {
            feed.release.as_mut().unwrap().sha256 = "00".repeat(32);
        })
        .await
        .in_use("2026.08.19");

        let err = fixture.updater.refresh(false).await.unwrap_err();

        assert_eq!(err.to_string(), "yt-dlp update failed: release 2026.09.02 does not match its checksum");
        assert_eq!(fixture.updater.version(), Some(version("2026.08.19")));
        assert_eq!(fixture.installed(), None);
        assert!(!fixture.dir.path().join("yt-dlp").join(ARCHIVE).exists());
    }

    #[tokio::test]
    async fn a_failed_check_is_retried_on_the_next_refresh() {
        let fixture = Fixture::build("2026.09.02", |feed| {
            feed.release = Err(AppError::Update("offline".into()));
        })
        .await
        .in_use("2026.08.19");

        assert!(fixture.updater.refresh(false).await.is_err());
        assert!(fixture.updater.refresh(false).await.is_err());
        assert_eq!(fixture.calls.latest.load(Ordering::SeqCst), 2);
    }
}
