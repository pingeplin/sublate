mod common;

use common::fixture;
use sublate_core::subtitle::{absorb_empty_cues, collapse_rolling, parse_srt, to_srt, Cue};

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

#[test]
fn empty_cues_extend_the_previous_cue() {
    let cues = vec![cue(0, 1000, "A"), cue(1010, 2000, " "), cue(2010, 3000, ""), cue(3010, 4000, "B")];
    assert_eq!(absorb_empty_cues(cues), [cue(0, 3000, "A"), cue(3010, 4000, "B")]);
}

#[test]
fn leading_empty_cues_extend_the_next_cue_backwards() {
    let cues = vec![cue(0, 1000, ""), cue(1010, 2000, ""), cue(2010, 3000, "A")];
    assert_eq!(absorb_empty_cues(cues), [cue(0, 3000, "A")]);
}

#[test]
fn all_empty_cues_yield_nothing() {
    assert!(absorb_empty_cues(vec![cue(0, 1000, "")]).is_empty());
}

#[test]
fn whitespace_only_separators_do_not_merge_cues() {
    let srt = "1\n00:00:01,000 --> 00:00:02,000\nHello\n \n2\n00:00:03,000 --> 00:00:04,000\nBye\n\t\n";
    assert_eq!(parse_srt(srt).unwrap(), [cue(1000, 2000, "Hello"), cue(3000, 4000, "Bye")]);
}

#[test]
fn numeric_cue_text_is_kept() {
    let srt = "1\n00:00:01,000 --> 00:00:02,000\n2024\n\n2\n00:00:03,000 --> 00:00:04,000\nBye\n";
    assert_eq!(parse_srt(srt).unwrap(), [cue(1000, 2000, "2024"), cue(3000, 4000, "Bye")]);
}

#[test]
fn serializer_drops_blank_lines_inside_cue_text() {
    let cues = [cue(0, 1000, "\n第一句\n\n 第二句 \n"), cue(1000, 2000, "下一句")];
    assert_eq!(
        parse_srt(&to_srt(&cues)).unwrap(),
        [cue(0, 1000, "第一句\n第二句"), cue(1000, 2000, "下一句")]
    );
}

#[test]
fn collapse_tolerates_leading_empty_cue() {
    let cues = vec![cue(0, 500, ""), cue(500, 1000, "A"), cue(1000, 1010, "A")];
    assert_eq!(collapse_rolling(cues), [cue(500, 1000, "A")]);
}
