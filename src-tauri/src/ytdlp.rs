use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::error::{AppError, AppResult};
use crate::metadata::{parse_metadata, SubtitleTrack, TrackKind, VideoMetadata};
use crate::process_env::ProcessEnv;

const PROGRAM: &str = "yt-dlp";
const OUTPUT_TEMPLATE: &str = "%(id)s.%(ext)s";
const STDERR_TAIL_LINES: usize = 8;

pub struct YtDlp {
    env: ProcessEnv,
    js_runtime: Option<String>,
}

impl YtDlp {
    pub fn new(env: ProcessEnv) -> Self {
        let js_runtime = detect_js_runtime(&env);
        Self { env, js_runtime }
    }

    pub async fn fetch_metadata(&self, url: &str) -> AppResult<VideoMetadata> {
        let stdout = self.run(&self.metadata_args(url), |_| {}).await?;
        Ok(parse_metadata(&stdout, url)?)
    }

    pub async fn download_video(
        &self,
        url: &str,
        out_dir: &Path,
        on_progress: impl Fn(f32),
    ) -> AppResult<PathBuf> {
        let stdout = self
            .run(&self.video_args(url, out_dir), |line| {
                if let Some(pct) = parse_progress(line) {
                    on_progress(pct);
                }
            })
            .await?;
        stdout
            .lines()
            .rev()
            .find(|l| !l.is_empty() && !l.starts_with('['))
            .map(PathBuf::from)
            .ok_or_else(|| AppError::YtDlp("video path not reported".into()))
    }

    pub async fn download_subtitle(
        &self,
        url: &str,
        video_id: &str,
        out_dir: &Path,
        track: &SubtitleTrack,
    ) -> AppResult<PathBuf> {
        self.run(&self.subtitle_args(url, out_dir, track), |_| {}).await?;
        let path = subtitle_path(out_dir, video_id, &track.code);
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

    fn base_args(&self) -> Vec<String> {
        let mut args = vec!["--no-playlist".to_string()];
        if let Some(runtime) = &self.js_runtime {
            args.extend(["--js-runtimes".into(), runtime.clone()]);
        }
        args
    }

    fn metadata_args(&self, url: &str) -> Vec<String> {
        let mut args = self.base_args();
        args.extend(["-J".into(), url.into()]);
        args
    }

    fn video_args(&self, url: &str, out_dir: &Path) -> Vec<String> {
        let mut args = self.base_args();
        args.extend(
            [
                "--newline",
                "--progress",
                "-S",
                "ext",
                "--print",
                "after_move:filepath",
                "-o",
            ]
            .map(String::from),
        );
        args.extend([output_template(out_dir), url.into()]);
        args
    }

    fn subtitle_args(&self, url: &str, out_dir: &Path, track: &SubtitleTrack) -> Vec<String> {
        let write_flag = match track.kind {
            TrackKind::Manual => "--write-subs",
            TrackKind::Auto => "--write-auto-subs",
        };
        let mut args = self.base_args();
        args.extend(
            ["--skip-download", write_flag, "--sub-langs", &track.code, "--convert-subs", "srt", "-o"]
                .map(String::from),
        );
        args.extend([output_template(out_dir), url.into()]);
        args
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

pub fn subtitle_path(out_dir: &Path, video_id: &str, lang: &str) -> PathBuf {
    out_dir.join(format!("{video_id}.{lang}.srt"))
}

fn output_template(out_dir: &Path) -> String {
    out_dir.join(OUTPUT_TEMPLATE).to_string_lossy().into_owned()
}

/// YouTube extraction needs a JS runtime; yt-dlp only enables deno by default.
fn detect_js_runtime(env: &ProcessEnv) -> Option<String> {
    if env.resolve("deno").is_some() {
        return None;
    }
    env.resolve("node").map(|_| "node".to_string())
}

fn parse_progress(line: &str) -> Option<f32> {
    let rest = line.strip_prefix("[download]")?.trim_start();
    let (pct, _) = rest.split_once('%')?;
    pct.trim().parse().ok()
}

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ytdlp(js_runtime: Option<&str>) -> YtDlp {
        YtDlp {
            env: ProcessEnv::with_path(""),
            js_runtime: js_runtime.map(String::from),
        }
    }

    fn track(code: &str, kind: TrackKind) -> SubtitleTrack {
        SubtitleTrack { code: code.into(), name: code.into(), kind }
    }

    #[test]
    fn parses_download_progress() {
        assert_eq!(parse_progress("[download]  12.6% of  246.27KiB at 1MiB/s"), Some(12.6));
        assert_eq!(parse_progress("[download] 100% of 218.53KiB in 00:00:00"), Some(100.0));
        assert_eq!(parse_progress("[download] Destination: a.webm"), None);
        assert_eq!(parse_progress("[info] 50% nothing"), None);
    }

    #[test]
    fn metadata_args_include_js_runtime_when_detected() {
        assert_eq!(
            ytdlp(Some("node")).metadata_args("U"),
            ["--no-playlist", "--js-runtimes", "node", "-J", "U"]
        );
        assert_eq!(ytdlp(None).metadata_args("U"), ["--no-playlist", "-J", "U"]);
    }

    #[test]
    fn video_args_never_embed_subtitles() {
        let args = ytdlp(None).video_args("U", Path::new("/out"));
        assert!(args.iter().all(|a| !a.contains("sub")));
        assert!(args.windows(2).any(|w| w == ["-o", "/out/%(id)s.%(ext)s"]));
        assert_eq!(args.last().unwrap(), "U");
    }

    #[test]
    fn subtitle_args_pick_flag_by_track_kind() {
        let auto = ytdlp(None).subtitle_args("U", Path::new("/o"), &track("ko-orig", TrackKind::Auto));
        assert!(auto.contains(&"--write-auto-subs".to_string()));
        assert!(auto.windows(2).any(|w| w == ["--sub-langs", "ko-orig"]));
        assert!(auto.contains(&"--skip-download".to_string()));

        let manual = ytdlp(None).subtitle_args("U", Path::new("/o"), &track("en", TrackKind::Manual));
        assert!(manual.contains(&"--write-subs".to_string()));
        assert!(!manual.contains(&"--write-auto-subs".to_string()));
    }

    #[test]
    fn subtitle_path_follows_output_template() {
        assert_eq!(
            subtitle_path(Path::new("/o"), "abc", "zh-TW"),
            PathBuf::from("/o/abc.zh-TW.srt")
        );
    }

    #[test]
    fn tail_keeps_last_non_blank_lines() {
        assert_eq!(tail("a\n\nb\nc\n", 2), "b\nc");
    }
}
