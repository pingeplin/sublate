use serde::Serialize;

use crate::punctuation::PunctuationStyle::{self, Chinese, Japanese, Latin};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub native: &'static str,
    #[serde(skip)]
    pub punctuation: PunctuationStyle,
}

#[rustfmt::skip]
pub const TARGET_LANGUAGES: &[Language] = &[
    Language { code: "zh-TW", name: "Traditional Chinese as used in Taiwan", native: "繁體中文（台灣）", punctuation: Chinese },
    Language { code: "zh-CN", name: "Simplified Chinese as used in mainland China", native: "简体中文", punctuation: Chinese },
    Language { code: "en", name: "English", native: "English", punctuation: Latin },
    Language { code: "ja", name: "Japanese", native: "日本語", punctuation: Japanese },
    Language { code: "ko", name: "Korean", native: "한국어", punctuation: Latin },
    Language { code: "es", name: "Spanish", native: "Español", punctuation: Latin },
    Language { code: "fr", name: "French", native: "Français", punctuation: Latin },
    Language { code: "de", name: "German", native: "Deutsch", punctuation: Latin },
    Language { code: "vi", name: "Vietnamese", native: "Tiếng Việt", punctuation: Latin },
    Language { code: "th", name: "Thai", native: "ไทย", punctuation: Latin },
    Language { code: "id", name: "Indonesian", native: "Bahasa Indonesia", punctuation: Latin },
];

pub fn find_target(code: &str) -> Option<Language> {
    TARGET_LANGUAGES.iter().copied().find(|l| l.code == code)
}
