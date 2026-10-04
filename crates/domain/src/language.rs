use serde::{Deserialize, Serialize};

/// 语言
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// 简体中文
    #[serde(rename = "zh-Hans")]
    ZhHans,
    /// 繁体中文
    #[serde(rename = "zh-Hant")]
    ZhHant,
    /// 英语
    En,
    /// 日语
    Ja,
    /// 韩语
    Ko,
    /// 俄语
    Ru,
    /// 法语
    Fr,
    /// 德语
    De,
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::ZhHans => "简体中文",
            Self::ZhHant => "繁体中文",
            Self::En => "英语",
            Self::Ja => "日语",
            Self::Ko => "韩语",
            Self::Ru => "俄语",
            Self::Fr => "法语",
            Self::De => "德语",
        };
        f.write_str(name)
    }
}
