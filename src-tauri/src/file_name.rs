use std::path::{Path, PathBuf};

/// Longest stem that leaves room for suffixes such as ".ko-orig.srt" and yt-dlp's
/// intermediate ".f401.webm.part" under the 255-unit name limit.
const MAX_STEM: usize = 200;

/// `<title> [<id>]`: readable, and unique per video so same-titled videos never share files.
pub fn file_stem(title: &str, id: &str) -> String {
    let id = portable(id);
    let suffix = format!(" [{id}]");
    let budget = MAX_STEM.saturating_sub(suffix.chars().map(name_units).sum());
    match trim_edges(truncate(trim_edges(&portable(title)), budget)) {
        "" => id,
        title => format!("{title}{suffix}"),
    }
}

/// Where every file for one video goes: `<dir>/<stem>.<ext>` and `<dir>/<stem>.<lang>.srt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLocation {
    dir: PathBuf,
    stem: String,
}

impl OutputLocation {
    pub fn new(dir: impl Into<PathBuf>, title: &str, id: &str) -> Self {
        Self {
            dir: dir.into(),
            stem: file_stem(title, id),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Relative to `dir`; yt-dlp runs inside it.
    pub fn ytdlp_template(&self) -> String {
        format!("{}.%(ext)s", escape_template(&self.stem))
    }

    pub fn subtitle(&self, lang: &str) -> PathBuf {
        self.dir.join(format!("{}.{lang}.srt", self.stem))
    }

    /// Never the source track's own path, even when translating e.g. `en` to `en`.
    pub fn translation(&self, source_lang: &str, target_lang: &str) -> PathBuf {
        if source_lang == target_lang {
            self.subtitle(&format!("{target_lang}.translated"))
        } else {
            self.subtitle(target_lang)
        }
    }
}

/// yt-dlp output templates treat `%` as a field marker.
fn escape_template(literal: &str) -> String {
    literal.replace('%', "%%")
}

fn portable(s: &str) -> String {
    let replaced: String = s.chars().map(portable_char).collect();
    replaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Characters forbidden on Windows/exFAT (and `/`, `:` on macOS) become full-width look-alikes,
/// as does `$`, which yt-dlp would expand as an environment variable.
fn portable_char(c: char) -> char {
    match c {
        '$' => '＄',
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

/// Name-length units: Linux filesystems count bytes; APFS and NTFS count UTF-16 units.
fn name_units(c: char) -> usize {
    if cfg!(target_os = "linux") {
        c.len_utf8()
    } else {
        c.len_utf16()
    }
}

fn truncate(s: &str, max: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += name_units(c);
        if units > max {
            return &s[..i];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title_part(stem: &str) -> &str {
        stem.strip_suffix(" [id]").expect("id suffix")
    }

    #[test]
    fn appends_the_video_id_to_readable_titles() {
        assert_eq!(
            file_stem("[자막뉴스] 월급빼고 다 올랐다...한국은행의 결정은? / YTN", "SrvYHXmiLAY"),
            "[자막뉴스] 월급빼고 다 올랐다...한국은행의 결정은？ ⧸ YTN [SrvYHXmiLAY]"
        );
    }

    #[test]
    fn same_title_different_videos_get_different_stems() {
        assert_ne!(file_stem("Live Stream", "a1"), file_stem("Live Stream", "b2"));
    }

    #[test]
    fn replaces_every_reserved_character() {
        assert_eq!(
            title_part(&file_stem(r#"a/b\c:d*e?f"g<h>i|j$k"#, "id")),
            "a⧸b⧹c：d＊e？f＂g＜h＞i｜j＄k"
        );
    }

    #[test]
    fn collapses_whitespace_and_trims_dots() {
        assert_eq!(file_stem("  a\t\nb   c  ", "id"), "a b c [id]");
        assert_eq!(file_stem("...hidden. ", "id"), "hidden [id]");
    }

    #[test]
    fn uses_the_id_alone_when_title_is_empty() {
        assert_eq!(file_stem(" .. ", "abc123"), "abc123");
    }

    #[test]
    fn keeps_full_length_korean_titles() {
        assert_eq!(title_part(&file_stem(&"한".repeat(100), "id")), "한".repeat(100));
    }

    #[test]
    fn truncates_title_but_keeps_id_within_limit() {
        let stem = file_stem(&"😀".repeat(150), "id");
        assert!(stem.chars().map(name_units).sum::<usize>() <= MAX_STEM);
        assert!(title_part(&stem).chars().all(|c| c == '😀'));
    }

    #[test]
    fn location_derives_template_and_subtitle_paths_from_one_stem() {
        let out = OutputLocation::new("/o", "100% 확실? ", "id");
        assert_eq!(out.ytdlp_template(), "100%% 확실？ [id].%(ext)s");
        assert_eq!(out.subtitle("zh-TW"), PathBuf::from("/o/100% 확실？ [id].zh-TW.srt"));
    }

    #[test]
    fn translation_never_overwrites_its_source() {
        let out = OutputLocation::new("/o", "T", "id");
        assert_eq!(out.translation("ko-orig", "zh-TW"), PathBuf::from("/o/T [id].zh-TW.srt"));
        assert_eq!(out.translation("en", "en"), PathBuf::from("/o/T [id].en.translated.srt"));
    }
}
