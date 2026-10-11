use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::StatusCode;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use super::installs::ytdlp_version;
use super::{AppRelease, AppReleaseFeed, Release, ReleaseFeed, Version};
use crate::error::{AppError, AppResult};

const YTDLP_LATEST_URL: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
/// The onedir build: it starts in a fraction of a second, where the single-file one unpacks
/// itself on every launch.
const ASSET: &str = "yt-dlp_macos.zip";
const DIGEST_PREFIX: &str = "sha256:";
const USER_AGENT: &str = concat!("sublate/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

const APP_LATEST_URL: &str = "https://api.github.com/repos/pingeplin/sublate/releases/latest";
const APP_RELEASE_PAGE: &str = "https://github.com/pingeplin/sublate/releases/tag/";
/// Sublate's releases are tagged `v<version>`, such as `v0.2.0`.
const APP_TAG_PREFIX: char = 'v';
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

fn client(timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(timeout)
        .build()
}

/// yt-dlp's official GitHub releases.
pub struct GitHubReleases {
    http: reqwest::Client,
}

impl GitHubReleases {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            http: client(DOWNLOAD_TIMEOUT).map_err(failure)?,
        })
    }

    async fn get(&self, url: &str) -> AppResult<reqwest::Response> {
        let response = self.http.get(url).send().await.map_err(failure)?;
        response.error_for_status().map_err(failure)
    }
}

#[async_trait]
impl ReleaseFeed for GitHubReleases {
    async fn latest(&self) -> AppResult<Release> {
        parse_release(&self.get(YTDLP_LATEST_URL).await?.text().await.map_err(failure)?)
    }

    async fn download(&self, release: &Release, dest: &Path) -> AppResult<()> {
        let mut response = self.get(&release.url).await?;
        let mut file = tokio::fs::File::create(dest).await?;
        while let Some(chunk) = response.chunk().await.map_err(failure)? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct ReleaseDto {
    tag_name: String,
    assets: Vec<AssetDto>,
}

#[derive(Deserialize)]
struct AssetDto {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

fn parse_release(json: &str) -> AppResult<Release> {
    let release: ReleaseDto = serde_json::from_str(json)?;
    let version = ytdlp_version(&release.tag_name)?;
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == ASSET)
        .ok_or_else(|| AppError::Update(format!("release {version} has no {ASSET}")))?;
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix(DIGEST_PREFIX))
        .ok_or_else(|| AppError::Update(format!("release {version} publishes no sha256 for {ASSET}")))?;
    Ok(Release {
        version,
        url: asset.browser_download_url,
        sha256: sha256.to_string(),
    })
}

fn failure(error: reqwest::Error) -> AppError {
    AppError::Update(error.to_string())
}

/// Sublate's own releases on GitHub. Drafts and prereleases are never the latest one.
pub struct GitHubAppReleases {
    http: reqwest::Client,
}

impl GitHubAppReleases {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            http: client(CHECK_TIMEOUT).map_err(check_failure)?,
        })
    }
}

#[async_trait]
impl AppReleaseFeed for GitHubAppReleases {
    async fn latest(&self) -> AppResult<AppRelease> {
        let response = self.http.get(APP_LATEST_URL).send().await.map_err(check_failure)?;
        // GitHub answers the same for a repository without releases and for one it cannot show.
        if response.status() == StatusCode::NOT_FOUND {
            return Err(AppError::AppUpdate("no published release was found".into()));
        }
        let response = response.error_for_status().map_err(check_failure)?;
        parse_app_release(&response.text().await.map_err(check_failure)?)
    }
}

#[derive(Deserialize)]
struct AppReleaseDto {
    tag_name: String,
}

/// The page is derived from the checked version, so nothing the feed says is opened as it is.
fn parse_app_release(json: &str) -> AppResult<AppRelease> {
    let AppReleaseDto { tag_name } = serde_json::from_str(json)?;
    let version = tag_name
        .strip_prefix(APP_TAG_PREFIX)
        .and_then(Version::parse)
        .ok_or_else(|| AppError::AppUpdate(format!("release '{tag_name}' is not tagged v<version>")))?;
    Ok(AppRelease {
        page: format!("{APP_RELEASE_PAGE}{APP_TAG_PREFIX}{version}"),
        version,
    })
}

fn check_failure(error: reqwest::Error) -> AppError {
    AppError::AppUpdate(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(assets: &str) -> String {
        format!(r#"{{"tag_name": "2026.08.19", "prerelease": false, "assets": [{assets}]}}"#)
    }

    const ONEFILE: &str = r#"{"name": "yt-dlp_macos", "digest": "sha256:0f19",
        "browser_download_url": "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos"}"#;

    #[test]
    fn picks_the_onedir_archive_and_its_digest() {
        let onedir = r#"{"name": "yt-dlp_macos.zip", "digest": "sha256:07e5", "size": 53923637,
            "browser_download_url": "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos.zip"}"#;
        let release = parse_release(&release_json(&format!("{ONEFILE}, {onedir}"))).unwrap();
        assert_eq!(
            release,
            Release {
                version: Version::parse("2026.08.19").unwrap(),
                url: "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp_macos.zip".into(),
                sha256: "07e5".into(),
            }
        );
    }

    #[test]
    fn a_release_without_the_archive_or_its_digest_is_unusable() {
        let no_archive = parse_release(&release_json(ONEFILE)).unwrap_err();
        assert_eq!(no_archive.to_string(), "yt-dlp update failed: release 2026.08.19 has no yt-dlp_macos.zip");

        let no_digest = r#"{"name": "yt-dlp_macos.zip", "digest": null, "browser_download_url": "https://x"}"#;
        let err = parse_release(&release_json(no_digest)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "yt-dlp update failed: release 2026.08.19 publishes no sha256 for yt-dlp_macos.zip"
        );
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_rejected() {
        let json = r#"{"tag_name": "../evil", "assets": []}"#;
        assert!(matches!(parse_release(json), Err(AppError::Update(_))));
    }

    fn app_release_json(tag: &str) -> String {
        format!(r#"{{"tag_name": "{tag}", "html_url": "https://example.com/elsewhere", "assets": []}}"#)
    }

    #[test]
    fn an_app_release_is_read_from_its_tag_and_links_to_its_own_page() {
        assert_eq!(
            parse_app_release(&app_release_json("v0.2.0")).unwrap(),
            AppRelease {
                version: Version::parse("0.2.0").unwrap(),
                page: "https://github.com/pingeplin/sublate/releases/tag/v0.2.0".into(),
            }
        );
    }

    #[test]
    fn an_app_tag_outside_the_convention_is_rejected() {
        for tag in ["0.2.0", "v", "vnext", "V0.2.0", "v0.2.0-beta", "v../evil", "nightly"] {
            let err = parse_app_release(&app_release_json(tag)).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("Sublate update check failed: release '{tag}' is not tagged v<version>")
            );
        }
    }
}
