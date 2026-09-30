//! Network-dependent checks. Run with `cargo test --test live_test -- --ignored --nocapture`.

use contents_title_lib::auth::resolve_provider;
use contents_title_lib::file_name::OutputLocation;
use contents_title_lib::metadata::TrackKind;
use contents_title_lib::process_env::ProcessEnv;
use contents_title_lib::subtitle::parse_srt;
use contents_title_lib::translate::claude::{ClaudeTranslator, DEFAULT_MODEL};
use contents_title_lib::translate::{BatchRequest, Translator};
use contents_title_lib::ytdlp::YtDlp;

const KOREAN_VIDEO: &str = "https://www.youtube.com/watch?v=SrvYHXmiLAY";
const SHORT_VIDEO: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";

#[tokio::test]
#[ignore]
async fn ytdlp_fetches_metadata_and_downloads_auto_subtitle() {
    let ytdlp = YtDlp::new(ProcessEnv::from_login_shell());
    let meta = ytdlp.fetch_metadata(KOREAN_VIDEO).await.unwrap();
    let track = meta.subtitles.iter().find(|t| t.kind == TrackKind::Auto).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let out = OutputLocation::new(dir.path(), &meta.title, &meta.id);
    let path = ytdlp.download_subtitle(&meta.url, &out, track).await.unwrap();
    let cues = parse_srt(&std::fs::read_to_string(&path).unwrap()).unwrap();
    println!("{} -> {} cues at {}", track.code, cues.len(), path.display());
    assert!(!cues.is_empty());
}

#[tokio::test]
#[ignore]
async fn ytdlp_downloads_video_without_subtitles() {
    let ytdlp = YtDlp::new(ProcessEnv::from_login_shell());
    let dir = tempfile::tempdir().unwrap();
    let out = OutputLocation::new(dir.path(), "Me at the zoo?", "jNQXAC9IVRw");
    let path = ytdlp.download_video(SHORT_VIDEO, &out, |_| {}).await.unwrap();
    let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
    println!("video: {} / dir: {files:?}", path.display());
    assert!(path.is_file());
    assert_eq!(files.len(), 1);
}

#[tokio::test]
#[ignore]
async fn claude_translates_korean_to_traditional_chinese() {
    let env = ProcessEnv::from_login_shell();
    let translator = ClaudeTranslator::new(resolve_provider(&env), DEFAULT_MODEL).unwrap();
    let lines = vec![
        "오늘은 집에서 보리차를 끓이는 방법을".to_string(),
        "차근차근 알려 드리려고 합니다 먼저".to_string(),
        "냄비에 물을 이 리터 정도 붓고".to_string(),
    ];
    let out = translator
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
    use contents_title_lib::languages::find_target;
    use contents_title_lib::metadata::SubtitleTrack;
    use contents_title_lib::subtitle_job::translate_subtitle_file;
    use contents_title_lib::translate::TranslationPlan;

    let env = ProcessEnv::from_login_shell();
    let translator = ClaudeTranslator::new(resolve_provider(&env), DEFAULT_MODEL).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out.zh-TW.srt");
    let track = SubtitleTrack { code: "ko-orig".into(), name: "Korean (Original)".into(), kind: TrackKind::Auto };
    let source = std::path::PathBuf::from(format!("{}/tests/fixtures/ko_auto_rolling.srt", env!("CARGO_MANIFEST_DIR")));

    let count = translate_subtitle_file(
        &translator,
        &source,
        &track,
        find_target("zh-TW").unwrap(),
        &output,
        TranslationPlan::default(),
        |done, total| println!("batch {done}/{total}"),
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
