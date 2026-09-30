use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::error::{AppError, AppResult};
use crate::file_name::OutputLocation;
use crate::metadata::{parse_metadata, SubtitleTrack, TrackKind, VideoMetadata};
use crate::process_env::ProcessEnv;

const PROGRAM: &str = "yt-dlp";
const STDERR_TAIL_LINES: usize = 8;
const PROGRESS_TAG: &str = "CT_PROGRESS";
const PATH_TAG: &str = "CT_PATH";

pub struct YtDlp {
    env: ProcessEnv,
    use_node: bool,
}

impl YtDlp {
    /// YouTube extraction needs a JS runtime; yt-dlp only enables deno by default.
    pub fn new(env: ProcessEnv) -> Self {
        let use_node = env.resolve("deno").is_none() && env.resolve("node").is_some();
        Self { env, use_node }
    }

    pub async fn fetch_metadata(&self, url: &str) -> AppResult<VideoMetadata> {
        let stdout = self.run(&self.metadata_args(url), |_| {}).await?;
        Ok(parse_metadata(&stdout, url)?)
    }

    pub async fn download_video(
        &self,
        url: &str,
        out: &OutputLocation,
        on_progress: impl Fn(f32),
    ) -> AppResult<PathBuf> {
        let stdout = self
            .run(&self.video_args(url, out), |line| {
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
        self.run(&self.subtitle_args(url, out, track), |_| {}).await?;
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

    fn metadata_args(&self, url: &str) -> Vec<String> {
        self.args(&["-J"], url)
    }

    fn video_args(&self, url: &str, out: &OutputLocation) -> Vec<String> {
        let progress = format!("download:{PROGRESS_TAG} %(progress._percent)s");
        let path = format!("after_move:{PATH_TAG} %(filepath)s");
        let template = out.ytdlp_template();
        self.args(
            &["--newline", "--progress", "--progress-template", &progress, "--print", &path, "-o", &template],
            url,
        )
    }

    fn subtitle_args(&self, url: &str, out: &OutputLocation, track: &SubtitleTrack) -> Vec<String> {
        let write_flag = match track.kind {
            TrackKind::Manual => "--write-subs",
            TrackKind::Auto => "--write-auto-subs",
        };
        let template = out.ytdlp_template();
        self.args(
            &["--skip-download", write_flag, "--sub-langs", &track.code, "--convert-subs", "srt", "-o", &template],
            url,
        )
    }

    fn args(&self, flags: &[&str], url: &str) -> Vec<String> {
        let runtime: &[&str] = if self.use_node { &["--js-runtimes", "node"] } else { &[] };
        ["--no-playlist"]
            .iter()
            .chain(runtime)
            .chain(flags)
            .chain([&url])
            .map(|s| s.to_string())
            .collect()
    }

    async fn run(&self, args: &[String], on_line: impl Fn(&str)) -> AppResult<String> {
        let mut child = self
            .env
            .command(PROGRAM)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::YtDlp(format!("cannot start {PROGRAM}: {e}")))?;

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

    fn ytdlp(use_node: bool) -> YtDlp {
        YtDlp {
            env: ProcessEnv::default(),
            use_node,
        }
    }

    fn out() -> OutputLocation {
        OutputLocation::new("/o", "T", "id")
    }

    fn track(code: &str, kind: TrackKind) -> SubtitleTrack {
        SubtitleTrack { code: code.into(), name: code.into(), kind }
    }

    #[test]
    fn reads_only_tagged_lines() {
        assert_eq!(tagged("CT_PROGRESS 12.6", PROGRESS_TAG), Some("12.6"));
        assert_eq!(tagged("CT_PATH /o/T.webm", PATH_TAG), Some("/o/T.webm"));
        assert_eq!(tagged("[download] 12.6% of 1MiB", PROGRESS_TAG), None);
        assert_eq!(tagged("CT_PATHX /o", PATH_TAG), None);
    }

    #[test]
    fn metadata_args_include_node_runtime_when_selected() {
        assert_eq!(
            ytdlp(true).metadata_args("U"),
            ["--no-playlist", "--js-runtimes", "node", "-J", "U"]
        );
        assert_eq!(ytdlp(false).metadata_args("U"), ["--no-playlist", "-J", "U"]);
    }

    #[test]
    fn video_args_tag_machine_output_and_never_embed_subtitles() {
        let args = ytdlp(false).video_args("U", &OutputLocation::new("/out", "Title", "id"));
        assert_eq!(
            args,
            [
                "--no-playlist", "--newline", "--progress",
                "--progress-template", "download:CT_PROGRESS %(progress._percent)s",
                "--print", "after_move:CT_PATH %(filepath)s",
                "-o", "/out/Title.%(ext)s", "U",
            ]
        );
    }

    #[test]
    fn subtitle_args_pick_flag_by_track_kind() {
        let auto = ytdlp(false).subtitle_args("U", &out(), &track("ko-orig", TrackKind::Auto));
        assert!(auto.contains(&"--write-auto-subs".to_string()));
        assert!(auto.windows(2).any(|w| w == ["--sub-langs", "ko-orig"]));
        assert!(auto.contains(&"--skip-download".to_string()));

        let manual = ytdlp(false).subtitle_args("U", &out(), &track("en", TrackKind::Manual));
        assert!(manual.contains(&"--write-subs".to_string()));
        assert!(!manual.contains(&"--write-auto-subs".to_string()));
    }

    #[test]
    fn tail_keeps_last_non_blank_lines() {
        assert_eq!(tail("a\n\nb\nc\n", 2), "b\nc");
    }
}
