mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use common::{fixture_path, ko_auto_track};
use sublate_core::error::{AppError, AppResult};
use sublate_core::file_name::OutputLocation;
use sublate_core::languages::find_target;
use sublate_core::metadata::{SubtitleTrack, TrackKind};
use sublate_core::subtitle::{parse_srt, Cue};
use sublate_core::subtitle_job::{download_and_translate, translate_subtitle_file, SubtitleSource};
use sublate_core::translate::{translate_cues, BatchRequest, TranslationPlan, Translator};

/// (context, lines, source language, target language)
type Recorded = (Vec<String>, Vec<String>, String, String);

#[derive(Default)]
struct UppercaseTranslator {
    requests: Mutex<Vec<Recorded>>,
}

#[async_trait]
impl Translator for UppercaseTranslator {
    async fn translate(&self, r: BatchRequest<'_>) -> AppResult<Vec<String>> {
        self.requests.lock().unwrap().push((
            r.context.to_vec(),
            r.lines.to_vec(),
            r.source_language.to_string(),
            r.target_language.to_string(),
        ));
        Ok(r.lines.iter().map(|l| l.to_uppercase()).collect())
    }
}

/// Drops the last line on the first `failures` calls, then behaves.
struct FlakyTranslator {
    failures: usize,
    calls: AtomicUsize,
}

#[async_trait]
impl Translator for FlakyTranslator {
    async fn translate(&self, r: BatchRequest<'_>) -> AppResult<Vec<String>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let keep = if call < self.failures { r.lines.len() - 1 } else { r.lines.len() };
        Ok(r.lines[..keep].to_vec())
    }
}

struct FailingTranslator;

#[async_trait]
impl Translator for FailingTranslator {
    async fn translate(&self, _: BatchRequest<'_>) -> AppResult<Vec<String>> {
        Err(AppError::Auth("no token".into()))
    }
}

fn cues(n: usize) -> Vec<Cue> {
    (0..n as u64)
        .map(|i| Cue { start_ms: i * 1000, end_ms: i * 1000 + 900, text: format!("line {i}") })
        .collect()
}

fn plan(batch_size: usize, concurrency: usize) -> TranslationPlan {
    TranslationPlan { batch_size, context_size: 2, concurrency, max_attempts: 3 }
}

#[tokio::test]
async fn translates_every_cue_in_order_reporting_progress_in_cues() {
    let source = cues(7);
    let progress = Mutex::new(Vec::new());
    let out = translate_cues(&UppercaseTranslator::default(), &source, "Korean", "English", plan(3, 3), |d, t| {
        progress.lock().unwrap().push((d, t))
    })
    .await
    .unwrap();

    assert_eq!(out.len(), 7);
    for (src, dst) in source.iter().zip(&out) {
        assert_eq!((src.start_ms, src.end_ms), (dst.start_ms, dst.end_ms));
        assert_eq!(dst.text, src.text.to_uppercase());
    }
    let progress = progress.into_inner().unwrap();
    assert_eq!(progress.first(), Some(&(0, 7)));
    assert_eq!(progress.last(), Some(&(7, 7)));
    assert_eq!(progress.len(), 4);
    assert!(progress.windows(2).all(|w| w[0].0 < w[1].0 && w[1].1 == 7));
}

#[tokio::test]
async fn batches_carry_preceding_lines_as_context() {
    let translator = UppercaseTranslator::default();
    translate_cues(&translator, &cues(5), "Korean", "English", plan(2, 1), |_, _| {})
        .await
        .unwrap();

    let requests = translator.requests.into_inner().unwrap();
    let shapes: Vec<(Vec<String>, Vec<String>)> =
        requests.iter().map(|(c, l, _, _)| (c.clone(), l.clone())).collect();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(
        shapes,
        [
            (s(&[]), s(&["line 0", "line 1"])),
            (s(&["line 0", "line 1"]), s(&["line 2", "line 3"])),
            (s(&["line 2", "line 3"]), s(&["line 4"])),
        ]
    );
}

#[tokio::test]
async fn retries_misaligned_batches() {
    let translator = FlakyTranslator { failures: 2, calls: AtomicUsize::new(0) };
    let out = translate_cues(&translator, &cues(3), "a", "b", plan(10, 1), |_, _| {}).await.unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(translator.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn gives_up_after_max_attempts() {
    let translator = FlakyTranslator { failures: usize::MAX, calls: AtomicUsize::new(0) };
    let err = translate_cues(&translator, &cues(3), "a", "b", plan(10, 1), |_, _| {}).await.unwrap_err();
    assert!(matches!(err, AppError::Translation(_)));
    assert_eq!(translator.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn non_translation_errors_are_not_retried() {
    let err = translate_cues(&FailingTranslator, &cues(3), "a", "b", plan(10, 1), |_, _| {}).await.unwrap_err();
    assert!(matches!(err, AppError::Auth(_)));
}

#[tokio::test]
async fn empty_subtitles_translate_to_empty_output() {
    let out = translate_cues(&FailingTranslator, &[], "a", "b", plan(10, 1), |_, _| {}).await.unwrap();
    assert!(out.is_empty());
}

#[tokio::test]
async fn subtitle_file_job_collapses_auto_captions_and_writes_separate_srt() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("vid.ko-orig.srt");
    let output = dir.path().join("vid.zh-TW.srt");
    std::fs::copy(fixture_path("ko_auto_rolling.srt"), &source).unwrap();
    let original = std::fs::read_to_string(&source).unwrap();
    let track = ko_auto_track();
    let translator = UppercaseTranslator::default();

    let count = translate_subtitle_file(
        &translator,
        &source,
        &track,
        find_target("zh-TW").unwrap(),
        &output,
        |_, _| {},
    )
    .await
    .unwrap();

    let written = parse_srt(&std::fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(written.len(), count);
    assert!(count < parse_srt(&original).unwrap().len());
    assert_eq!(written[1].text, "차근차근 알려 드리려고 합니다 먼저");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), original);

    let (_, _, src_lang, tgt_lang) = &translator.requests.lock().unwrap()[0];
    assert_eq!(src_lang, "Korean");
    assert_eq!(tgt_lang, "Traditional Chinese as used in Taiwan");
}

struct HalfWidthTranslator;

#[async_trait]
impl Translator for HalfWidthTranslator {
    async fn translate(&self, r: BatchRequest<'_>) -> AppResult<Vec<String>> {
        Ok(r.lines.iter().map(|_| "政府, 上個月調漲了 3.5%.".to_string()).collect())
    }
}

#[tokio::test]
async fn subtitle_file_job_normalizes_punctuation_for_the_target_language() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("v.en.srt");
    std::fs::write(&source, "1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();
    let track = SubtitleTrack { code: "en".into(), name: "English".into(), kind: TrackKind::Manual };

    for (target, expected) in [
        ("zh-TW", "政府，上個月調漲了 3.5%。"),
        ("en", "政府, 上個月調漲了 3.5%."),
    ] {
        let output = dir.path().join(format!("v.{target}.srt"));
        translate_subtitle_file(
            &HalfWidthTranslator,
            &source,
            &track,
            find_target(target).unwrap(),
            &output,
            |_, _| {},
        )
        .await
        .unwrap();
        let written = parse_srt(&std::fs::read_to_string(&output).unwrap()).unwrap();
        assert_eq!(written[0].text, expected, "target {target}");
    }
}

/// Writes one cue where the track belongs, or fails when the video has no such track.
struct OneCueSource;

#[async_trait]
impl SubtitleSource for OneCueSource {
    async fn download_subtitle(
        &self,
        _url: &str,
        out: &OutputLocation,
        track: &SubtitleTrack,
    ) -> AppResult<PathBuf> {
        if track.code == "missing" {
            return Err(AppError::YtDlp("no such track".into()));
        }
        let path = out.subtitle(&track.code);
        std::fs::write(&path, "1\n00:00:01,000 --> 00:00:02,000\nhello\n").unwrap();
        Ok(path)
    }
}

fn manual_track(code: &str) -> SubtitleTrack {
    SubtitleTrack { code: code.into(), name: "English".into(), kind: TrackKind::Manual }
}

#[tokio::test]
async fn subtitle_job_writes_the_translation_beside_the_downloaded_track() {
    let dir = tempfile::tempdir().unwrap();
    let out = OutputLocation::new(dir.path(), "Title", "abc");

    for (target, translated_path) in [
        ("zh-TW", out.subtitle("zh-TW")),
        ("en", out.subtitle("en.translated")),
    ] {
        let output = download_and_translate(
            &OneCueSource,
            &UppercaseTranslator::default(),
            "https://example.com/v",
            &out,
            &manual_track("en"),
            find_target(target).unwrap(),
            |_, _| {},
        )
        .await
        .unwrap();

        assert_eq!(output.source_path, out.subtitle("en"));
        assert_eq!(output.translated_path, translated_path, "target {target}");
        assert_eq!(output.cue_count, 1);
        let read = |path| parse_srt(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(read(&output.source_path)[0].text, "hello");
        assert_eq!(read(&output.translated_path)[0].text, "HELLO");
    }
}

#[tokio::test]
async fn subtitle_job_translates_nothing_when_the_download_fails() {
    let dir = tempfile::tempdir().unwrap();
    let out = OutputLocation::new(dir.path(), "Title", "abc");
    let translator = UppercaseTranslator::default();

    let err = download_and_translate(
        &OneCueSource,
        &translator,
        "https://example.com/v",
        &out,
        &manual_track("missing"),
        find_target("zh-TW").unwrap(),
        |_, _| {},
    )
    .await
    .unwrap_err();

    assert!(matches!(err, AppError::YtDlp(_)));
    assert!(translator.requests.into_inner().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// Cuts off any batch larger than two lines, like a response hitting max_tokens.
#[derive(Default)]
struct TruncatingTranslator {
    batch_sizes: Mutex<Vec<usize>>,
}

#[async_trait]
impl Translator for TruncatingTranslator {
    async fn translate(&self, r: BatchRequest<'_>) -> AppResult<Vec<String>> {
        self.batch_sizes.lock().unwrap().push(r.lines.len());
        if r.lines.len() > 2 {
            return Err(AppError::Truncated);
        }
        Ok(r.lines.iter().map(|l| l.to_uppercase()).collect())
    }
}

#[tokio::test]
async fn truncated_batches_are_split_instead_of_resent() {
    let translator = TruncatingTranslator::default();
    let out = translate_cues(&translator, &cues(7), "a", "b", plan(7, 1), |_, _| {}).await.unwrap();

    let texts: Vec<&str> = out.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, ["LINE 0", "LINE 1", "LINE 2", "LINE 3", "LINE 4", "LINE 5", "LINE 6"]);
    assert_eq!(translator.batch_sizes.into_inner().unwrap(), [7, 3, 1, 2, 4, 2, 2]);
}

struct AlwaysTruncated;

#[async_trait]
impl Translator for AlwaysTruncated {
    async fn translate(&self, _: BatchRequest<'_>) -> AppResult<Vec<String>> {
        Err(AppError::Truncated)
    }
}

#[tokio::test]
async fn a_single_truncated_line_is_an_error() {
    let err = translate_cues(&AlwaysTruncated, &cues(2), "a", "b", plan(10, 1), |_, _| {}).await.unwrap_err();
    assert!(matches!(err, AppError::Truncated));
}
