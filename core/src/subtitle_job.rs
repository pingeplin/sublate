use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::error::AppResult;
use crate::file_name::OutputLocation;
use crate::languages::Language;
use crate::metadata::{SubtitleTrack, TrackKind};
use crate::punctuation::normalize;
use crate::subtitle::{absorb_empty_cues, collapse_rolling, parse_srt, to_srt, Cue};
use crate::translate::{translate_cues, TranslationPlan, Translator};

/// Where a video's subtitle tracks come from.
#[async_trait]
pub trait SubtitleSource: Send + Sync {
    /// Writes the track at `out.subtitle(&track.code)` and returns that path.
    async fn download_subtitle(
        &self,
        url: &str,
        out: &OutputLocation,
        track: &SubtitleTrack,
    ) -> AppResult<PathBuf>;
}

#[derive(Debug)]
pub struct SubtitleOutput {
    pub source_path: PathBuf,
    pub translated_path: PathBuf,
    pub cue_count: usize,
}

/// Downloads a subtitle track and writes its translation next to it.
pub async fn download_and_translate(
    source: &dyn SubtitleSource,
    translator: &dyn Translator,
    url: &str,
    out: &OutputLocation,
    track: &SubtitleTrack,
    target: Language,
    on_progress: impl Fn(usize, usize) + Sync,
) -> AppResult<SubtitleOutput> {
    let source_path = source.download_subtitle(url, out, track).await?;
    let translated_path = out.translation(&track.code, target.code);
    let cue_count =
        translate_subtitle_file(translator, &source_path, track, target, &translated_path, on_progress)
            .await?;
    Ok(SubtitleOutput {
        source_path,
        translated_path,
        cue_count,
    })
}

/// Reads a subtitle, translates it, cleans the result (punctuation, empty cues), and
/// writes it as a standalone .srt.
pub async fn translate_subtitle_file(
    translator: &dyn Translator,
    source_path: &Path,
    track: &SubtitleTrack,
    target: Language,
    output_path: &Path,
    on_progress: impl Fn(usize, usize) + Sync,
) -> AppResult<usize> {
    let raw = tokio::fs::read_to_string(source_path).await?;
    let mut cues = parse_srt(&raw)?;
    if track.kind == TrackKind::Auto {
        cues = collapse_rolling(cues);
    }
    let translated = translate_cues(
        translator,
        &cues,
        track.language_name(),
        target.name,
        TranslationPlan::default(),
        on_progress,
    )
    .await?;
    let normalized = translated
        .into_iter()
        .map(|cue| Cue {
            text: normalize(&cue.text, target.punctuation),
            ..cue
        })
        .collect();
    let cleaned = absorb_empty_cues(normalized);
    tokio::fs::write(output_path, to_srt(&cleaned)).await?;
    Ok(cleaned.len())
}
