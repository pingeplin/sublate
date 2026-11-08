use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::error::{AppError, AppResult};

const ORIGINAL_SUFFIX: &str = "-orig";
const EXCLUDED_TRACKS: &[&str] = &["live_chat"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrackKind {
    Manual,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SubtitleTrack {
    pub code: String,
    pub name: String,
    pub kind: TrackKind,
}

impl SubtitleTrack {
    /// Drops yt-dlp's "(Original)" marker on spoken-language ASR tracks while keeping real
    /// qualifiers such as "Chinese (Traditional)".
    pub fn language_name(&self) -> &str {
        self.name.strip_suffix(" (Original)").unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct VideoMetadata {
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub url: String,
    pub subtitles: Vec<SubtitleTrack>,
    /// Heights of the video streams on offer, largest first.
    pub heights: Vec<u32>,
}

type RawTracks = BTreeMap<String, Vec<RawFormat>>;

#[derive(Deserialize)]
struct RawFormat {
    name: Option<String>,
}

/// yt-dlp marks a stream without video as `vcodec: "none"`; a missing codec is unknown and,
/// as in yt-dlp's own `hasvid` sort field, still counts as video.
#[derive(Deserialize)]
struct RawStream {
    vcodec: Option<String>,
    height: Option<f64>,
}

impl RawStream {
    fn video_height(&self) -> Option<u32> {
        let has_video = self.vcodec.as_deref() != Some("none");
        self.height.filter(|h| has_video && *h >= 1.0).map(|h| h as u32)
    }
}

#[derive(Deserialize)]
struct RawMetadata {
    #[serde(rename = "_type")]
    kind: Option<String>,
    id: String,
    title: String,
    thumbnail: Option<String>,
    duration: Option<f64>,
    webpage_url: Option<String>,
    language: Option<String>,
    subtitles: Option<RawTracks>,
    automatic_captions: Option<RawTracks>,
    formats: Option<Vec<RawStream>>,
}

pub fn parse_metadata(json: &str, requested_url: &str) -> AppResult<VideoMetadata> {
    let raw: RawMetadata = serde_json::from_str(json)?;
    if matches!(raw.kind.as_deref(), Some("playlist" | "multi_video")) {
        return Err(AppError::Unsupported(
            "playlists aren't supported; paste the URL of a single video".into(),
        ));
    }
    let mut subtitles = manual_tracks(raw.subtitles.as_ref());
    subtitles.extend(original_auto_tracks(
        raw.automatic_captions.as_ref(),
        raw.language.as_deref(),
    ));
    Ok(VideoMetadata {
        url: raw.webpage_url.unwrap_or_else(|| requested_url.to_string()),
        id: raw.id,
        title: raw.title,
        thumbnail: raw.thumbnail,
        duration: raw.duration,
        subtitles,
        heights: video_heights(raw.formats.as_deref().unwrap_or_default()),
    })
}

fn video_heights(streams: &[RawStream]) -> Vec<u32> {
    let heights: BTreeSet<u32> = streams.iter().filter_map(RawStream::video_height).collect();
    heights.into_iter().rev().collect()
}

fn manual_tracks(tracks: Option<&RawTracks>) -> Vec<SubtitleTrack> {
    tracks
        .into_iter()
        .flatten()
        .filter(|(code, _)| !EXCLUDED_TRACKS.contains(&code.as_str()))
        .map(|(code, formats)| to_track(code, formats, TrackKind::Manual))
        .collect()
}

/// YouTube lists the spoken-language ASR track plus ~150 machine translations of it;
/// only the spoken-language track is a meaningful translation source.
fn original_auto_tracks(tracks: Option<&RawTracks>, language: Option<&str>) -> Vec<SubtitleTrack> {
    let Some(tracks) = tracks else {
        return Vec::new();
    };
    let originals: Vec<SubtitleTrack> = tracks
        .iter()
        .filter(|(code, _)| code.ends_with(ORIGINAL_SUFFIX))
        .map(|(code, formats)| to_track(code, formats, TrackKind::Auto))
        .collect();
    if !originals.is_empty() {
        return originals;
    }
    language
        .and_then(|lang| tracks.get_key_value(lang))
        .map(|(code, formats)| to_track(code, formats, TrackKind::Auto))
        .into_iter()
        .collect()
}

fn to_track(code: &str, formats: &[RawFormat], kind: TrackKind) -> SubtitleTrack {
    let name = formats
        .iter()
        .find_map(|f| f.name.clone())
        .unwrap_or_else(|| code.to_string());
    SubtitleTrack {
        code: code.to_string(),
        name,
        kind,
    }
}
