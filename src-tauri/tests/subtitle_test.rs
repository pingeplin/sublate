use contents_title_lib::subtitle::{collapse_rolling, parse_srt, to_srt, Cue};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn cue(start_ms: u64, end_ms: u64, text: &str) -> Cue {
    Cue { start_ms, end_ms, text: text.into() }
}

#[test]
fn parses_crlf_bom_and_multiline_cues() {
    let srt = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,500\r\nHello\r\nworld\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nBye\r\n";
    assert_eq!(
        parse_srt(srt).unwrap(),
        vec![cue(1000, 2500, "Hello\nworld"), cue(3000, 4000, "Bye")]
    );
}

#[test]
fn serializes_with_sequential_indices() {
    let srt = to_srt(&[cue(0, 1000, "A"), cue(61_000, 3_723_004, "B\nC")]);
    assert_eq!(
        srt,
        "1\n00:00:00,000 --> 00:00:01,000\nA\n\n2\n00:01:01,000 --> 01:02:03,004\nB\nC\n"
    );
}

#[test]
fn round_trips_through_parse_and_serialize() {
    let cues = vec![cue(840, 2869, "첫 줄"), cue(2879, 5390, "둘째\n셋째")];
    assert_eq!(parse_srt(&to_srt(&cues)).unwrap(), cues);
}

#[test]
fn rejects_bad_timestamps() {
    assert!(parse_srt("1\n00:00 --> 00:01\nx\n").is_err());
}

#[test]
fn collapses_youtube_rolling_auto_captions() {
    let cues = collapse_rolling(parse_srt(&fixture("ko_auto_rolling.srt")).unwrap());

    assert_eq!(
        cues[..3],
        [
            cue(500, 4100, "오늘은 집에서 보리차를 끓이는 방법을"),
            cue(4110, 7620, "차근차근 알려 드리려고 합니다 먼저"),
            cue(7630, 10960, "냄비에 물을 이 리터 정도 붓고"),
        ]
    );
    for pair in cues.windows(2) {
        assert!(pair[0].start_ms <= pair[1].start_ms);
        assert_ne!(pair[0].text.lines().last(), pair[1].text.lines().next());
    }
}

#[test]
fn collapse_keeps_distinct_consecutive_cues() {
    let cues = vec![cue(0, 1000, "A"), cue(1000, 2000, "B")];
    assert_eq!(collapse_rolling(cues.clone()), cues);
}
