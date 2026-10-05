use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use super::{Release, ReleaseFeed, Version};
use crate::error::{AppError, AppResult};

const LATEST_URL: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
/// The onedir build: it starts in a fraction of a second, where the single-file one unpacks
/// itself on every launch.
const ASSET: &str = "yt-dlp_macos.zip";
const DIGEST_PREFIX: &str = "sha256:";
const USER_AGENT: &str = concat!("sublate/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// yt-dlp's official GitHub releases.
pub struct GitHubReleases {
    http: reqwest::Client,
}

impl GitHubReleases {
    pub fn new() -> AppResult<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .map_err(failure)?;
        Ok(Self { http })
    }

    async fn get(&self, url: &str) -> AppResult<reqwest::Response> {
        let response = self.http.get(url).send().await.map_err(failure)?;
        response.error_for_status().map_err(failure)
    }
}

#[async_trait]
impl ReleaseFeed for GitHubReleases {
    async fn latest(&self) -> AppResult<Release> {
        parse_release(&self.get(LATEST_URL).await?.text().await.map_err(failure)?)
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
    let version = Version::parse(&release.tag_name)?;
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
}
