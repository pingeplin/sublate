use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::installs::unpack;
use super::{sha256_hex, Version};
use crate::disk;
use crate::error::{AppError, AppResult};

/// The bundle every app archive holds, alone at its top level.
pub const APP_BUNDLE: &str = "Sublate.app";
const UNPACKED: &str = ".unpacked";

/// The file of a release that the app installs from.
pub fn archive_name(version: &Version) -> String {
    format!("Sublate-{version}.zip")
}

/// The archive of a release, as the release lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppArchive {
    /// Lowercase hex digest of the archive.
    pub sha256: String,
}

/// A published version of the app itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRelease {
    pub version: Version,
    /// The page the user downloads it from.
    pub page: String,
    /// `None` when the release has no archive to install from, only the page.
    pub archive: Option<AppArchive>,
}

/// Bytes received so far, and the full size when the server tells it.
pub type DownloadProgress<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);

#[async_trait]
pub trait AppReleaseFeed: Send + Sync {
    async fn latest(&self) -> AppResult<AppRelease>;
    /// Downloads the archive of `version` to `dest`.
    async fn download(&self, version: &Version, dest: &Path, on_progress: DownloadProgress<'_>) -> AppResult<()>;
}

/// The latest release when it is newer than `current`, the version that is running.
pub async fn newer_release(feed: &dyn AppReleaseFeed, current: &str) -> AppResult<Option<AppRelease>> {
    let current = parse_version(current).map_err(AppError::AppUpdate)?;
    let latest = feed.latest().await?;
    Ok((latest.version > current).then_some(latest))
}

/// App versions are digits and dots, like the `MARKETING_VERSION` they come from.
fn parse_version(text: &str) -> Result<Version, String> {
    Version::parse(text).ok_or_else(|| format!("'{}' is not a Sublate version", text.trim()))
}

/// Fetches the archive of a release into a staging folder of its own and unpacks it there.
/// Installing the bundle is left to the app, which can check its signature.
pub struct AppDownloader {
    feed: Box<dyn AppReleaseFeed>,
    staging: PathBuf,
}

impl AppDownloader {
    pub fn new(feed: Box<dyn AppReleaseFeed>, staging: PathBuf) -> Self {
        Self { feed, staging }
    }

    /// Returns the unpacked bundle, the only entry left in the staging folder. Whatever an
    /// earlier attempt left there is removed first; after a failure the folder is empty.
    pub async fn download(
        &self,
        version: &str,
        sha256: Option<&str>,
        on_progress: DownloadProgress<'_>,
    ) -> AppResult<PathBuf> {
        let version = parse_version(version).map_err(AppError::AppInstall)?;
        let sha256 = sha256.ok_or_else(|| {
            AppError::AppInstall(format!("release {version} has no {}", archive_name(&version)))
        })?;
        let staged = self.restage(&version, sha256, on_progress).await;
        if staged.is_err() {
            self.empty_staging().await;
        }
        staged.map_err(as_install_failure)
    }

    async fn restage(&self, version: &Version, sha256: &str, on_progress: DownloadProgress<'_>) -> AppResult<PathBuf> {
        disk::delete(&self.staging).await?;
        tokio::fs::create_dir_all(&self.staging).await?;
        self.stage(version, sha256, on_progress).await
    }

    async fn stage(&self, version: &Version, sha256: &str, on_progress: DownloadProgress<'_>) -> AppResult<PathBuf> {
        let archive = self.staging.join(archive_name(version));
        self.feed.download(version, &archive, on_progress).await?;
        if !sha256_hex(&archive).await?.eq_ignore_ascii_case(sha256) {
            return Err(AppError::AppInstall(format!("release {version} does not match its checksum")));
        }
        let unpacked = self.staging.join(UNPACKED);
        unpack(&archive, &unpacked).await.map_err(AppError::AppInstall)?;
        let bundle = sole_bundle(&unpacked, version)?;
        let staged = self.staging.join(APP_BUNDLE);
        tokio::fs::rename(bundle, &staged).await?;
        disk::delete(&unpacked).await?;
        disk::delete(&archive).await?;
        Ok(staged)
    }

    async fn empty_staging(&self) {
        let Ok(entries) = std::fs::read_dir(&self.staging) else { return };
        for entry in entries.flatten() {
            let _ = disk::delete(&entry.path()).await;
        }
    }
}

/// Any failure of a download, such as a full disk, is reported as a failed update.
fn as_install_failure(error: AppError) -> AppError {
    match error {
        AppError::AppInstall(_) => error,
        other => AppError::AppInstall(other.to_string()),
    }
}

/// The bundle at the top of an unpacked archive, which must hold that folder and nothing else.
fn sole_bundle(unpacked: &Path, version: &Version) -> AppResult<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(unpacked)?.flatten().collect();
    let bundle = unpacked.join(APP_BUNDLE);
    let is_folder = std::fs::symlink_metadata(&bundle).is_ok_and(|meta| meta.is_dir());
    if entries.len() == 1 && is_folder {
        Ok(bundle)
    } else {
        Err(AppError::AppInstall(format!(
            "{} does not hold {APP_BUNDLE} alone",
            archive_name(version)
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::super::installs::UNZIP;
    use super::*;

    struct Offering {
        latest: Result<&'static str, &'static str>,
        asked: AtomicUsize,
    }

    impl Offering {
        fn new(latest: Result<&'static str, &'static str>) -> Self {
            Self {
                latest,
                asked: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl AppReleaseFeed for Offering {
        async fn latest(&self) -> AppResult<AppRelease> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            match self.latest {
                Ok(version) => Ok(release(version)),
                Err(reason) => Err(AppError::AppUpdate(reason.into())),
            }
        }

        async fn download(&self, _: &Version, _: &Path, _: DownloadProgress<'_>) -> AppResult<()> {
            unreachable!("a check downloads nothing")
        }
    }

    fn release(version: &str) -> AppRelease {
        AppRelease {
            version: Version::parse(version).unwrap(),
            page: format!("https://example.com/v{version}"),
            archive: None,
        }
    }

    #[tokio::test]
    async fn a_newer_release_is_reported() {
        let feed = Offering::new(Ok("0.2.0"));
        assert_eq!(newer_release(&feed, "0.1.0").await.unwrap(), Some(release("0.2.0")));
    }

    #[tokio::test]
    async fn the_running_version_or_an_older_release_is_not_an_update() {
        let feed = Offering::new(Ok("0.2.0"));
        assert_eq!(newer_release(&feed, "0.2.0").await.unwrap(), None);
        assert_eq!(newer_release(&feed, "0.3.0").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_failed_feed_fails_the_check() {
        let feed = Offering::new(Err("offline"));
        let err = newer_release(&feed, "0.1.0").await.unwrap_err();
        assert_eq!(err.to_string(), "Sublate update check failed: offline");
    }

    #[tokio::test]
    async fn an_unreadable_running_version_is_reported_without_asking_the_feed() {
        let feed = Offering::new(Ok("0.2.0"));

        let err = newer_release(&feed, "dev").await.unwrap_err();

        assert_eq!(err.to_string(), "Sublate update check failed: 'dev' is not a Sublate version");
        assert_eq!(feed.asked.load(Ordering::SeqCst), 0);
    }

    /// Serves one archive from disk, or fails partway through when `archive` is `None`.
    struct LocalArchive {
        archive: Option<PathBuf>,
        requested: Requests,
    }

    type Requests = Arc<Mutex<Vec<(String, PathBuf)>>>;

    #[async_trait]
    impl AppReleaseFeed for LocalArchive {
        async fn latest(&self) -> AppResult<AppRelease> {
            unreachable!("a download checks nothing")
        }

        async fn download(&self, version: &Version, dest: &Path, on_progress: DownloadProgress<'_>) -> AppResult<()> {
            self.requested.lock().unwrap().push((version.to_string(), dest.to_path_buf()));
            let Some(archive) = &self.archive else {
                std::fs::write(dest, "partial").unwrap();
                return Err(AppError::Io(std::io::Error::other("connection reset")));
            };
            let size = std::fs::copy(archive, dest)?;
            on_progress(size, Some(size));
            Ok(())
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        staging: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let staging = dir.path().join("cache/app-update");
            Self { dir, staging }
        }

        /// A zip of a folder whose top level is built by `fill`.
        fn archive(&self, fill: impl FnOnce(&Path)) -> PathBuf {
            let content = self.dir.path().join("content");
            std::fs::create_dir_all(&content).unwrap();
            fill(&content);
            let archive = self.dir.path().join("served.zip");
            let status = std::process::Command::new(UNZIP)
                .args(["-c", "-k"])
                .arg(&content)
                .arg(&archive)
                .status()
                .unwrap();
            assert!(status.success());
            archive
        }

        fn app_archive(&self) -> PathBuf {
            self.archive(|top| fake_bundle(&top.join(APP_BUNDLE), "new"))
        }

        /// The downloader, and the versions and paths its feed was asked to download.
        fn downloader(&self, archive: Option<PathBuf>) -> (AppDownloader, Requests) {
            let requested = Requests::default();
            let feed = LocalArchive {
                archive,
                requested: Arc::clone(&requested),
            };
            (AppDownloader::new(Box::new(feed), self.staging.clone()), requested)
        }

        fn staged_entries(&self) -> Vec<String> {
            let mut names: Vec<_> = std::fs::read_dir(&self.staging)
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    fn fake_bundle(bundle: &Path, marker: &str) {
        let contents = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(contents.join("Sublate"), marker).unwrap();
    }

    async fn sha_of(path: &Path) -> String {
        sha256_hex(path).await.unwrap()
    }

    fn silent(_: u64, _: Option<u64>) {}

    #[tokio::test]
    async fn a_matching_archive_is_unpacked_to_the_staged_bundle_and_the_zip_is_removed() {
        let fixture = Fixture::new();
        let archive = fixture.app_archive();
        let sha = sha_of(&archive).await.to_uppercase();
        let (downloader, requested) = fixture.downloader(Some(archive));
        let reports = Mutex::new(Vec::new());

        let staged = downloader
            .download("0.4.0", Some(&sha), &|done, total| reports.lock().unwrap().push((done, total)))
            .await
            .unwrap();

        assert_eq!(staged, fixture.staging.join("Sublate.app"));
        assert_eq!(std::fs::read_to_string(staged.join("Contents/MacOS/Sublate")).unwrap(), "new");
        assert_eq!(fixture.staged_entries(), ["Sublate.app"]);
        let requested = requested.lock().unwrap();
        assert_eq!(requested.len(), 1);
        assert_eq!(requested[0].0, "0.4.0");
        assert_eq!(requested[0].1, fixture.staging.join("Sublate-0.4.0.zip"));
        assert_eq!(reports.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn leftovers_of_an_earlier_attempt_are_removed_first() {
        let fixture = Fixture::new();
        let archive = fixture.app_archive();
        let sha = sha_of(&archive).await;
        fake_bundle(&fixture.staging.join("Sublate.app"), "old");
        std::fs::write(fixture.staging.join("Sublate-0.3.9.zip"), "partial").unwrap();
        std::fs::create_dir_all(fixture.staging.join(".unpacked/Other.app")).unwrap();
        let (downloader, _) = fixture.downloader(Some(archive));

        let staged = downloader.download("0.4.0", Some(&sha), &silent).await.unwrap();

        assert_eq!(fixture.staged_entries(), ["Sublate.app"]);
        assert_eq!(std::fs::read_to_string(staged.join("Contents/MacOS/Sublate")).unwrap(), "new");
    }

    #[tokio::test]
    async fn an_archive_that_does_not_match_its_checksum_leaves_the_staging_folder_empty() {
        let fixture = Fixture::new();
        let (downloader, _) = fixture.downloader(Some(fixture.app_archive()));

        let err = downloader.download("0.4.0", Some(&"00".repeat(32)), &silent).await.unwrap_err();

        assert_eq!(err.to_string(), "Sublate update failed: release 0.4.0 does not match its checksum");
        assert!(fixture.staging.is_dir());
        assert_eq!(fixture.staged_entries(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn an_archive_that_does_not_hold_the_bundle_alone_is_rejected() {
        let layouts: [(&str, fn(&Path)); 4] = [
            ("another app", |top| fake_bundle(&top.join("Other.app"), "x")),
            ("an extra item", |top| {
                fake_bundle(&top.join(APP_BUNDLE), "x");
                std::fs::write(top.join("extra"), "x").unwrap();
            }),
            ("a symlink to a bundle elsewhere", |top| {
                let elsewhere = top.parent().unwrap().join("elsewhere.app");
                fake_bundle(&elsewhere, "x");
                symlink(&elsewhere, top.join(APP_BUNDLE)).unwrap();
            }),
            ("a plain file", |top| std::fs::write(top.join(APP_BUNDLE), "x").unwrap()),
        ];
        for (layout, fill) in layouts {
            let fixture = Fixture::new();
            let archive = fixture.archive(fill);
            let sha = sha_of(&archive).await;
            let (downloader, _) = fixture.downloader(Some(archive));

            let err = downloader.download("0.4.0", Some(&sha), &silent).await.unwrap_err();

            assert_eq!(
                err.to_string(),
                "Sublate update failed: Sublate-0.4.0.zip does not hold Sublate.app alone",
                "{layout}"
            );
            assert_eq!(fixture.staged_entries(), Vec::<String>::new(), "{layout}");
        }
    }

    #[tokio::test]
    async fn a_release_without_an_archive_or_a_bad_version_is_refused_before_downloading() {
        let fixture = Fixture::new();
        let (downloader, requested) = fixture.downloader(Some(fixture.app_archive()));

        let no_archive = downloader.download("0.4.0", None, &silent).await.unwrap_err();
        let bad_version = downloader.download("../evil", Some("abcd"), &silent).await.unwrap_err();

        assert_eq!(no_archive.to_string(), "Sublate update failed: release 0.4.0 has no Sublate-0.4.0.zip");
        assert_eq!(bad_version.to_string(), "Sublate update failed: '../evil' is not a Sublate version");
        assert!(requested.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failed_download_leaves_the_staging_folder_empty() {
        let fixture = Fixture::new();
        let (downloader, _) = fixture.downloader(None);

        let err = downloader.download("0.4.0", Some(&"00".repeat(32)), &silent).await.unwrap_err();

        assert_eq!(err.to_string(), "Sublate update failed: I/O error: connection reset");
        assert!(fixture.staging.is_dir());
        assert_eq!(fixture.staged_entries(), Vec::<String>::new());
    }
}
