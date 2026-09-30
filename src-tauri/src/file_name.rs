use std::path::{Path, PathBuf};

/// APFS caps names at 255 UTF-16 units; leave room for suffixes such as ".ko-orig.srt"
/// and yt-dlp's intermediate ".f401.webm.part".
const MAX_STEM_UTF16: usize = 200;

/// Builds a portable file stem from a video title; falls back to `fallback` when nothing survives.
pub fn file_stem(title: &str, fallback: &str) -> String {
    let replaced: String = title.chars().map(portable_char).collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = truncate_utf16(trim_edges(&collapsed), MAX_STEM_UTF16);
    match trim_edges(stem) {
        "" => fallback.to_string(),
        stem => stem.to_string(),
    }
}

/// Where every file for one video goes: `<dir>/<stem>.<ext>` and `<dir>/<stem>.<lang>.srt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLocation {
    dir: PathBuf,
    stem: String,
}

impl OutputLocation {
    pub fn new(dir: impl Into<PathBuf>, title: &str, fallback: &str) -> Self {
        Self {
            dir: dir.into(),
            stem: file_stem(title, fallback),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn ytdlp_template(&self) -> String {
        self.dir
            .join(format!("{}.%(ext)s", escape_template(&self.stem)))
            .to_string_lossy()
            .into_owned()
    }

    pub fn subtitle(&self, lang: &str) -> PathBuf {
        self.dir.join(format!("{}.{lang}.srt", self.stem))
    }
}

/// yt-dlp output templates treat `%` as a field marker.
fn escape_template(literal: &str) -> String {
    literal.replace('%', "%%")
}

/// Characters forbidden on Windows/exFAT (and `/`, `:` on macOS) become full-width look-alikes.
fn portable_char(c: char) -> char {
    match c {
        '/' => '⧸',
        '\\' => '⧹',
        ':' => '：',
        '*' => '＊',
        '?' => '？',
        '"' => '＂',
        '<' => '＜',
        '>' => '＞',
        '|' => '｜',
        c if c.is_control() => ' ',
        c => c,
    }
}

/// Leading dots hide files on Unix; trailing dots and spaces are stripped by Windows.
fn trim_edges(s: &str) -> &str {
    s.trim_matches(|c: char| c == '.' || c.is_whitespace())
}

fn truncate_utf16(s: &str, max: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > max {
            return &s[..i];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_unicode_titles_readable() {
        assert_eq!(
            file_stem("[자막뉴스] 월급빼고 다 올랐다...한국은행의 결정은? / YTN", "id"),
            "[자막뉴스] 월급빼고 다 올랐다...한국은행의 결정은？ ⧸ YTN"
        );
    }

    #[test]
    fn replaces_every_reserved_character() {
        assert_eq!(file_stem(r#"a/b\c:d*e?f"g<h>i|j"#, "id"), "a⧸b⧹c：d＊e？f＂g＜h＞i｜j");
    }

    #[test]
    fn collapses_whitespace_and_control_characters() {
        assert_eq!(file_stem("  a\t\nb   c  ", "id"), "a b c");
    }

    #[test]
    fn strips_leading_and_trailing_dots() {
        assert_eq!(file_stem("...hidden. ", "id"), "hidden");
    }

    #[test]
    fn falls_back_when_title_is_empty() {
        assert_eq!(file_stem(" .. ", "abc123"), "abc123");
    }

    #[test]
    fn keeps_full_length_korean_titles() {
        assert_eq!(file_stem(&"한".repeat(100), "id"), "한".repeat(100));
    }

    #[test]
    fn truncates_by_utf16_units_on_char_boundary() {
        assert_eq!(file_stem(&"한".repeat(300), "id"), "한".repeat(MAX_STEM_UTF16));
        let emoji = file_stem(&"😀".repeat(150), "id");
        assert_eq!(emoji, "😀".repeat(MAX_STEM_UTF16 / 2));
    }

    #[test]
    fn location_derives_template_and_subtitle_paths_from_one_stem() {
        let out = OutputLocation::new("/o", "100% 확실? ", "id");
        assert_eq!(out.ytdlp_template(), "/o/100%% 확실？.%(ext)s");
        assert_eq!(out.subtitle("zh-TW"), PathBuf::from("/o/100% 확실？.zh-TW.srt"));
    }
}
