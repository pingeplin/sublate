use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::update::{Install, Version};

const VERSION_FILE: &str = "yt-dlp.version";
const YTDLP_DIR: &str = "yt-dlp";

/// The tools shipped inside the app bundle, so nothing depends on what the user has installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    pub ytdlp: Install,
    /// Holds both `ffmpeg` and `ffprobe`.
    pub ffmpeg_dir: PathBuf,
    pub deno: PathBuf,
}

impl Toolchain {
    /// Reads the layout written by `vendor/fetch.sh`.
    pub fn bundled(dir: &Path) -> AppResult<Self> {
        let version = std::fs::read_to_string(dir.join(VERSION_FILE))
            .map_err(|e| AppError::Toolchain(format!("{VERSION_FILE} is unreadable in {}: {e}", dir.display())))?;
        let toolchain = Self {
            ytdlp: Install::at(Version::parse(&version)?, &dir.join(YTDLP_DIR)),
            ffmpeg_dir: dir.to_path_buf(),
            deno: dir.join("deno"),
        };
        let required = [
            toolchain.ytdlp.program.clone(),
            toolchain.deno.clone(),
            dir.join("ffmpeg"),
            dir.join("ffprobe"),
        ];
        match required.iter().find(|path| !path.is_file()) {
            Some(missing) => Err(AppError::Toolchain(format!("{} is missing", missing.display()))),
            None => Ok(toolchain),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::PROGRAM;

    fn layout(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(YTDLP_DIR)).unwrap();
        for file in files {
            std::fs::write(dir.path().join(file), "2026.08.19\n").unwrap();
        }
        dir
    }

    #[test]
    fn reads_the_vendored_layout() {
        let ytdlp = format!("{YTDLP_DIR}/{PROGRAM}");
        let dir = layout(&[VERSION_FILE, &ytdlp, "deno", "ffmpeg", "ffprobe"]);
        let toolchain = Toolchain::bundled(dir.path()).unwrap();
        assert_eq!(toolchain.ytdlp.version, Version::parse("2026.08.19").unwrap());
        assert_eq!(toolchain.ytdlp.program, dir.path().join(ytdlp));
        assert_eq!(toolchain.ffmpeg_dir, dir.path());
        assert_eq!(toolchain.deno, dir.path().join("deno"));
    }

    #[test]
    fn names_the_missing_tool() {
        let ytdlp = format!("{YTDLP_DIR}/{PROGRAM}");
        let dir = layout(&[VERSION_FILE, &ytdlp, "deno", "ffmpeg"]);
        let err = Toolchain::bundled(dir.path()).unwrap_err().to_string();
        assert!(err.ends_with("ffprobe is missing"), "{err}");
    }

    #[test]
    fn an_empty_directory_is_not_a_toolchain() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(Toolchain::bundled(dir.path()), Err(AppError::Toolchain(_))));
    }
}
