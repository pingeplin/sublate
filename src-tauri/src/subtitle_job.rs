use std::path::Path;

use crate::error::AppResult;
use crate::languages::Language;
use crate::metadata::{SubtitleTrack, TrackKind};
use crate::punctuation::normalize;
use crate::subtitle::{absorb_empty_cues, collapse_rolling, parse_srt, to_srt, Cue};
use crate::translate::{translate_cues, TranslationPlan, Translator};

/// Reads a downloaded subtitle, translates it, cleans the result (punctuation, empty
/// cues), and writes a standalone .srt next to it.
pub async fn translate_subtitle_file(
    translator: &dyn Translator,
    source_path: &Path,
    track: &SubtitleTrack,
    target: Language,
    output_path: &Path,
    plan: TranslationPlan,
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
        plan,
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
