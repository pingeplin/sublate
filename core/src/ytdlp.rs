use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::disk;
use crate::error::{AppError, AppResult};
use crate::file_name::OutputLocation;
use crate::metadata::{parse_metadata, SubtitleTrack, TrackKind, VideoMetadata};
use crate::process;
use crate::subtitle_job::SubtitleSource;
use crate::toolchain::Toolchain;
use crate::update::ActiveRelease;
use crate::video_progress::{ByteProgress, DownloadEvent};

/// What Finder-launched apps get; fixed so tools on the user's own PATH never take part.
const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
const PROGRESS_TAG: &str = "CT_PROGRESS";
const STREAMS_TAG: &str = "CT_STREAMS";
const PATH_TAG: &str = "CT_PATH";

/// Which streams a video download prefers.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StreamRequest {
    /// Caps the video at the tallest stream no taller than it, or the shortest one when every
    /// stream is taller; `None` takes the tallest.
    pub max_height: Option<u32>,
    /// Takes H.264 with AAC over other codecs of the same height.
    pub prefer_h264_aac: bool,
}

impl StreamRequest {
    /// `-S` value: a preference, not a `-f` filter, so it still picks a stream when a fresh
    /// page extraction offers different heights. `height`, not `res`, which is the smaller side.
    fn sort(&self) -> Option<String> {
        let cap = self.max_height.map(|h| format!("height:{h}"));
        if !self.prefer_h264_aac {
            return cap;
        }
        // The height comes before the codec, so H.264 only wins over VP9 or AV1 of the same
        // height, and before `acodec`, so a small muxed H.264+AAC stream never wins over a
        // taller video-only one. `lang` first keeps the original audio language.
        let height = cap.unwrap_or_else(|| "height".into());
        Some(format!("lang,{height},vcodec:h264,acodec:aac"))
    }
}

pub struct YtDlp {
    /// Read at each run, so a release the updater switches to is used from the next run on.
    release: Arc<ActiveRelease>,
    /// Options every invocation starts with.
    base_args: Vec<String>,
    cache_dir: PathBuf,
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
    pub fn new(tools: Toolchain, release: Arc<ActiveRelease>, cache_dir: &Path) -> Self {
        let text = |path: &Path| path.to_string_lossy().into_owned();
        let deno_dir = cache_dir.join("deno");
        let cache_dir = cache_dir.join("yt-dlp");
        #[rustfmt::skip]
        let base_args = vec![
            "--ignore-config".into(), "--no-playlist".into(),
            "--ffmpeg-location".into(), text(&tools.ffmpeg_dir),
            "--js-runtimes".into(), format!("deno:{}", text(&tools.deno)),
            "--cache-dir".into(), text(&cache_dir),
        ];
        Self {
            release,
            base_args,
            cache_dir,
            deno_dir,
            last_info: Mutex::new(None),
        }
    }

    /// Deletes what yt-dlp and deno cached; both fill it again as they run.
    pub async fn clear_cache(&self) -> AppResult<()> {
        disk::delete(&self.cache_dir).await?;
        Ok(disk::delete(&self.deno_dir).await?)
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
        request: &StreamRequest,
        on_event: impl Fn(DownloadEvent),
    ) -> AppResult<PathBuf> {
        let progress = format!(
            "download:{PROGRESS_TAG} %(info.format_id)s %(progress.status)s %(progress._percent)s \
             %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s"
        );
        let streams = format!("before_dl:{STREAMS_TAG} %(format_id)s");
        let path = format!("after_move:{PATH_TAG} %(filepath)s");
        // Always an mp4 carrying the thumbnail as cover art, so Quick Look (Finder, open
        // dialogs) previews it and players show the cover. mp4 is forced because yt-dlp's
        // `mp4/mkv` fallback picks mkv whenever the audio is Opus. Streams are copied, never
        // re-encoded.
        #[rustfmt::skip]
        let mut flags = vec![
            "--newline", "--progress", "--progress-template", &progress,
            "--print", &streams, "--print", &path,
            "--merge-output-format", "mp4", "--remux-video", "mp4",
            "--embed-thumbnail", "--convert-thumbnails", "jpg",
        ];
        let sort = request.sort();
        if let Some(sort) = &sort {
            flags.extend(["-S", sort]);
        }
        let stdout = self
            .download(url, out, &flags, |line| {
                if let Some(event) = download_event(line) {
                    on_event(event);
                }
            })
            .await?;
        stdout
            .lines()
            .find_map(|line| tagged(line, PATH_TAG))
            .map(PathBuf::from)
            .ok_or_else(|| AppError::YtDlp("video path not reported".into()))
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
        let program = self.release.get().ok_or(AppError::YtDlpMissing)?.program;
        let mut command = tokio::process::Command::new(&program);
        command.args(args).env("PATH", SYSTEM_PATH).env("DENO_DIR", &self.deno_dir);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        process::run(command, stdin, on_line, AppError::YtDlp).await
    }
}

#[async_trait]
impl SubtitleSource for YtDlp {
    async fn download_subtitle(
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
}

fn write_flag(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Manual => "--write-subs",
        TrackKind::Auto => "--write-auto-subs",
    }
}

fn download_event(line: &str) -> Option<DownloadEvent> {
    if let Some(ids) = tagged(line, STREAMS_TAG) {
        return Some(DownloadEvent::Streams { format_ids: ids.split('+').map(String::from).collect() });
    }
    let mut fields = tagged(line, PROGRESS_TAG)?.split(' ');
    let format_id = fields.next()?.to_string();
    let finished = match fields.next()? {
        "finished" => true,
        "downloading" => false,
        _ => return None,
    };
    let [percent, done, total, estimate] = [(); 4].map(|_| fields.next().and_then(|v| v.parse::<f64>().ok()));
    let bytes = done.map(|done| ByteProgress { done: done as u64, total: total.or(estimate).map(|t| t as u64) });
    Some(DownloadEvent::Stream { format_id, finished, percent: percent.map(|p| p as f32), bytes })
}

/// Our `--print`/`--progress-template` lines are prefixed with a tag and a space.
fn tagged<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    line.strip_prefix(tag)?.strip_prefix(' ')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::{Install, Version};

    fn install(version: &str) -> Install {
        Install::at(Version::parse(version).unwrap(), &Path::new("/support").join(version))
    }

    fn tools() -> Toolchain {
        Toolchain {
            ffmpeg_dir: "/tools".into(),
            deno: "/tools/deno".into(),
        }
    }

    fn active(install: Option<Install>) -> Arc<ActiveRelease> {
        Arc::new(ActiveRelease::new(install))
    }

    fn ytdlp() -> YtDlp {
        YtDlp::new(tools(), active(Some(install("2026.08.19"))), Path::new("/cache"))
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

    #[tokio::test]
    async fn clearing_the_cache_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let ytdlp = YtDlp::new(tools(), active(None), dir.path());
        for cache in ["yt-dlp/youtube-sigfuncs", "deno/npm"] {
            std::fs::create_dir_all(dir.path().join(cache)).unwrap();
        }

        ytdlp.clear_cache().await.unwrap();

        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    const URL: &str = "https://example.com/v";
    const INFO: &str = r#"{"id": "abc", "title": "Title"}"#;

    /// A stand-in release: a shell script run in place of yt-dlp.
    fn scripted_release(dir: &Path, script: &str) -> Install {
        use std::os::unix::fs::PermissionsExt;
        let install = Install::at(Version::parse("2026.08.19").unwrap(), dir);
        std::fs::write(&install.program, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&install.program, std::fs::Permissions::from_mode(0o755)).unwrap();
        install
    }

    fn scripted(dir: &Path, script: &str) -> YtDlp {
        YtDlp::new(tools(), active(Some(scripted_release(dir, script))), &dir.join("cache"))
    }

    fn english() -> SubtitleTrack {
        SubtitleTrack {
            code: "en".into(),
            name: "English".into(),
            kind: TrackKind::Manual,
        }
    }

    #[tokio::test]
    async fn each_run_uses_the_release_in_use_at_that_moment() {
        let dir = tempfile::tempdir().unwrap();
        let release = active(None);
        let ytdlp = YtDlp::new(tools(), Arc::clone(&release), &dir.path().join("cache"));

        let err = ytdlp.fetch_metadata(URL).await.unwrap_err();
        assert_eq!(err.to_string(), "yt-dlp is not installed; download it in Settings (⌘,)");

        release.set(Some(scripted_release(dir.path(), &format!("echo '{INFO}'"))));
        assert_eq!(ytdlp.fetch_metadata(URL).await.unwrap().id, "abc");
    }

    #[tokio::test]
    async fn fetching_runs_the_installed_release_and_reads_what_it_prints() {
        let dir = tempfile::tempdir().unwrap();
        let ytdlp = scripted(dir.path(), &format!("echo '{INFO}'"));

        let metadata = ytdlp.fetch_metadata(URL).await.unwrap();

        assert_eq!((metadata.id.as_str(), metadata.title.as_str()), ("abc", "Title"));
        assert!(ytdlp.cached_info(URL).is_some());
    }

    #[tokio::test]
    async fn every_run_sees_the_system_path_and_the_app_s_deno_cache() {
        let dir = tempfile::tempdir().unwrap();
        let ytdlp = scripted(dir.path(), r#"echo "{\"id\": \"$PATH\", \"title\": \"$DENO_DIR\"}""#);

        let metadata = ytdlp.fetch_metadata(URL).await.unwrap();

        assert_eq!(metadata.id, SYSTEM_PATH);
        assert_eq!(Path::new(&metadata.title), dir.path().join("cache/deno"));
    }

    #[tokio::test]
    async fn a_failed_run_reports_the_end_of_its_error_output() {
        let dir = tempfile::tempdir().unwrap();
        let ytdlp = scripted(dir.path(), "echo 'WARNING: slow' >&2; echo 'ERROR: Unsupported URL' >&2; exit 1");

        let err = ytdlp.fetch_metadata(URL).await.unwrap_err();

        assert_eq!(err.to_string(), "yt-dlp failed: WARNING: slow\nERROR: Unsupported URL");
    }

    #[tokio::test]
    async fn a_release_that_cannot_start_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let ytdlp = YtDlp::new(tools(), active(Some(install("2026.08.19"))), dir.path());

        let err = ytdlp.fetch_metadata(URL).await.unwrap_err().to_string();

        assert!(err.starts_with("yt-dlp failed: cannot start /support/2026.08.19/yt-dlp_macos: "), "{err}");
    }

    #[tokio::test]
    async fn a_video_download_reports_its_streams_and_their_progress_and_the_path_it_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let script = "echo 'CT_STREAMS 395+251'; echo 'CT_PROGRESS 395 downloading 50.0 1024 2048 NA'
            echo '[download] noise'; echo 'CT_PROGRESS 395 finished 100.0 2048 2048 NA'
            echo 'CT_PROGRESS 251 finished 100.0 NA 4096 NA'
            : > ran-here; echo 'CT_PATH /out/Title [abc].mp4'";
        let ytdlp = scripted(dir.path(), script);
        let out = OutputLocation::new(dir.path().join("new dir"), "Title", "abc");
        let events = Mutex::new(Vec::new());

        let path = ytdlp.download_video(URL, &out, &StreamRequest::default(), |e| events.lock().unwrap().push(e)).await.unwrap();

        assert_eq!(path, Path::new("/out/Title [abc].mp4"));
        assert_eq!(
            events.into_inner().unwrap(),
            [
                DownloadEvent::Streams { format_ids: vec!["395".into(), "251".into()] },
                stream("395", false, Some(50.0), Some(ByteProgress { done: 1024, total: Some(2048) })),
                stream("395", true, Some(100.0), Some(ByteProgress { done: 2048, total: Some(2048) })),
                stream("251", true, Some(100.0), None),
            ]
        );
        assert!(out.dir().join("ran-here").is_file());
    }

    #[tokio::test]
    async fn a_video_download_asks_yt_dlp_for_the_streams_and_the_fields_it_reads() {
        let dir = tempfile::tempdir().unwrap();
        let ytdlp = scripted(dir.path(), r#"for arg; do echo "$arg"; done > args; echo 'CT_PATH /out/Title [abc].mp4'"#);
        let out = OutputLocation::new(dir.path().to_path_buf(), "Title", "abc");

        ytdlp.download_video(URL, &out, &StreamRequest::default(), |_| {}).await.unwrap();

        let args = std::fs::read_to_string(out.dir().join("args")).unwrap();
        let args: Vec<&str> = args.lines().collect();
        let after = |flag: &str| args.windows(2).filter(|w| w[0] == flag).map(|w| w[1]).collect::<Vec<_>>();
        assert!(after("--print").contains(&"before_dl:CT_STREAMS %(format_id)s"), "{args:?}");
        assert_eq!(
            after("--progress-template"),
            ["download:CT_PROGRESS %(info.format_id)s %(progress.status)s %(progress._percent)s \
              %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s"]
        );
    }

    fn stream(id: &str, finished: bool, percent: Option<f32>, bytes: Option<ByteProgress>) -> DownloadEvent {
        DownloadEvent::Stream { format_id: id.into(), finished, percent, bytes }
    }

    #[test]
    fn a_download_line_prefers_the_size_over_the_estimate() {
        let event = download_event("CT_PROGRESS 18 downloading 30.0 1100000 3600000 4000000");

        let bytes = ByteProgress { done: 1_100_000, total: Some(3_600_000) };
        assert_eq!(event, Some(stream("18", false, Some(30.0), Some(bytes))));
    }

    #[test]
    fn a_download_line_with_another_status_is_ignored() {
        assert_eq!(download_event("CT_PROGRESS 18 error NA NA NA NA"), None);
    }

    #[test]
    fn a_download_line_reads_the_estimate_when_the_size_is_unknown() {
        let event = download_event("CT_PROGRESS 18 downloading 30.0 1100000 NA 3600000.5");

        let bytes = ByteProgress { done: 1_100_000, total: Some(3_600_000) };
        assert_eq!(event, Some(stream("18", false, Some(30.0), Some(bytes))));
    }

    #[test]
    fn a_download_line_without_sizes_or_percent_keeps_what_it_has() {
        let no_total = download_event("CT_PROGRESS 18 downloading 30.0 1100000 NA NA");
        let no_percent = download_event("CT_PROGRESS 18 downloading NA 1100000 3600000 NA");

        let with_total = |total| Some(ByteProgress { done: 1_100_000, total });
        assert_eq!(no_total, Some(stream("18", false, Some(30.0), with_total(None))));
        assert_eq!(no_percent, Some(stream("18", false, None, with_total(Some(3_600_000)))));
    }

    #[tokio::test]
    async fn a_height_limit_becomes_a_sort_preference_and_no_limit_adds_none() {
        let dir = tempfile::tempdir().unwrap();
        let ytdlp = scripted(dir.path(), r#"echo "$*" > args; echo 'CT_PATH /out/Title [abc].mp4'"#);
        let args_for = |max_height| {
            let out = OutputLocation::new(dir.path().join(format!("{max_height:?}")), "Title", "abc");
            let ytdlp = &ytdlp;
            async move {
                let request = StreamRequest { max_height, prefer_h264_aac: false };
                ytdlp.download_video(URL, &out, &request, |_| {}).await.unwrap();
                let args = std::fs::read_to_string(out.dir().join("args")).unwrap();
                args.split_whitespace().map(String::from).collect::<Vec<_>>()
            }
        };

        let limited = args_for(Some(720)).await;
        let unlimited = args_for(None).await;

        assert!(limited.windows(2).any(|pair| pair == ["-S", "height:720"]), "{limited:?}");
        assert!(!unlimited.iter().any(|arg| arg == "-S"), "{unlimited:?}");
    }

    #[test]
    fn h264_is_preferred_after_the_height_and_before_the_audio_codec() {
        let sort = |max_height| StreamRequest { max_height, prefer_h264_aac: true }.sort();

        assert_eq!(sort(Some(720)).as_deref(), Some("lang,height:720,vcodec:h264,acodec:aac"));
        assert_eq!(sort(None).as_deref(), Some("lang,height,vcodec:h264,acodec:aac"));
    }

    #[tokio::test]
    async fn a_download_reuses_the_fetched_info_instead_of_the_page() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            r#"case " $* " in
                *" -J "*) echo '{INFO}' ;;
                *" --load-info-json - "*) cat > info.json; : > 'Title [abc].en.srt' ;;
                *) exit 1 ;;
            esac"#
        );
        let ytdlp = scripted(dir.path(), &script);
        let out = OutputLocation::new(dir.path().join("out"), "Title", "abc");
        ytdlp.fetch_metadata(URL).await.unwrap();

        let path = ytdlp.download_subtitle(URL, &out, &english()).await.unwrap();

        assert_eq!(path, out.subtitle("en"));
        assert_eq!(std::fs::read_to_string(out.dir().join("info.json")).unwrap(), format!("{INFO}\n"));
    }

    #[tokio::test]
    async fn a_download_falls_back_to_the_page_when_the_fetched_info_fails() {
        let dir = tempfile::tempdir().unwrap();
        let script = format!(
            r#"case " $* " in
                *" -J "*) echo '{INFO}' ;;
                *" --load-info-json - "*) echo 'ERROR: expired' >&2; exit 1 ;;
                *" -- {URL} "*) : > 'Title [abc].en.srt' ;;
            esac"#
        );
        let ytdlp = scripted(dir.path(), &script);
        let out = OutputLocation::new(dir.path().join("out"), "Title", "abc");
        ytdlp.fetch_metadata(URL).await.unwrap();

        assert_eq!(ytdlp.download_subtitle(URL, &out, &english()).await.unwrap(), out.subtitle("en"));
    }

    #[tokio::test]
    async fn a_subtitle_that_was_not_written_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let ytdlp = scripted(dir.path(), "exit 0");
        let out = OutputLocation::new(dir.path().join("out"), "Title", "abc");

        let err = ytdlp.download_subtitle(URL, &out, &english()).await.unwrap_err().to_string();

        assert!(err.starts_with("yt-dlp failed: subtitle 'en' was not produced at "), "{err}");
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
}
