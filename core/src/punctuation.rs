#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PunctuationStyle {
    Latin,
    Chinese,
    Japanese,
}

/// Converts half-width punctuation into full-width forms when it sits next to CJK text or
/// ends a line containing CJK. Marks between ASCII characters (`3.5`, `1,000`, `10:30`,
/// `http://`) and dot runs (`...`) are kept.
pub fn normalize(text: &str, style: PunctuationStyle) -> String {
    if style == PunctuationStyle::Latin {
        return text.to_string();
    }
    text.split('\n')
        .map(|line| normalize_line(line, style))
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_line(line: &str, style: PunctuationStyle) -> String {
    let chars: Vec<char> = line.chars().collect();
    let line_has_cjk = chars.iter().copied().any(is_cjk);
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let context = Context {
            prev: out.chars().rev().find(|&p| !is_inline_space(p)),
            next: chars[i + 1..].iter().copied().find(|&n| !is_inline_space(n)),
            line_has_cjk,
        };
        let in_dot_run = c == '.' && ((i > 0 && chars[i - 1] == '.') || chars.get(i + 1) == Some(&'.'));
        match full_width(c, style).filter(|_| !in_dot_run && context.should_convert(c)) {
            Some(mark) => {
                out.truncate(out.trim_end_matches(is_inline_space).len());
                out.push(mark);
                while chars.get(i + 1).is_some_and(|&n| is_inline_space(n)) {
                    i += 1;
                }
            }
            None => out.push(c),
        }
        i += 1;
    }
    out
}

struct Context {
    prev: Option<char>,
    next: Option<char>,
    line_has_cjk: bool,
}

impl Context {
    fn should_convert(&self, c: char) -> bool {
        let prev_cjk = self.prev.is_some_and(is_cjk);
        let ends_cjk_line = self.next.is_none() && self.line_has_cjk;
        match c {
            '.' => prev_cjk || ends_cjk_line,
            _ => prev_cjk || self.next.is_some_and(is_cjk) || ends_cjk_line,
        }
    }
}

fn full_width(c: char, style: PunctuationStyle) -> Option<char> {
    Some(match c {
        ',' if style == PunctuationStyle::Japanese => '、',
        ',' => '，',
        '.' => '。',
        '?' => '？',
        '!' => '！',
        ':' => '：',
        ';' => '；',
        _ => return None,
    })
}

fn is_inline_space(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3000}'..='\u{303F}'   // CJK symbols and punctuation
        | '\u{3040}'..='\u{30FF}' // Hiragana, Katakana
        | '\u{3100}'..='\u{312F}' // Bopomofo
        | '\u{3400}'..='\u{4DBF}' // CJK extension A
        | '\u{4E00}'..='\u{9FFF}' // CJK unified ideographs
        | '\u{F900}'..='\u{FAFF}' // CJK compatibility ideographs
        | '\u{FF01}'..='\u{FF60}' // Full-width forms
        | '\u{20000}'..='\u{2FFFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use PunctuationStyle::*;

    #[test]
    fn converts_marks_after_chinese_and_drops_surrounding_spaces() {
        assert_eq!(
            normalize("天然氣供應中斷持續, 政府於上個月調漲了電費.", Chinese),
            "天然氣供應中斷持續，政府於上個月調漲了電費。"
        );
        assert_eq!(normalize("真的嗎 ?  太好了!", Chinese), "真的嗎？太好了！");
        assert_eq!(normalize("時間:十點; 地點:台北", Chinese), "時間：十點；地點：台北");
    }

    #[test]
    fn converts_marks_between_latin_and_chinese() {
        assert_eq!(normalize("OK?好", Chinese), "OK？好");
        assert_eq!(normalize("用 iPhone, 也用 Android.", Chinese), "用 iPhone，也用 Android。");
    }

    #[test]
    fn converts_marks_ending_a_chinese_line_after_latin_or_digits() {
        assert_eq!(normalize("你用 iPhone?", Chinese), "你用 iPhone？");
        assert_eq!(normalize("漲了 3.5%.", Chinese), "漲了 3.5%。");
        assert_eq!(normalize("共 1,000,\n第二行", Chinese), "共 1,000，\n第二行");
    }

    #[test]
    fn line_end_rule_ignores_lines_without_cjk() {
        assert_eq!(normalize("第一行\nVersion 2.0.", Chinese), "第一行\nVersion 2.0.");
    }

    #[test]
    fn keeps_numbers_times_urls_and_ellipses() {
        for text in [
            "漲了 3.5%",
            "共 1,000 元",
            "10:30 開始",
            "請看 https://example.com/a.b",
            "所以...",
            "等等..這樣",
        ] {
            assert_eq!(normalize(text, Chinese), text);
        }
    }

    #[test]
    fn japanese_uses_ideographic_comma() {
        assert_eq!(normalize("政府は, 先月値上げした.", Japanese), "政府は、先月値上げした。");
    }

    #[test]
    fn latin_style_is_untouched() {
        assert_eq!(normalize("Hello , world .", Latin), "Hello , world .");
    }

    #[test]
    fn preserves_line_breaks_and_is_idempotent() {
        let once = normalize("第一行,\n第二行.", Chinese);
        assert_eq!(once, "第一行，\n第二行。");
        assert_eq!(normalize(&once, Chinese), once);
    }
}
