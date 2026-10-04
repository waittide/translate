//! OpenAI Chat Completions 风格客户端（`<base_url>/chat/completions`）。

use std::collections::BTreeMap;

use domain::{
    language::Language,
    llm::{LlmConfig, LlmResponse},
};
use serde::{Deserialize, Serialize};

use crate::{LlmError, StyleClient, fetch, protocol, usage_from};

/// Chat Completions 风格客户端。
pub(crate) struct Client;

#[derive(Serialize)]
struct Request {
    model:       String,
    messages:    Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p:       Option<f64>,
}

#[derive(Serialize)]
struct Message {
    role:    &'static str,
    content: String,
}

#[derive(Deserialize)]
struct Response {
    choices: Vec<Choice>,
    usage:   Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageOut,
}

#[derive(Deserialize)]
struct MessageOut {
    content: Option<Content>,
}

/// 部分代理返回 string，部分返回内容块数组。
#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    String(String),
    Parts(Vec<Part>),
}

#[derive(Deserialize)]
struct Part {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens:     Option<u64>,
    completion_tokens: Option<u64>,
}

impl StyleClient for Client {
    async fn translate(
        &self,
        config: &LlmConfig,
        http: &reqwest::Client,
        source: Language,
        target: Language,
        segments: BTreeMap<u64, String>,
    ) -> Result<LlmResponse, LlmError> {
        let body = Request {
            model:       config.model.clone(),
            messages:    vec![
                Message {
                    role:    "system",
                    content: protocol::build_system(source, target),
                },
                Message {
                    role:    "user",
                    content: protocol::build_user(&segments),
                },
            ],
            temperature: config.temperature,
            top_p:       config.top_p,
        };
        let url = format!("{}/chat/completions", config.base_url);
        let parsed: Response = fetch(http.post(&url).bearer_auth(&config.api_key), Some(&body)).await?;

        let reply = match parsed
            .choices
            .first()
            .and_then(|c| c.message.content.as_ref())
        {
            Some(Content::String(s)) => s.clone(),
            Some(Content::Parts(parts)) => parts
                .iter()
                .filter_map(|p| p.text.clone())
                .collect::<Vec<_>>()
                .join("\n"),
            None => String::new(),
        };
        let segments = protocol::parse_reply(&segments, &reply)?;
        let usage = parsed
            .usage
            .map_or_else(Default::default, |u| usage_from(u.prompt_tokens, u.completion_tokens));
        Ok(LlmResponse { segments, usage })
    }

    async fn models(&self, config: &LlmConfig, http: &reqwest::Client) -> Result<Vec<String>, LlmError> {
        let url = format!("{}/models", config.base_url);
        // GET 无请求体；`&()` 仅作类型标注，None 分支不序列化
        let parsed: crate::ModelsResponse = fetch(http.get(&url).bearer_auth(&config.api_key), None::<&()>).await?;
        Ok(parsed.data.into_iter().map(|m| m.id).collect())
    }
}

#[cfg(test)]
mod tests {
    use domain::llm::ApiStyle;

    use super::*;

    fn config(base_url: String) -> LlmConfig {
        LlmConfig {
            api_style: ApiStyle::Completion,
            base_url,
            api_key: "k".into(),
            model: "m".into(),
            temperature: None,
            top_p: Some(0.9),
            top_k: None,
        }
    }

    fn segments() -> BTreeMap<u64, String> {
        BTreeMap::from([(1, "hello".into()), (2, "thanks".into())])
    }

    #[tokio::test]
    async fn translate_completion_style() {
        let (addr, mut rx) = test_support::spawn(
            r#"{"choices":[{"message":{"content":"{\"1\":\"こんにちは\",\"2\":\"ありがとう\"}"}}],"usage":{"prompt_tokens":5,"completion_tokens":7}}"#,
            200,
        )
        .await;
        let http = reqwest::Client::new();
        let out = Client
            .translate(&config(addr), &http, Language::ZhHans, Language::Ja, segments())
            .await
            .unwrap();

        let req = rx.recv().await.unwrap();
        assert_eq!(req.path, "/chat/completions");
        assert_eq!(req.headers["authorization"], "Bearer k");
        let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(body["model"], "m");
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("从简体中文翻译成日语")
        );
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], r#"{"1":"hello","2":"thanks"}"#);
        assert_eq!(body["top_p"], 0.9);
        assert!(body.get("temperature").is_none());

        assert_eq!(out.segments.get(&1).unwrap(), "こんにちは");
        assert_eq!(out.segments.get(&2).unwrap(), "ありがとう");
        assert_eq!(out.usage.input, 5);
        assert_eq!(out.usage.output, 7);
    }

    #[tokio::test]
    async fn content_as_parts_and_missing_usage() {
        let (addr, _rx) = test_support::spawn(
            r#"{"choices":[{"message":{"content":[{"type":"text","text":"{\"1\":\"hi\",\"2\":\"bye\"}"}]}}],"usage":null}"#,
            200,
        )
        .await;
        let http = reqwest::Client::new();
        let out = Client
            .translate(&config(addr), &http, Language::ZhHans, Language::Ja, segments())
            .await
            .unwrap();
        assert_eq!(out.segments.get(&1).unwrap(), "hi");
        assert_eq!(out.segments.get(&2).unwrap(), "bye");
        assert_eq!(out.usage.input, 0);
        assert_eq!(out.usage.output, 0);
    }
}
