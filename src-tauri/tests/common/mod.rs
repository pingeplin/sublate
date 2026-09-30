#![allow(dead_code)]

use std::path::PathBuf;

use contents_title_lib::metadata::{SubtitleTrack, TrackKind};

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name)).unwrap()
}

pub fn ko_auto_track() -> SubtitleTrack {
    SubtitleTrack {
        code: "ko-orig".into(),
        name: "Korean (Original)".into(),
        kind: TrackKind::Auto,
    }
}
