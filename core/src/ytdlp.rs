use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, RwLock};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::error::{AppError, AppResult};
use crate::file_name::OutputLocation;
use crate::metadata::{parse_metadata, SubtitleTrack, TrackKind, VideoMetadata};
use crate::toolchain::Toolchain;
use crate::update::{Install, Version};

/// What Finder-launched apps get; fixed so tools on the user's own PATH never take part.
const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const STDERR_TAIL_LINES: usize = 8;
const PROGRESS_TAG: &str = "CT_PROGRESS";
const PATH_TAG: &str = "CT_PATH";

pub struct YtDlp {
    /// Replaced when the updater installs a newer release.
    install: RwLock<Install>,
    /// Options every invocation starts with.
    base_args: Vec<String>,
    deno_dir: PathBuf,
    /// `-J` output of the last fetched video, keyed by its page URL.
    last_info: Mutex<Option<(String, Arc<str>)>>,
}

/// Where yt-dlp reads the video from.
#[derive(Clone, Copy)]
enum Source<'a> {
    Url(&'a str),
    /// Info JSON from an earlier `-J`, fed through stdin; skips re-extracting the page.
    InfoJson,
}

impl YtDlp {
    /// Everything yt-dlp reads or runs is pinned to the bundle and `cache_dir`: the user's
    /// yt-dlp config, ffmpeg and JS runtime are never picked up.
    pub fn new(tools: Toolchain, cache_dir: &Path) -> Self {
        let text = |path: &Path| path.to_string_lossy().into_owned();
        #[rustfmt::skip]
        let base_args = vec![
            "--ignore-config".into(), "--no-playlist".into(),
            "--ffmpeg-location".into(), text(&tools.ffmpeg_dir),
            "--js-runtimes".into(), format!("deno:{}", text(&tools.deno)),
            "--cache-dir".into(), text(&cache_dir.join("yt-dlp")),
        ];
        Self {
            install: RwLock::new(tools.ytdlp),
            base_args,
            deno_dir: cache_dir.join("deno"),
            last_info: Mutex::new(None),
        }
    }

    pub fn version(&self) -> Version {
        self.install().version
    }

    pub fn switch_to(&self, install: Install) {
        *self.install.write().expect("install lock") = install;
    }

    fn install(&self) -> Install {
        self.install.read().expect("install lock").clone()
    }

    pub async fn fetch_metadata(&self, url: &str) -> AppResult<VideoMetadata> {
        let args = self.args(&["-J", "--flat-playlist"], Source::Url(url));
        let stdout = self.run(&args, None, None, |_| {}).await?;
        let metadata = parse_metadata(&stdout, url)?;
        *self.last_info.lock().expect("info cache lock") = Some((metadata.url.clone(), stdout.into()));
        Ok(metadata)
    }

    pub async fn download_video(
        &self,
        url: &str,
        out: &OutputLocation,
        on_progress: impl Fn(f32),
    ) -> AppResult<PathBuf> {
        let progress = format!("download:{PROGRESS_TAG} %(progress._percent)s");
        let path = format!("after_move:{PATH_TAG} %(filepath)s");
        // Always an mp4 carrying the thumbnail as cover art, so Quick Look (Finder, open
        // dialogs) previews it and players show the cover. mp4 is forced because yt-dlp's
        // `mp4/mkv` fallback picks mkv whenever the audio is Opus. Streams are copied at best
        // quality rather than selected for H.264 (yt-dlp's `-t mp4` preset, capped near
        // 1080p), so AV1/VP9 videos still need VLC or IINA instead of QuickTime.
        #[rustfmt::skip]
        let flags = [
            "--newline", "--progress", "--progress-template", &progress, "--print", &path,
            "--merge-output-format", "mp4", "--remux-video", "mp4",
            "--embed-thumbnail", "--convert-thumbnails", "jpg",
        ];
        let stdout = self
            .download(url, out, &flags, |line| {
                if let Some(percent) = tagged(line, PROGRESS_TAG).and_then(|v| v.parse().ok()) {
                    on_progress(percent);
                }
            })
            .await?;
        stdout
            .lines()
            .find_map(|line| tagged(line, PATH_TAG))
            .map(PathBuf::from)
            .ok_or_else(|| AppError::YtDlp("video path not reported".into()))
    }

    pub async fn download_subtitle(
        &self,
        url: &str,
        out: &OutputLocation,
        track: &SubtitleTrack,
    ) -> AppResult<PathBuf> {
        let flags = ["--skip-download", write_flag(track.kind), "--sub-langs", &track.code, "--convert-subs", "srt"];
        self.download(url, out, &flags, |_| {}).await?;
        let path = out.subtitle(&track.code);
        if path.is_file() {
            Ok(path)
        } else {
            Err(AppError::YtDlp(format!(
                "subtitle '{}' was not produced at {}",
                track.code,
                path.display()
            )))
        }
    }

    /// Runs inside the output directory with a relative template, so the directory path is
    /// never subject to yt-dlp's template or environment-variable expansion. Reuses the
    /// fetched info JSON when available; its stream URLs expire after a few hours, so a
    /// failure falls back to extracting from the page again.
    async fn download(
        &self,
        url: &str,
        out: &OutputLocation,
        flags: &[&str],
        on_line: impl Fn(&str),
    ) -> AppResult<String> {
        tokio::fs::create_dir_all(out.dir()).await?;
        let template = out.ytdlp_template();
        let flags = [flags, &["-o", &template]].concat();
        if let Some(info) = self.cached_info(url) {
            let args = self.args(&flags, Source::InfoJson);
            if let Ok(stdout) = self.run(&args, Some(info), Some(out.dir()), &on_line).await {
                return Ok(stdout);
            }
        }
        let args = self.args(&flags, Source::Url(url));
        self.run(&args, None, Some(out.dir()), &on_line).await
    }

    fn cached_info(&self, url: &str) -> Option<Arc<str>> {
        let cache = self.last_info.lock().expect("info cache lock");
        cache
            .as_ref()
            .filter(|(cached_url, _)| cached_url == url)
            .map(|(_, info)| Arc::clone(info))
    }

    fn args(&self, flags: &[&str], source: Source) -> Vec<String> {
        // `--` keeps a URL that starts with `-` from being read as an option.
        let input: &[&str] = match source {
            Source::Url(url) => &["--", url],
            Source::InfoJson => &["--load-info-json", "-"],
        };
        let rest = flags.iter().chain(input).map(|s| s.to_string());
        self.base_args.iter().cloned().chain(rest).collect()
    }

    async fn run(
        &self,
        args: &[String],
        stdin: Option<Arc<str>>,
        cwd: Option<&Path>,
        on_line: impl Fn(&str),
    ) -> AppResult<String> {
        let program = self.install().program;
        let mut command = tokio::process::Command::new(&program);
        command
            .args(args)
            .env("PATH", SYSTEM_PATH)
            .env("DENO_DIR", &self.deno_dir)
            .kill_on_drop(true)
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let mut child = command
            .spawn()
            .map_err(|e| AppError::YtDlp(format!("cannot start {}: {e}", program.display())))?;

        if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
            tokio::spawn(async move {
                let _ = pipe.write_all(input.as_bytes()).await;
            });
        }
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        });

        let mut stdout = String::new();
        let mut lines = BufReader::new(child.stdout.take().expect("stdout is piped")).lines();
        while let Some(line) = lines.next_line().await? {
            on_line(&line);
            stdout.push_str(&line);
            stdout.push('\n');
        }

        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        if status.success() {
            Ok(stdout)
        } else {
            Err(AppError::YtDlp(tail(&stderr, STDERR_TAIL_LINES)))
        }
    }
}

fn write_flag(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Manual => "--write-subs",
        TrackKind::Auto => "--write-auto-subs",
    }
}

/// Our `--print`/`--progress-template` lines are prefixed with a tag and a space.
fn tagged<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    line.strip_prefix(tag)?.strip_prefix(' ')
}

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ytdlp() -> YtDlp {
        let tools = Toolchain {
            ytdlp: Install::at(Version::parse("2026.08.19").unwrap(), Path::new("/tools/yt-dlp")),
            ffmpeg_dir: "/tools".into(),
            deno: "/tools/deno".into(),
        };
        YtDlp::new(tools, Path::new("/cache"))
    }

    #[rustfmt::skip]
    const BASE: [&str; 8] = [
        "--ignore-config", "--no-playlist",
        "--ffmpeg-location", "/tools",
        "--js-runtimes", "deno:/tools/deno",
        "--cache-dir", "/cache/yt-dlp",
    ];

    fn with_base(rest: &[&'static str]) -> Vec<&'static str> {
        [&BASE[..], rest].concat()
    }

    #[test]
    fn reads_only_tagged_lines() {
        assert_eq!(tagged("CT_PROGRESS 12.6", PROGRESS_TAG), Some("12.6"));
        assert_eq!(tagged("CT_PATH /o/T.webm", PATH_TAG), Some("/o/T.webm"));
        assert_eq!(tagged("[download] 12.6% of 1MiB", PROGRESS_TAG), None);
        assert_eq!(tagged("CT_PATHX /o", PATH_TAG), None);
    }

    #[test]
    fn every_invocation_is_pinned_to_the_bundled_tools() {
        assert_eq!(ytdlp().args(&[], Source::InfoJson)[..BASE.len()], BASE);
    }

    #[test]
    fn url_follows_end_of_options_marker() {
        assert_eq!(
            ytdlp().args(&["-J"], Source::Url("--exec=x")),
            with_base(&["-J", "--", "--exec=x"])
        );
    }

    #[test]
    fn cached_info_is_read_from_stdin_without_url() {
        assert_eq!(
            ytdlp().args(&["--skip-download"], Source::InfoJson),
            with_base(&["--skip-download", "--load-info-json", "-"])
        );
    }

    #[test]
    fn switching_installs_changes_the_reported_version() {
        let ytdlp = ytdlp();
        let newer = Install::at(Version::parse("2026.09.02").unwrap(), Path::new("/support/2026.09.02"));
        ytdlp.switch_to(newer.clone());
        assert_eq!(ytdlp.version(), newer.version);
        assert_eq!(ytdlp.install().program, newer.program);
    }

    #[test]
    fn info_cache_only_serves_the_same_page() {
        let ytdlp = ytdlp();
        *ytdlp.last_info.lock().unwrap() = Some(("https://a".into(), "{}".into()));
        assert!(ytdlp.cached_info("https://a").is_some());
        assert!(ytdlp.cached_info("https://b").is_none());
    }

    #[test]
    fn auto_tracks_need_the_auto_subs_flag() {
        assert_eq!(write_flag(TrackKind::Auto), "--write-auto-subs");
        assert_eq!(write_flag(TrackKind::Manual), "--write-subs");
    }

    #[test]
    fn tail_keeps_last_non_blank_lines() {
        assert_eq!(tail("a\n\nb\nc\n", 2), "b\nc");
    }
}
