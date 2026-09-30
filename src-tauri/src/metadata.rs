use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

const ORIGINAL_SUFFIX: &str = "-orig";
const EXCLUDED_TRACKS: &[&str] = &["live_chat"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Manual,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub code: String,
    pub name: String,
    pub kind: TrackKind,
}

impl SubtitleTrack {
    pub fn language_name(&self) -> &str {
        self.name
            .split_once(" (")
            .map_or(self.name.as_str(), |(base, _)| base)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub url: String,
    pub subtitles: Vec<SubtitleTrack>,
}

type RawTracks = BTreeMap<String, Vec<RawFormat>>;

#[derive(Deserialize)]
struct RawFormat {
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawMetadata {
    id: String,
    title: String,
    thumbnail: Option<String>,
    duration: Option<f64>,
    webpage_url: Option<String>,
    language: Option<String>,
    subtitles: Option<RawTracks>,
    automatic_captions: Option<RawTracks>,
}

pub fn parse_metadata(json: &str, requested_url: &str) -> serde_json::Result<VideoMetadata> {
    let raw: RawMetadata = serde_json::from_str(json)?;
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
    })
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
