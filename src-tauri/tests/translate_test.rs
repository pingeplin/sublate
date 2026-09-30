use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use contents_title_lib::error::{AppError, AppResult};
use contents_title_lib::languages::find_target;
use contents_title_lib::metadata::{SubtitleTrack, TrackKind};
use contents_title_lib::subtitle::{parse_srt, Cue};
use contents_title_lib::subtitle_job::translate_subtitle_file;
use contents_title_lib::translate::{translate_cues, BatchRequest, TranslationPlan, Translator};

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
async fn translates_every_cue_in_order_preserving_timing() {
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
    let mut progress = progress.into_inner().unwrap();
    progress.sort();
    assert_eq!(progress, [(1, 3), (2, 3), (3, 3)]);
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
    std::fs::copy(
        format!("{}/tests/fixtures/ko_auto_rolling.srt", env!("CARGO_MANIFEST_DIR")),
        &source,
    )
    .unwrap();
    let original = std::fs::read_to_string(&source).unwrap();
    let track = SubtitleTrack { code: "ko-orig".into(), name: "Korean (Original)".into(), kind: TrackKind::Auto };
    let translator = UppercaseTranslator::default();

    let count = translate_subtitle_file(
        &translator,
        &source,
        &track,
        find_target("zh-TW").unwrap(),
        &output,
        TranslationPlan::default(),
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
