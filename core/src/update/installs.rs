use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use crate::error::{AppError, AppResult};

/// Executable at the top of yt-dlp's macOS onedir build; `_internal/` sits next to it.
pub const PROGRAM: &str = "yt-dlp_macos";
const UNZIP: &str = "/usr/bin/ditto";
const STAGING_PREFIX: &str = ".staging-";
/// A fresh install is slow to start once, while macOS scans its libraries.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(120);

/// A yt-dlp release tag such as `2026.08.19` or `2026.08.19.1`, ordered numerically.
#[derive(Debug, Clone)]
pub struct Version(String);

impl Version {
    /// Only digits and dots are accepted, so a tag is always safe to use as a directory name.
    pub fn parse(text: &str) -> AppResult<Self> {
        let tag = text.trim();
        if tag.split('.').all(|part| part.parse::<u32>().is_ok()) {
            Ok(Self(tag.to_string()))
        } else {
            Err(AppError::Update(format!("'{tag}' is not a yt-dlp version")))
        }
    }

    fn parts(&self) -> Vec<u32> {
        self.0.split('.').flat_map(str::parse).collect()
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parts() == other.parts()
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.parts().cmp(&other.parts())
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub version: Version,
    pub program: PathBuf,
}

impl Install {
    pub fn at(version: Version, dir: &Path) -> Self {
        Self {
            version,
            program: dir.join(PROGRAM),
        }
    }
}

/// yt-dlp releases installed after the app shipped, one directory per version.
pub struct Installs {
    root: PathBuf,
}

impl Installs {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn newest(&self) -> Option<Install> {
        let entries = std::fs::read_dir(&self.root).ok()?;
        entries
            .flatten()
            .filter_map(|entry| Version::parse(&entry.file_name().to_string_lossy()).ok())
            .filter_map(|version| self.find(&version))
            .max_by(|a, b| a.version.cmp(&b.version))
    }

    pub fn find(&self, version: &Version) -> Option<Install> {
        let install = Install::at(version.clone(), &self.root.join(version.to_string()));
        install.program.is_file().then_some(install)
    }

    /// Unpacks a release archive and keeps it only if it starts and reports `version`.
    pub async fn add(&self, version: &Version, archive: &Path) -> AppResult<Install> {
        let staging = self.root.join(format!("{STAGING_PREFIX}{version}"));
        let target = self.root.join(version.to_string());
        remove(&staging);
        tokio::fs::create_dir_all(&self.root).await?;

        let staged = stage(archive, &staging, version).await;
        if staged.is_err() {
            remove(&staging);
        }
        staged?;
        remove(&target);
        tokio::fs::rename(&staging, &target).await?;
        Ok(Install::at(version.clone(), &target))
    }

    /// Deletes every other entry, including leftovers of an interrupted install.
    pub fn retain(&self, keep: Option<&Version>) {
        let keep = keep.map(Version::to_string);
        let Ok(entries) = std::fs::read_dir(&self.root) else { return };
        for entry in entries.flatten() {
            if Some(entry.file_name().to_string_lossy().as_ref()) != keep.as_deref() {
                remove(&entry.path());
            }
        }
    }
}

fn remove(path: &Path) {
    let _ = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
}

async fn stage(archive: &Path, staging: &Path, version: &Version) -> AppResult<()> {
    unpack(archive, staging).await?;
    let reported = reported_version(&staging.join(PROGRAM)).await?;
    if reported == *version {
        Ok(())
    } else {
        Err(AppError::Update(format!("release {version} reports itself as {reported}")))
    }
}

async fn unpack(archive: &Path, dest: &Path) -> AppResult<()> {
    let output = tokio::process::Command::new(UNZIP)
        .args(["-x", "-k"])
        .arg(archive)
        .arg(dest)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| AppError::Update(format!("cannot start {UNZIP}: {e}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Update(format!(
            "cannot unpack the release: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

async fn reported_version(program: &Path) -> AppResult<Version> {
    let run = tokio::process::Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(LAUNCH_TIMEOUT, run)
        .await
        .map_err(|_| AppError::Update("the release did not start".into()))?
        .map_err(|e| AppError::Update(format!("the release cannot run: {e}")))?;
    if output.status.success() {
        Version::parse(&String::from_utf8_lossy(&output.stdout))
    } else {
        Err(AppError::Update(format!(
            "the release failed to start: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use super::PROGRAM;

    /// A stand-in release directory whose program prints `reports` for `--version`.
    pub fn fake_release(dir: &Path, reports: &str) {
        std::fs::create_dir_all(dir.join("_internal")).unwrap();
        let program = dir.join(PROGRAM);
        std::fs::write(&program, format!("#!/bin/sh\necho {reports}\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    pub fn fake_archive(dir: &Path, reports: &str) -> PathBuf {
        let content = dir.join("content");
        fake_release(&content, reports);
        let archive = dir.join("release.zip");
        let status = std::process::Command::new(super::UNZIP)
            .args(["-c", "-k"])
            .arg(&content)
            .arg(&archive)
            .status()
            .unwrap();
        assert!(status.success());
        archive
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{fake_archive, fake_release};
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn versions_order_numerically_not_lexically() {
        assert!(version("2026.08.19") < version("2026.10.01"));
        assert!(version("2026.08.19") < version("2026.08.19.1"));
        assert!(version("2026.08.19.9") < version("2026.08.19.10"));
        assert_eq!(version(" 2026.08.19\n"), version("2026.08.19"));
    }

    #[test]
    fn versions_print_as_upstream_tags() {
        assert_eq!(version("2026.08.09\n").to_string(), "2026.08.09");
        assert_eq!(version("2026.08.19.012345").to_string(), "2026.08.19.012345");
    }

    #[test]
    fn tags_that_could_escape_the_directory_are_rejected() {
        for tag in ["", "../2026", "2026.08.19/..", "v2026.08.19", "2026..19", "nightly"] {
            assert!(Version::parse(tag).is_err(), "{tag}");
        }
    }

    #[test]
    fn newest_ignores_incomplete_and_foreign_entries() {
        let dir = tempfile::tempdir().unwrap();
        let installs = Installs::new(dir.path().join("yt-dlp"));
        assert_eq!(installs.newest(), None);

        fake_release(&installs.root().join("2026.08.19"), "2026.08.19");
        fake_release(&installs.root().join("2026.09.02"), "2026.09.02");
        fake_release(&installs.root().join(".staging-2026.12.01"), "2026.12.01");
        std::fs::create_dir_all(installs.root().join("2026.11.11")).unwrap();

        let newest = installs.newest().unwrap();
        assert_eq!(newest.version, version("2026.09.02"));
        assert_eq!(newest.program, installs.root().join("2026.09.02").join(PROGRAM));
    }

    #[tokio::test]
    async fn add_installs_a_release_that_reports_the_expected_version() {
        let dir = tempfile::tempdir().unwrap();
        let installs = Installs::new(dir.path().join("yt-dlp"));
        let archive = fake_archive(dir.path(), "2026.09.02");

        let install = installs.add(&version("2026.09.02"), &archive).await.unwrap();

        assert_eq!(installs.newest(), Some(install.clone()));
        assert!(install.program.is_file());
        assert!(installs.root().join("2026.09.02/_internal").is_dir());
    }

    #[tokio::test]
    async fn add_rejects_a_release_that_reports_another_version() {
        let dir = tempfile::tempdir().unwrap();
        let installs = Installs::new(dir.path().join("yt-dlp"));
        let archive = fake_archive(dir.path(), "2026.01.01");

        let err = installs.add(&version("2026.09.02"), &archive).await.unwrap_err();

        assert_eq!(err.to_string(), "yt-dlp update failed: release 2026.09.02 reports itself as 2026.01.01");
        assert_eq!(std::fs::read_dir(installs.root()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn add_rejects_an_archive_that_is_not_a_release() {
        let dir = tempfile::tempdir().unwrap();
        let installs = Installs::new(dir.path().join("yt-dlp"));
        let archive = dir.path().join("broken.zip");
        std::fs::write(&archive, "not a zip").unwrap();

        assert!(installs.add(&version("2026.09.02"), &archive).await.is_err());
        assert_eq!(installs.newest(), None);
    }

    #[test]
    fn retain_keeps_only_the_named_version() {
        let dir = tempfile::tempdir().unwrap();
        let installs = Installs::new(dir.path().to_path_buf());
        fake_release(&dir.path().join("2026.08.19"), "2026.08.19");
        fake_release(&dir.path().join("2026.09.02"), "2026.09.02");
        std::fs::write(dir.path().join(".download.zip"), "partial").unwrap();

        installs.retain(Some(&version("2026.09.02")));
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["2026.09.02"]);

        installs.retain(None);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
