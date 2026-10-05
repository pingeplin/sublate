//! Network-dependent checks against the vendored tools (`make tools`). Run with
//! `cargo test --test live_test -- --ignored --nocapture`.

mod common;

use std::path::Path;

use common::{fixture_path, ko_auto_track};
use sublate_core::auth::CredentialChain;
use sublate_core::file_name::OutputLocation;
use sublate_core::languages::find_target;
use sublate_core::metadata::TrackKind;
use sublate_core::subtitle::parse_srt;
use sublate_core::subtitle_job::translate_subtitle_file;
use sublate_core::translate::claude::ClaudeTranslator;
use sublate_core::toolchain::Toolchain;
use sublate_core::translate::{BatchRequest, Translator};
use sublate_core::update::{GitHubReleases, Installs, ReleaseFeed, Updater, Version};
use sublate_core::ytdlp::YtDlp;

const KOREAN_VIDEO: &str = "https://www.youtube.com/watch?v=SrvYHXmiLAY";
const SHORT_VIDEO: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
const PLAYLIST: &str = "https://www.youtube.com/playlist?list=PLbpi6ZahtOH5GvM7crvM-15gZseMm3zKd";

fn tools() -> Toolchain {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/tools");
    Toolchain::bundled(&dir).expect("run `make tools` first")
}

/// Caches go to a throwaway directory so each run starts as a fresh install would.
fn ytdlp() -> (YtDlp, tempfile::TempDir) {
    let cache = tempfile::tempdir().unwrap();
    (YtDlp::new(tools(), cache.path()), cache)
}

fn translator() -> ClaudeTranslator {
    ClaudeTranslator::new(Box::new(CredentialChain::default())).unwrap()
}

#[tokio::test]
#[ignore]
async fn ytdlp_fetches_metadata_and_downloads_auto_subtitle() {
    let (ytdlp, _cache) = ytdlp();
    let meta = ytdlp.fetch_metadata(KOREAN_VIDEO).await.unwrap();
    let track = meta.subtitles.iter().find(|t| t.kind == TrackKind::Auto).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let out = OutputLocation::new(dir.path().join("new dir"), &meta.title, &meta.id);
    let path = ytdlp.download_subtitle(&meta.url, &out, track).await.unwrap();
    let cues = parse_srt(&std::fs::read_to_string(&path).unwrap()).unwrap();
    println!("{} -> {} cues at {}", track.code, cues.len(), path.display());
    assert!(!cues.is_empty());
}

#[tokio::test]
#[ignore]
async fn ytdlp_downloads_video_literally_named_into_any_directory() {
    let (ytdlp, _cache) = ytdlp();
    let meta = ytdlp.fetch_metadata(SHORT_VIDEO).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = OutputLocation::new(dir.path().join("dir $HOME 100%"), "Save $HOME 100%?", &meta.id);
    let progress = std::sync::Mutex::new(Vec::new());
    let path = ytdlp
        .download_video(&meta.url, &out, |p| progress.lock().unwrap().push(p))
        .await
        .unwrap();
    let files: Vec<String> = std::fs::read_dir(out.dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let progress = progress.into_inner().unwrap();
    println!("video: {} / dir: {files:?} / {} progress events", path.display(), progress.len());
    assert!(path.is_file());
    assert_eq!(files, [format!("Save ＄HOME 100%？ [{}].mp4", meta.id)]);
    let probe = tokio::process::Command::new(tools().ffmpeg_dir.join("ffprobe"))
        .args(["-v", "error", "-show_entries", "stream=codec_type:stream_disposition=attached_pic", "-of", "compact"])
        .arg(&path)
        .output()
        .await
        .unwrap();
    let streams = String::from_utf8_lossy(&probe.stdout);
    println!("{streams}");
    assert!(streams.contains("attached_pic=1"), "cover art embedded");
    assert!(progress.last().is_some_and(|&p| p >= 99.0));
}

#[tokio::test]
#[ignore]
async fn ytdlp_rejects_playlists() {
    let (ytdlp, _cache) = ytdlp();
    let err = ytdlp.fetch_metadata(PLAYLIST).await.unwrap_err();
    println!("{err}");
    assert!(matches!(err, sublate_core::error::AppError::Unsupported(_)));
}

#[tokio::test]
#[ignore]
async fn updater_installs_the_latest_release_from_github() {
    let dir = tempfile::tempdir().unwrap();
    let installs = || Installs::new(dir.path().join("yt-dlp"));
    let feed = GitHubReleases::new().unwrap();
    let latest = feed.latest().await.unwrap().version;
    let updater = Updater::new(Box::new(feed), installs(), dir.path().join("last-check"));

    let outdated = Version::parse("2020.01.01").unwrap();
    let install = updater.refresh(&outdated, false).await.unwrap().expect("a newer release");
    println!("installed {} at {}", install.version, install.program.display());
    assert_eq!(install.version, latest);
    assert_eq!(installs().newest(), Some(install.clone()));

    let cache = tempfile::tempdir().unwrap();
    let ytdlp = YtDlp::new(Toolchain { ytdlp: install, ..tools() }, cache.path());
    let meta = ytdlp.fetch_metadata(SHORT_VIDEO).await.unwrap();
    assert_eq!(meta.title, "Me at the zoo");
}

#[tokio::test]
#[ignore]
async fn claude_translates_korean_to_traditional_chinese() {
    let lines = vec![
        "오늘은 집에서 보리차를 끓이는 방법을".to_string(),
        "차근차근 알려 드리려고 합니다 먼저".to_string(),
        "냄비에 물을 이 리터 정도 붓고".to_string(),
    ];
    let out = translator()
        .translate(BatchRequest {
            source_language: "Korean",
            target_language: "Traditional Chinese as used in Taiwan",
            context: &[],
            lines: &lines,
        })
        .await
        .unwrap();
    println!("{out:#?}");
    assert_eq!(out.len(), lines.len());
}

#[tokio::test]
#[ignore]
async fn claude_translates_full_auto_caption_file() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out.zh-TW.srt");

    let count = translate_subtitle_file(
        &translator(),
        &fixture_path("ko_auto_rolling.srt"),
        &ko_auto_track(),
        find_target("zh-TW").unwrap(),
        &output,
        |done, total| println!("translated {done}/{total} cues"),
    )
    .await
    .unwrap();

    let written = std::fs::read_to_string(&output).unwrap();
    println!("{count} cues\n{}", written.lines().take(16).collect::<Vec<_>>().join("\n"));
    let cues = parse_srt(&written).unwrap();
    assert_eq!(cues.len(), count);
    assert!(cues.iter().all(|c| !c.text.trim().is_empty()));
    assert!(!written.contains(", ") && !written.contains(". "));
}
