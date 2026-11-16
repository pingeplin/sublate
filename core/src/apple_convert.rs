use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tokio::process::Command;

use crate::error::{AppError, AppResult};
use crate::process;
use crate::toolchain::Toolchain;

const ACCEPTED_VIDEO: &[&str] = &["h264", "hevc"];
const ACCEPTED_AUDIO: &str = "aac";
/// Fixed and short: the folder belongs to one video, and a name built from the title could
/// pass the 255-byte limit.
const TEMP_NAME: &str = ".converting.mp4";
const STDERR_TAIL_LINES: usize = 8;

/// Converts a downloaded video into what the Photos app accepts, on the Mac's media engine.
pub struct AppleConverter {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

impl AppleConverter {
    pub fn new(tools: &Toolchain) -> Self {
        Self {
            ffmpeg: tools.ffmpeg_dir.join("ffmpeg"),
            ffprobe: tools.ffmpeg_dir.join("ffprobe"),
        }
    }

    /// Replaces `video` with a converted copy when one of its streams needs it, and tells
    /// whether it did. `on_progress` gets the percent converted. On failure or cancellation
    /// `video` stays as downloaded.
    pub async fn convert(&self, video: &Path, on_progress: impl Fn(f32)) -> AppResult<bool> {
        let probe = self.probe(video).await?;
        let Some(plan) = Plan::for_streams(&probe.streams) else {
            return Ok(false);
        };
        let temp = TempFile(video.with_file_name(TEMP_NAME));
        let mut command = Command::new(&self.ffmpeg);
        command.args(plan.args(video, &temp.0));
        let running = process::spawn(command, None)
            .map_err(|e| AppError::Convert(format!("cannot start {}: {e}", self.ffmpeg.display())))?;
        let duration = probe.duration();
        let output = running
            .finish(|line| {
                if let Some(percent) = progress_percent(line, duration) {
                    on_progress(percent);
                }
            })
            .await?;
        if !output.status.success() {
            return Err(AppError::Convert(process::tail(&output.stderr, STDERR_TAIL_LINES)));
        }
        tokio::fs::rename(&temp.0, video).await?;
        Ok(true)
    }

    async fn probe(&self, video: &Path) -> AppResult<Probe> {
        let mut command = Command::new(&self.ffprobe);
        #[rustfmt::skip]
        command.args([
            "-v", "error", "-of", "json",
            "-show_entries", "stream=codec_type,codec_name:stream_disposition=attached_pic:format=duration",
        ]).arg(video);
        let running = process::spawn(command, None)
            .map_err(|e| AppError::Convert(format!("cannot start {}: {e}", self.ffprobe.display())))?;
        let output = running.finish(|_| {}).await?;
        if !output.status.success() {
            return Err(AppError::Convert(process::tail(&output.stderr, STDERR_TAIL_LINES)));
        }
        Ok(serde_json::from_str(&output.stdout)?)
    }
}

/// Deleted when dropped, so a failed or cancelled conversion leaves nothing behind.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}

impl Probe {
    fn duration(&self) -> Option<f64> {
        self.format.as_ref()?.duration.as_deref()?.parse().ok()
    }
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    #[serde(default)]
    disposition: Disposition,
}

#[derive(Default, Deserialize)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

impl Stream {
    fn is(&self, kind: &str) -> bool {
        self.codec_type.as_deref() == Some(kind)
    }

    fn codec(&self) -> &str {
        self.codec_name.as_deref().unwrap_or_default()
    }
}

/// Which streams get re-encoded; everything else, the cover art included, is copied.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    video: bool,
    audio: bool,
}

impl Plan {
    fn for_streams(streams: &[Stream]) -> Option<Self> {
        let plan = Self {
            video: streams
                .iter()
                .any(|s| s.is("video") && s.disposition.attached_pic == 0 && !ACCEPTED_VIDEO.contains(&s.codec())),
            audio: streams.iter().any(|s| s.is("audio") && s.codec() != ACCEPTED_AUDIO),
        };
        (plan.video || plan.audio).then_some(plan)
    }

    /// HEVC on VideoToolbox: `-q:v 65` with `-prio_speed` kept SSIM at 0.991 against a 4K AV1
    /// source and ran 1.6x real time on an M1 Pro. `:V` leaves the cover art out, `hvc1` is the
    /// tag Apple players require, and a 10-bit HDR source stays Main 10 with its colour tags.
    fn args(&self, input: &Path, output: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> = ["-nostdin", "-v", "error", "-y", "-i"].map(OsString::from).to_vec();
        args.push(input.into());
        args.extend(["-map", "0", "-c", "copy"].map(OsString::from));
        if self.video {
            #[rustfmt::skip]
            args.extend(["-c:V", "hevc_videotoolbox", "-q:v", "65", "-prio_speed", "1", "-tag:V", "hvc1"].map(OsString::from));
        }
        if self.audio {
            args.extend(["-c:a", "aac_at", "-b:a", "192k"].map(OsString::from));
        }
        args.extend(["-movflags", "+faststart", "-progress", "pipe:1", "-nostats"].map(OsString::from));
        args.push(output.into());
        args
    }
}

/// Reads a line of ffmpeg's `-progress` output.
fn progress_percent(line: &str, duration: Option<f64>) -> Option<f32> {
    if line == "progress=end" {
        return Some(100.0);
    }
    let micros: f64 = line.strip_prefix("out_time_us=")?.parse().ok()?;
    let duration = duration.filter(|d| *d > 0.0)?;
    Some((micros / 1e6 / duration * 100.0).clamp(0.0, 100.0) as f32)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn streams(json: &str) -> Vec<Stream> {
        serde_json::from_str::<Probe>(json).unwrap().streams
    }

    fn probe_json(video: &str, audio: &str) -> String {
        format!(
            r#"{{"streams": [
                {{"codec_name": "{video}", "codec_type": "video", "disposition": {{"attached_pic": 0}}}},
                {{"codec_name": "{audio}", "codec_type": "audio", "disposition": {{"attached_pic": 0}}}},
                {{"codec_name": "mjpeg", "codec_type": "video", "disposition": {{"attached_pic": 1}}}}
            ], "format": {{"duration": "10.000000"}}}}"#
        )
    }

    #[test]
    fn only_the_streams_apple_rejects_are_converted() {
        let plan = |video, audio| Plan::for_streams(&streams(&probe_json(video, audio)));

        assert_eq!(plan("av1", "opus"), Some(Plan { video: true, audio: true }));
        assert_eq!(plan("vp9", "aac"), Some(Plan { video: true, audio: false }));
        assert_eq!(plan("hevc", "opus"), Some(Plan { video: false, audio: true }));
        assert_eq!(plan("h264", "aac"), None);
    }

    #[test]
    fn a_video_without_audio_converts_only_the_video() {
        let json = r#"{"streams": [{"codec_name": "vp9", "codec_type": "video"}]}"#;

        assert_eq!(Plan::for_streams(&streams(json)), Some(Plan { video: true, audio: false }));
    }

    #[test]
    fn the_arguments_copy_everything_but_the_streams_to_convert() {
        let args = |plan: Plan| {
            plan.args(Path::new("in.mp4"), Path::new(".converting.mp4"))
                .into_iter()
                .map(|arg| arg.into_string().unwrap())
                .collect::<Vec<_>>()
                .join(" ")
        };

        assert_eq!(
            args(Plan { video: true, audio: true }),
            "-nostdin -v error -y -i in.mp4 -map 0 -c copy \
             -c:V hevc_videotoolbox -q:v 65 -prio_speed 1 -tag:V hvc1 -c:a aac_at -b:a 192k \
             -movflags +faststart -progress pipe:1 -nostats .converting.mp4"
        );
        assert_eq!(
            args(Plan { video: false, audio: true }),
            "-nostdin -v error -y -i in.mp4 -map 0 -c copy -c:a aac_at -b:a 192k \
             -movflags +faststart -progress pipe:1 -nostats .converting.mp4"
        );
    }

    #[test]
    fn progress_is_the_share_of_the_duration_written() {
        assert_eq!(progress_percent("out_time_us=2500000", Some(10.0)), Some(25.0));
        assert_eq!(progress_percent("out_time_us=12000000", Some(10.0)), Some(100.0));
        assert_eq!(progress_percent("progress=end", None), Some(100.0));
        assert_eq!(progress_percent("out_time_us=N/A", Some(10.0)), None);
        assert_eq!(progress_percent("out_time_us=2500000", None), None);
        assert_eq!(progress_percent("frame=12", Some(10.0)), None);
    }

    /// Stand-ins for ffprobe and ffmpeg. The fake ffmpeg sees the output path as its last argument.
    fn converter(dir: &Path, probe: &str, ffmpeg: &str) -> AppleConverter {
        use std::os::unix::fs::PermissionsExt;
        let write = |name: &str, script: String| {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        AppleConverter {
            ffprobe: write("ffprobe", format!("cat <<'EOF'\n{probe}\nEOF")),
            ffmpeg: write("ffmpeg", format!("for arg; do out=$arg; done\n: > \"{}/ran\"\n{ffmpeg}", dir.display())),
        }
    }

    fn downloaded(dir: &Path) -> PathBuf {
        let folder = dir.join("Title [abc]");
        std::fs::create_dir(&folder).unwrap();
        let video = folder.join("Title [abc].mp4");
        std::fs::write(&video, "downloaded").unwrap();
        video
    }

    #[tokio::test]
    async fn a_converted_copy_replaces_the_download_and_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = r#"echo converted > "$out"; echo out_time_us=5000000; echo progress=end"#;
        let converter = converter(dir.path(), &probe_json("av1", "opus"), ffmpeg);
        let video = downloaded(dir.path());
        let progress = Mutex::new(Vec::new());

        let converted = converter.convert(&video, |p| progress.lock().unwrap().push(p)).await.unwrap();

        assert!(converted);
        assert_eq!(std::fs::read_to_string(&video).unwrap(), "converted\n");
        assert_eq!(progress.into_inner().unwrap(), [50.0, 100.0]);
        assert_eq!(std::fs::read_dir(video.parent().unwrap()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn an_accepted_video_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let converter = converter(dir.path(), &probe_json("h264", "aac"), "");
        let video = downloaded(dir.path());

        let converted = converter.convert(&video, |_| {}).await.unwrap();

        assert!(!converted);
        assert_eq!(std::fs::read_to_string(&video).unwrap(), "downloaded");
        assert!(!dir.path().join("ran").exists());
    }

    #[tokio::test]
    async fn a_failed_conversion_keeps_the_download_and_removes_the_partial_copy() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = r#"echo partial > "$out"; echo 'noise' >&2; echo 'Error: no encoder' >&2; exit 1"#;
        let converter = converter(dir.path(), &probe_json("vp9", "opus"), ffmpeg);
        let video = downloaded(dir.path());

        let err = converter.convert(&video, |_| {}).await.unwrap_err().to_string();

        assert!(err.ends_with("noise\nError: no encoder"), "{err}");
        assert_eq!(std::fs::read_to_string(&video).unwrap(), "downloaded");
        assert_eq!(std::fs::read_dir(video.parent().unwrap()).unwrap().count(), 1);
    }
}
