use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub native: &'static str,
}

pub const TARGET_LANGUAGES: &[Language] = &[
    Language { code: "zh-TW", name: "Traditional Chinese as used in Taiwan", native: "繁體中文（台灣）" },
    Language { code: "zh-CN", name: "Simplified Chinese as used in mainland China", native: "简体中文" },
    Language { code: "en", name: "English", native: "English" },
    Language { code: "ja", name: "Japanese", native: "日本語" },
    Language { code: "ko", name: "Korean", native: "한국어" },
    Language { code: "es", name: "Spanish", native: "Español" },
    Language { code: "fr", name: "French", native: "Français" },
    Language { code: "de", name: "German", native: "Deutsch" },
    Language { code: "vi", name: "Vietnamese", native: "Tiếng Việt" },
    Language { code: "th", name: "Thai", native: "ไทย" },
    Language { code: "id", name: "Indonesian", native: "Bahasa Indonesia" },
];

pub fn find_target(code: &str) -> Option<Language> {
    TARGET_LANGUAGES.iter().copied().find(|l| l.code == code)
}
