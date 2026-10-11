use async_trait::async_trait;

use super::Version;
use crate::error::{AppError, AppResult};

/// A published version of the app itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRelease {
    pub version: Version,
    /// The page the user downloads it from.
    pub page: String,
}

#[async_trait]
pub trait AppReleaseFeed: Send + Sync {
    async fn latest(&self) -> AppResult<AppRelease>;
}

/// The latest release when it is newer than `current`, the version that is running. The app
/// only reports it: installing stays with the user.
pub async fn newer_release(feed: &dyn AppReleaseFeed, current: &str) -> AppResult<Option<AppRelease>> {
    let current = parse_version(current)?;
    let latest = feed.latest().await?;
    Ok((latest.version > current).then_some(latest))
}

/// App versions are digits and dots, like the `MARKETING_VERSION` they come from.
fn parse_version(text: &str) -> AppResult<Version> {
    Version::parse(text).ok_or_else(|| AppError::AppUpdate(format!("'{}' is not a Sublate version", text.trim())))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

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
    }

    fn release(version: &str) -> AppRelease {
        AppRelease {
            version: Version::parse(version).unwrap(),
            page: format!("https://example.com/v{version}"),
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
}
