use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// The tools shipped inside the app bundle, so nothing depends on what the user has installed.
/// yt-dlp is not among them: the app downloads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    /// Holds both `ffmpeg` and `ffprobe`.
    pub ffmpeg_dir: PathBuf,
    pub deno: PathBuf,
}

impl Toolchain {
    /// Reads the layout written by `vendor/fetch.sh`.
    pub fn bundled(dir: &Path) -> AppResult<Self> {
        let toolchain = Self {
            ffmpeg_dir: dir.to_path_buf(),
            deno: dir.join("deno"),
        };
        let required = [toolchain.deno.clone(), dir.join("ffmpeg"), dir.join("ffprobe")];
        match required.iter().find(|path| !path.is_file()) {
            Some(missing) => Err(AppError::Toolchain(format!("{} is missing", missing.display()))),
            None => Ok(toolchain),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            std::fs::write(dir.path().join(file), "").unwrap();
        }
        dir
    }

    #[test]
    fn reads_the_vendored_layout() {
        let dir = layout(&["deno", "ffmpeg", "ffprobe"]);
        let toolchain = Toolchain::bundled(dir.path()).unwrap();
        assert_eq!(toolchain.ffmpeg_dir, dir.path());
        assert_eq!(toolchain.deno, dir.path().join("deno"));
    }

    #[test]
    fn names_the_missing_tool() {
        let dir = layout(&["deno", "ffmpeg"]);
        let err = Toolchain::bundled(dir.path()).unwrap_err().to_string();
        assert!(err.ends_with("ffprobe is missing"), "{err}");
    }

    #[test]
    fn an_empty_directory_is_not_a_toolchain() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(Toolchain::bundled(dir.path()), Err(AppError::Toolchain(_))));
    }
}
