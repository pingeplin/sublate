mod common;

use common::{fixture, ko_auto_track};
use sublate_core::error::AppError;
use sublate_core::metadata::{parse_metadata, SubtitleTrack, TrackKind};

#[test]
fn auto_captions_expose_only_the_spoken_language_track() {
    let meta = parse_metadata(&fixture("ko_auto.json"), "https://youtu.be/SrvYHXmiLAY").unwrap();

    assert_eq!(meta.id, "SrvYHXmiLAY");
    assert!(meta.title.contains("한국은행"));
    assert!(meta.thumbnail.unwrap().starts_with("https://"));
    assert_eq!(meta.url, "https://www.youtube.com/watch?v=SrvYHXmiLAY");
    assert_eq!(meta.subtitles, vec![ko_auto_track()]);
    assert_eq!(meta.subtitles[0].language_name(), "Korean");
}

#[test]
fn manual_subtitles_are_listed_before_auto_tracks() {
    let meta = parse_metadata(&fixture("en_manual.json"), "u").unwrap();
    let listed: Vec<(&str, TrackKind)> = meta
        .subtitles
        .iter()
        .map(|t| (t.code.as_str(), t.kind))
        .collect();

    assert_eq!(listed[..2], [("de", TrackKind::Manual), ("en", TrackKind::Manual)]);
    assert!(listed[2..].iter().all(|(_, kind)| *kind == TrackKind::Auto));
}

#[test]
fn null_subtitle_maps_and_live_chat_are_tolerated() {
    let json = r#"{
        "id": "x", "title": "t", "thumbnail": null, "duration": null,
        "webpage_url": null, "language": null,
        "subtitles": { "live_chat": [{ "ext": "json", "name": "Live chat" }] },
        "automatic_captions": null
    }"#;
    let meta = parse_metadata(json, "https://example.com/v").unwrap();
    assert_eq!(meta.url, "https://example.com/v");
    assert!(meta.subtitles.is_empty());
}

#[test]
fn falls_back_to_video_language_when_no_original_track() {
    let json = r#"{
        "id": "x", "title": "t", "language": "ja",
        "automatic_captions": {
            "en": [{ "name": "English" }],
            "ja": [{ "name": "Japanese" }]
        }
    }"#;
    let meta = parse_metadata(json, "u").unwrap();
    assert_eq!(meta.subtitles.len(), 1);
    assert_eq!(meta.subtitles[0].code, "ja");
    assert_eq!(meta.subtitles[0].kind, TrackKind::Auto);
}

#[test]
fn language_name_keeps_qualifiers_but_drops_original_marker() {
    let track = |name: &str| SubtitleTrack { code: "x".into(), name: name.into(), kind: TrackKind::Manual };
    assert_eq!(track("Chinese (Traditional)").language_name(), "Chinese (Traditional)");
    assert_eq!(track("English").language_name(), "English");
}

#[test]
fn playlists_are_rejected() {
    for kind in ["playlist", "multi_video"] {
        let json = format!(r#"{{ "_type": "{kind}", "id": "PL1", "title": "List", "entries": [] }}"#);
        assert!(matches!(parse_metadata(&json, "u"), Err(AppError::Unsupported(_))));
    }
    let video = r#"{ "_type": "video", "id": "x", "title": "t" }"#;
    assert!(parse_metadata(video, "u").is_ok());
}
