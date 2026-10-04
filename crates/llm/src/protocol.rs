//! 编号段 prompt 构建与回包 JSON 解析（三种 API 风格共用）。

use std::collections::BTreeMap;

use domain::language::Language;

use crate::LlmError;

/// 翻译指令 + 输出格式指令。
const FORMAT_INSTRUCTION: &str = "\
逐条翻译输入 JSON 对象中的每个编号段：键为编号，值为原文。
输出必须满足：
- 输出一个 JSON 对象，键与输入完全一致，值为对应编号的译文；
- 只输出 JSON 本身，不要 Markdown 代码块、解释或任何额外内容。";

pub fn build_system(source: Language, target: Language) -> String {
    format!("把以下文本从{source}翻译成{target}，只输出译文。\n\n{FORMAT_INSTRUCTION}")
}

/// 把编号段渲染为 JSON 对象文本，如 `{"1":"原文","2":"原文"}`。
pub fn build_user(segments: &BTreeMap<u64, String>) -> String {
    serde_json::to_string(segments).expect("BTreeMap<u64, String> 序列化不应失败")
}

/// 从模型回复解析出 `id -> 译文`。只接受请求中存在的 id；未知 id 忽略。
/// 直接 JSON 反序列化；模型偶尔用 Markdown 代码块或说明文字包裹，先剥离再解析。
/// 缺失部分 id 时返回部分结果（调用方决定是否重试）；一段都没解析出则报错。
pub fn parse_reply(segments: &BTreeMap<u64, String>, reply: &str) -> Result<BTreeMap<u64, String>, LlmError> {
    let json = extract_json(reply);
    let snippet: String = json.chars().take(500).collect();
    let mut out: BTreeMap<u64, String> =
        serde_json::from_str(json).map_err(|e| LlmError::Parse(format!("回复不是合法 JSON: {e}: {snippet}")))?;

    out.retain(|id, _| segments.contains_key(id));
    if out.is_empty() {
        return Err(LlmError::Parse(format!("回复中没有可解析的译文: {reply:?}")));
    }
    if out.len() < segments.len() {
        tracing::warn!(got = out.len(), want = segments.len(), "译文段缺失");
    }
    Ok(out)
}

/// 取第一个 `{` 到最后一个 `}` 之间的内容（剥离 Markdown 围栏与前后说明）。
fn extract_json(reply: &str) -> &str {
    let Some(start) = reply.find('{') else { return reply };
    let end = reply.rfind('}').map(|e| e + 1).unwrap_or(reply.len());
    &reply[start..end]
}

/// Anthropic 必填 `max_tokens` 的估算：按输入字符数 ×2 留余量，夹在 [1024, 65536]。
/// 若目标模型输出上限低于估算值，API 会直接报错——比静默截断更可发现。
pub fn estimate_max_tokens(segments: &BTreeMap<u64, String>) -> u32 {
    let chars: u64 = segments.values().map(|s| s.chars().count() as u64).sum();
    (chars * 2).clamp(1024, 65_536) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(pairs: &[(u64, &str)]) -> BTreeMap<u64, String> {
        pairs.iter().map(|&(id, s)| (id, s.to_string())).collect()
    }

    #[test]
    fn build_user_renders_json_object() {
        let c = ctx(&[(10, "a\nb"), (20, "c")]);
        assert_eq!(build_user(&c), r#"{"10":"a\nb","20":"c"}"#);
    }

    #[test]
    fn build_system_contains_languages_and_instruction() {
        let s = build_system(Language::ZhHans, Language::Ja);
        assert!(s.starts_with("把以下文本从简体中文翻译成日语"));
        assert!(s.contains("JSON 对象"));
    }

    #[test]
    fn parses_json_object() {
        let c = ctx(&[(1, "a"), (2, "b"), (3, "c")]);
        let out = parse_reply(&c, r#"{"1":"A","2":"B","3":"C"}"#).unwrap();
        assert_eq!(out, ctx(&[(1, "A"), (2, "B"), (3, "C")]));
    }

    #[test]
    fn ignores_unknown_keys() {
        let c = ctx(&[(1, "a"), (2, "b")]);
        let out = parse_reply(&c, r#"{"1":"A","999":"X","2":"B"}"#).unwrap();
        assert_eq!(out, ctx(&[(1, "A"), (2, "B")]));
    }

    #[test]
    fn strips_code_fence_and_preamble() {
        let c = ctx(&[(1, "a"), (2, "b")]);
        let reply = "以下是译文：\n```json\n{\"1\":\"A\",\"2\":\"B\"}\n```";
        let out = parse_reply(&c, reply).unwrap();
        assert_eq!(out, ctx(&[(1, "A"), (2, "B")]));
    }

    #[test]
    fn returns_partial_when_ids_missing() {
        let c = ctx(&[(1, "a"), (2, "b"), (3, "c")]);
        let out = parse_reply(&c, r#"{"1":"A","3":"C"}"#).unwrap();
        assert_eq!(out, ctx(&[(1, "A"), (3, "C")]));
    }

    #[test]
    fn errors_when_not_json() {
        let c = ctx(&[(1, "a")]);
        assert!(parse_reply(&c, "抱歉，无法翻译").is_err());
        assert!(parse_reply(&c, "").is_err());
    }

    #[test]
    fn estimates_max_tokens_with_floor_and_cap() {
        let c = ctx(&[(1, "hi")]);
        assert_eq!(estimate_max_tokens(&c), 1024);
        let big = ctx(&[(1, &"x".repeat(50_000))]);
        assert_eq!(estimate_max_tokens(&big), 65_536);
    }
}
